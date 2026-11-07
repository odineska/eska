//! Discover reverse `BasedOn` relations from declared application metadata.

use std::collections::HashSet;

use super::{
    AutoCommandItem, FormResource, PropertySource,
    commands::{effective_metadata, metadata_caption},
    presentation::{literal, metadata_property},
    resources::{configuration_objects, has_readable_reference},
};
use crate::project::metadata_model::{MetadataKind, ObjectId, PropertyKey};

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
        .flat_map(|resource| configuration_objects(&resource.descriptor))
        .filter(|reference| supports_creation(reference.kind) && seen.insert(&reference.id))
        .filter_map(|reference| {
            let (resource, object) = effective_metadata(&reference.id, resources)?;
            let property = metadata_property(object, "BasedOn", resources)?;
            if !has_readable_reference(&property.value, &basis) {
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
