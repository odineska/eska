//! Resolve `QName` references while their XML namespace scope is still available.

use crate::project::metadata_model::{MetadataKind, ObjectId, PropertyKey};
use roxmltree::Node;
use serde::Serialize;
use std::ops::Range;

pub(super) const STYLE: &str = "http://v8.1c.ru/8.1/data/ui/style";
pub(super) const SYSTEM_FONT: &str = "http://v8.1c.ru/8.1/data/ui/fonts/system";
pub(super) const CONFIG: &str = "http://v8.1c.ru/8.1/data/enterprise/current-config";
pub(super) const WEB_COLOR: &str = "http://v8.1c.ru/8.1/data/ui/colors/web";
pub(super) const WINDOWS_COLOR: &str = "http://v8.1c.ru/8.1/data/ui/colors/windows";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceSlot {
    Text,
    Ref,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormReference {
    pub range: Range<usize>,
    pub property: PropertyKey,
    pub slot: ReferenceSlot,
    pub target: PropertyKey,
    #[serde(skip)]
    element_path: Vec<(usize, usize)>,
}

impl FormReference {
    /// Indices of record fields between the enclosing property and this reference.
    #[must_use]
    pub fn indices_from(&self, property_start: usize) -> Option<impl Iterator<Item = usize> + '_> {
        let property = self
            .element_path
            .iter()
            .position(|(start, _)| *start == property_start)?;
        Some(
            self.element_path[property + 1..]
                .iter()
                .map(|(_, index)| *index),
        )
    }
}

pub(super) fn collect(root: Node<'_, '_>) -> Vec<FormReference> {
    let mut output = Vec::new();
    let mut ancestors: Vec<(Range<usize>, usize, usize)> = Vec::new();
    for node in root.descendants().filter(Node::is_element) {
        let range = node.range();
        // Count element children in the same order as MetadataValue::Record.
        while ancestors
            .last()
            .is_some_and(|(parent, _, _)| parent.end <= range.start)
        {
            ancestors.pop();
        }
        let index = ancestors.last_mut().map_or(0, |(_, _, children)| {
            let index = *children;
            *children += 1;
            index
        });
        ancestors.push((range.clone(), index, 0));
        for (slot, text) in [
            (ReferenceSlot::Ref, node.attribute("ref")),
            (
                ReferenceSlot::Text,
                node.text()
                    .filter(|_| !node.children().any(|child| child.is_element())),
            ),
        ] {
            let Some((prefix, name)) = text.and_then(|text| text.trim().split_once(':')) else {
                continue;
            };
            let Some(namespace) = node.lookup_namespace_uri(Some(prefix)) else {
                continue;
            };
            if name.is_empty()
                || name.contains(':')
                || name.contains(char::is_whitespace)
                || !matches!(
                    namespace,
                    STYLE | SYSTEM_FONT | CONFIG | WEB_COLOR | WINDOWS_COLOR
                )
            {
                continue;
            }
            output.push(FormReference {
                range: range.clone(),
                property: PropertyKey {
                    namespace: node.tag_name().namespace().map(str::to_owned),
                    name: node.tag_name().name().into(),
                },
                slot,
                target: PropertyKey {
                    namespace: Some(namespace.into()),
                    name: name.into(),
                },
                element_path: ancestors
                    .iter()
                    .map(|(range, index, _)| (range.start, *index))
                    .collect(),
            });
        }
    }
    output
}

fn object_id(parent: Option<&ObjectId>, kind: MetadataKind, name: &str) -> Option<ObjectId> {
    (!name.is_empty() && !name.contains(['/', '\\', '\0']))
        .then(|| ObjectId::from_parts(parent, kind.as_str(), name))
}

pub(super) fn metadata_type_id(key: &PropertyKey) -> Option<ObjectId> {
    if key.namespace.as_deref() != Some(CONFIG) {
        return None;
    }
    let (kind, name) = key.name.split_once('.')?;
    object_id(None, type_kind(kind)?, name)
}

pub(super) fn metadata_command_id(reference: &str) -> Option<ObjectId> {
    let mut parts = reference.split('.');
    let kind = MetadataKind::from_xml_tag(parts.next()?).ok()?;
    let owner = object_id(None, kind, parts.next()?)?;
    let member = parts.next();
    if kind == MetadataKind::CommonCommand {
        return member.is_none().then_some(owner);
    }
    if member != Some("Command") || kind.collection_folder().is_none() {
        return None;
    }
    let name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    object_id(Some(&owner), MetadataKind::Command, name)
}

