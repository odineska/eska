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
}

pub(super) fn collect(root: Node<'_, '_>) -> Vec<FormReference> {
    let mut output = Vec::new();
    for node in root.descendants().filter(Node::is_element) {
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
                range: node.range(),
                property: PropertyKey {
                    namespace: node.tag_name().namespace().map(str::to_owned),
                    name: node.tag_name().name().into(),
                },
                slot,
                target: PropertyKey {
                    namespace: Some(namespace.into()),
                    name: name.into(),
                },
            });
        }
    }
    output
}

fn request(kind: MetadataKind, name: &str, output: &mut Vec<ObjectId>) {
    if name.is_empty() || name.contains(['/', '\\', '\0']) {
        return;
    }
    let id = ObjectId::from_parts(None, kind.as_str(), name);
    if !output.contains(&id) {
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
            request(MetadataKind::Style, value, &mut output);
        }
        if name == "CommandName" && kind == "CommonCommand" {
            request(MetadataKind::CommonCommand, value, &mut output);
        }
        if name == "Ref"
            && node.tag_name().namespace() == Some("http://v8.1c.ru/8.3/xcf/readable")
            && kind == "CommonPicture"
        {
            request(MetadataKind::CommonPicture, value, &mut output);
        }
        if include_types
            && matches!(name, "Type" | "MainTable" | "Value")
            && !kind.starts_with("External")
            && let Some(kind) = type_kind(kind)
        {
            request(kind, value, &mut output);
        }
    }
    for reference in references {
        if reference.target.namespace.as_deref() == Some(STYLE) {
            request(MetadataKind::StyleItem, &reference.target.name, &mut output);
        }
        if reference.target.namespace.as_deref() == Some(CONFIG)
            && let Some(name) = reference.target.name.strip_prefix("StyleItem.")
        {
            request(MetadataKind::StyleItem, name, &mut output);
        }
    }
    output
}

pub(super) fn type_kind(name: &str) -> Option<MetadataKind> {
    match name {
        "Catalog" | "CatalogObject" | "CatalogRef" => Some(MetadataKind::Catalog),
        "Document" | "DocumentObject" | "DocumentRef" => Some(MetadataKind::Document),
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
