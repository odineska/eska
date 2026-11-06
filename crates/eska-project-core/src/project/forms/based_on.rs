//! Discover reverse `BasedOn` relations from declared application metadata.

use std::collections::HashSet;

use super::{
    AutoCommandItem, FormResource, PropertySource,
    commands::{effective_metadata, metadata_caption},
    presentation::{literal, metadata_property},
    resources::{configuration_objects, direct_property},
};
use crate::project::{
    metadata_model::{MetadataKind, MetadataValue, ObjectId, PropertyKey},
    metadata_parser::{self, MetadataReference, ParsedDescriptor},
};

const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";

pub(super) const fn supports_creation(kind: MetadataKind) -> bool {
    matches!(
        kind,
        MetadataKind::Catalog
            | MetadataKind::Document
            | MetadataKind::ExchangePlan
            | MetadataKind::ChartOfCharacteristicTypes
            | MetadataKind::ChartOfAccounts
            | MetadataKind::ChartOfCalculationTypes
            | MetadataKind::InformationRegister
            | MetadataKind::BusinessProcess
            | MetadataKind::Task
    )
}

pub(super) fn basis(owner_type: &PropertyKey) -> Option<String> {
    let (kind, name) = owner_type.name.split_once('.')?;
    let tag = kind.strip_suffix("Object")?;
    supports_creation(MetadataKind::from_xml_tag(tag).ok()?).then(|| format!("{tag}.{name}"))
}

pub(super) fn candidates(
    descriptor: &ParsedDescriptor,
) -> impl Iterator<Item = &MetadataReference> {
    configuration_objects(descriptor).filter(|reference| supports_creation(reference.kind))
}

/// Skip the self-closing property emitted for objects without a basis. A prefixed
/// spelling still reaches the namespace-aware check below.
pub(super) fn may_declare(input: &str) -> bool {
    input.contains("</BasedOn") || input.contains(":BasedOn")
}

/// Only inspect the target's direct metadata property.
pub(super) fn declares(
    input: &str,
    kind: MetadataKind,
    basis: &str,
) -> Result<bool, metadata_parser::ParseError> {
    let document = metadata_parser::parse_xml(input, metadata_parser::MAX_DESCRIPTOR_NODES)?;
    let relation = direct_property(document.root_element(), kind, "BasedOn");
    Ok(relation.is_some_and(|relation| {
        relation.children().any(|item| {
            item.has_tag_name((READABLE, "Item")) && item.text().map(str::trim) == Some(basis)
        })
    }))
}

pub(super) fn commands(
    owner_type: &PropertyKey,
    resources: &[FormResource],
) -> Vec<AutoCommandItem> {
    let Some(basis) = basis(owner_type) else {
        return Vec::new();
    };
    let mut seen = HashSet::<&ObjectId>::new();
    resources
        .iter()
        .flat_map(|resource| candidates(&resource.descriptor))
        .filter(|reference| seen.insert(&reference.id))
        .filter_map(|reference| {
            let (resource, object) = effective_metadata(&reference.id, resources)?;
            let property = metadata_property(object, "BasedOn", resources)?;
            let MetadataValue::Record(items) = &property.value else {
                return None;
            };
            if !items.iter().any(|item| {
                item.key.namespace.as_deref() == Some(READABLE)
                    && item.key.name == "Item"
                    && super::appearance::text(&item.value).map(str::trim) == Some(basis.as_str())
            }) {
                return None;
            }
            let target = format!(
                "{}.{}",
                reference.kind.designer_tag(),
                object.metadata.name()
            );
            let mut item = AutoCommandItem::standard(&target, "CreateBasedOn", false);
            item.caption = metadata_property(object, "ObjectPresentation", resources)
                .and_then(|property| literal(&property.value))
                .unwrap_or_else(|| metadata_caption(object, resources));
            item.origin = Some(PropertySource {
                source: resource.source.clone(),
                range: object.range.clone(),
            });
            Some(item)
        })
        .collect()
}