pub(super) fn command_group_id(reference: &str) -> Option<ObjectId> {
    let name = reference.strip_prefix("CommandGroup.")?;
    object_id(None, MetadataKind::CommandGroup, name)
}

fn request(id: Option<ObjectId>, output: &mut Vec<ObjectId>) {
    if let Some(id) = id
        && !output.contains(&id)
    {
        output.push(id);
    }
}

pub(super) fn discover_resources(
    root: Node<'_, '_>,
    references: &[FormReference],
    include_types: bool,
) -> Vec<ObjectId> {
    let mut output = Vec::new();
    for node in root.descendants().filter(Node::is_element) {
        let name = node.tag_name().name();
        let text = node.text().unwrap_or_default().trim();
        let text = text
            .split_once(':')
            .filter(|(prefix, _)| node.lookup_namespace_uri(Some(prefix)) == Some(CONFIG))
            .map_or(text, |(_, value)| value);
        let mut parts = text.split('.');
        let kind = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        if matches!(name, "DefaultStyle" | "Style") && kind == "Style" {
            request(object_id(None, MetadataKind::Style, value), &mut output);
        }
        if node.tag_name().namespace() == Some(super::FORM_NAMESPACE)
            && matches!(name, "CommandName" | "Command")
        {
            request(metadata_command_id(text), &mut output);
        }
        if node.has_tag_name((super::FORM_NAMESPACE, "CommandGroup"))
            || node.has_tag_name((super::super::metadata_parser::MD_NAMESPACE, "Group"))
        {
            request(command_group_id(text), &mut output);
        }
        if name == "Ref"
            && node.tag_name().namespace() == Some("http://v8.1c.ru/8.3/xcf/readable")
            && kind == "CommonPicture"
        {
            request(
                object_id(None, MetadataKind::CommonPicture, value),
                &mut output,
            );
        }
        if include_types
            && matches!(name, "Type" | "MainTable" | "Value")
            && !kind.starts_with("External")
            && let Some(kind) = type_kind(kind)
        {
            request(object_id(None, kind, value), &mut output);
        }
    }
    for reference in references {
        if reference.target.namespace.as_deref() == Some(STYLE) {
            request(
                object_id(None, MetadataKind::StyleItem, &reference.target.name),
                &mut output,
            );
        }
        if reference.target.namespace.as_deref() == Some(CONFIG)
            && let Some(name) = reference.target.name.strip_prefix("StyleItem.")
        {
            request(object_id(None, MetadataKind::StyleItem, name), &mut output);
        }
    }
    output
}

pub(super) fn type_kind(name: &str) -> Option<MetadataKind> {
    match name {
        "Catalog" | "CatalogObject" | "CatalogRef" => Some(MetadataKind::Catalog),
        "Document" | "DocumentObject" | "DocumentRef" => Some(MetadataKind::Document),
        "DocumentJournal" => Some(MetadataKind::DocumentJournal),
        "Enum" | "EnumRef" => Some(MetadataKind::Enum),
        "DataProcessor" | "DataProcessorObject" | "ExternalDataProcessorObject" => {
            Some(MetadataKind::DataProcessor)
        }
        "Report" | "ReportObject" | "ExternalReportObject" => Some(MetadataKind::Report),
        "InformationRegister" | "InformationRegisterRecordSet" => {
            Some(MetadataKind::InformationRegister)
        }
        "AccumulationRegister" | "AccumulationRegisterRecordSet" => {
            Some(MetadataKind::AccumulationRegister)
        }
        "ChartOfCharacteristicTypes"
        | "ChartOfCharacteristicTypesRef"
        | "ChartOfCharacteristicTypesObject" => Some(MetadataKind::ChartOfCharacteristicTypes),
        "ChartOfAccounts" | "ChartOfAccountsRef" | "ChartOfAccountsObject" => {
            Some(MetadataKind::ChartOfAccounts)
        }
        "ChartOfCalculationTypes"
        | "ChartOfCalculationTypesRef"
        | "ChartOfCalculationTypesObject" => Some(MetadataKind::ChartOfCalculationTypes),
        "BusinessProcess" | "BusinessProcessObject" | "BusinessProcessRef" => {
            Some(MetadataKind::BusinessProcess)
        }
        "Task" | "TaskObject" | "TaskRef" => Some(MetadataKind::Task),
        "ExchangePlan" | "ExchangePlanObject" | "ExchangePlanRef" => {
            Some(MetadataKind::ExchangePlan)
        }
        _ => None,
    }
}
