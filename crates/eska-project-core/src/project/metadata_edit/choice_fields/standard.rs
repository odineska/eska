//! Platform standard fields and their owner-dependent availability in Designer XML.

use super::child;
use crate::project::{
    metadata_model::{MetadataProperty, MetadataValue},
    metadata_parser::ParsedObject,
};
use roxmltree::Node;

/// Parsed descriptors use the same availability rules as field-link choices and form data.
pub fn parsed_fields(owner: &ParsedObject, compatibility: &str) -> Vec<&'static str> {
    let has_owners = property(owner, "Owners").is_some_and(
        |field| matches!(&field.value, MetadataValue::Record(items) if !items.is_empty()),
    );
    fields_for(
        owner.metadata.kind().designer_tag(),
        compatibility,
        &|name| {
            property(owner, name).and_then(|field| match &field.value {
                MetadataValue::Text(value)
                | MetadataValue::QualifiedText { text: value, .. }
                | MetadataValue::TypedText { text: value, .. } => Some(value.as_str()),
                _ => None,
            })
        },
        has_owners,
    )
}

/// Ignore foreign fields with a colliding local name.
pub fn property<'a>(owner: &'a ParsedObject, name: &str) -> Option<&'a MetadataProperty> {
    owner
        .properties
        .as_ref()?
        .iter()
        .find(|field| {
            field.property.key.namespace.as_deref() == Some(super::MD)
                && field.property.key.name == name
        })
        .map(|field| &field.property)
}

/// Only metadata owners with a reviewed choice-field provider participate.
pub(super) fn supported(kind: &str) -> bool {
    matches!(
        kind,
        "Catalog"
            | "Document"
            | "DocumentJournal"
            | "Enum"
            | "Report"
            | "DataProcessor"
            | "ExternalReport"
            | "ExternalDataProcessor"
            | "TabularSection"
            | "ChartOfAccounts"
            | "ChartOfCalculationTypes"
            | "ChartOfCharacteristicTypes"
            | "InformationRegister"
            | "AccumulationRegister"
            | "AccountingRegister"
            | "CalculationRegister"
            | "ExchangePlan"
            | "BusinessProcess"
            | "Task"
    )
}

/// Source fields may exist without a `StandardAttributes` XML block; disabled fields remain excluded.
pub fn fields(owner: Node<'_, '_>, compatibility: &str) -> Vec<&'static str> {
    let Some(properties) = child(owner, "Properties") else {
        return Vec::new();
    };
    let owners = child(properties, "Owners")
        .is_some_and(|owners| owners.children().any(|node| node.is_element()));
    fields_for(
        owner.tag_name().name(),
        compatibility,
        &|name| text(properties, name),
        owners,
    )
}

/// Shared availability rules for parsed XML and composed form metadata.
pub fn fields_for<'a>(
    kind: &str,
    compatibility: &str,
    value: &dyn Fn(&str) -> Option<&'a str>,
    has_owners: bool,
) -> Vec<&'static str> {
    let mut result = match kind {
        "Catalog"
        | "ChartOfAccounts"
        | "ChartOfCalculationTypes"
        | "ChartOfCharacteristicTypes" => {
            let mut fields = vec!["Ref", "DeletionMark", "Predefined"];
            if newer_than(compatibility, 2) {
                fields.push("PredefinedDataName");
            }
            for (property, name) in [("CodeLength", "Code"), ("DescriptionLength", "Description")] {
                if positive(value, property) {
                    fields.push(name);
                }
            }
            fields
        }
        "Document" => vec!["Ref", "DeletionMark", "Date", "Posted"],
        "DocumentJournal" => vec!["Type", "Ref", "Date", "Posted", "DeletionMark", "Number"],
        "Enum" => vec!["Ref", "Order"],
        "BusinessProcess" => vec![
            "Ref",
            "DeletionMark",
            "Date",
            "Started",
            "Completed",
            "HeadTask",
        ],
        "Task" => vec![
            "Ref",
            "DeletionMark",
            "Date",
            "Executed",
            "BusinessProcess",
            "RoutePoint",
        ],
        "ExchangePlan" => vec![
            "Ref",
            "DeletionMark",
            "Code",
            "Description",
            "SentNo",
            "ReceivedNo",
            "ThisNode",
            "ExchangeDate",
        ],
        "TabularSection" => vec!["LineNumber"],
        "AccumulationRegister" | "AccountingRegister" => {
            vec!["Active", "LineNumber", "Recorder", "Period"]
        }
        "CalculationRegister" => vec!["RegistrationPeriod", "Active", "LineNumber", "Recorder"],
        "InformationRegister" if value("WriteMode") == Some("RecorderSubordinate") => {
            vec!["Active", "LineNumber", "Recorder"]
        }
        _ => Vec::new(),
    };
    append_conditional(kind, value, has_owners, compatibility, &mut result);
    result
}

/// Keep availability predicates separate from the owner's unconditional fixed fields.
fn append_conditional<'a>(
    kind: &str,
    value: &dyn Fn(&str) -> Option<&'a str>,
    has_owners: bool,
    compatibility: &str,
    result: &mut Vec<&'static str>,
) {
    match kind {
        "Catalog" => {
            if enabled(value, "Hierarchical") {
                result.push("Parent");
                if value("HierarchyType") == Some("HierarchyFoldersAndItems") {
                    result.push("IsFolder");
                }
            }
            if has_owners {
                result.push("Owner");
            }
        }
        "ChartOfAccounts" => {
            result.extend(["Parent", "Type", "OffBalance"]);
            if positive(value, "OrderLength") {
                result.push("Order");
            }
        }
        "ChartOfCalculationTypes" if enabled(value, "ActionPeriodUse") => {
            result.push("ActionPeriodIsBasic");
        }
        "ChartOfCharacteristicTypes" => {
            result.push("ValueType");
            if enabled(value, "Hierarchical") {
                result.extend(["Parent", "IsFolder"]);
            }
        }
        "InformationRegister"
            if value("InformationRegisterPeriodicity")
                .is_some_and(|value| value != "Nonperiodical") =>
        {
            result.push("Period");
        }
        "AccumulationRegister" if value("RegisterType") == Some("Balance") => {
            result.push("RecordType");
        }
        "AccountingRegister" => {
            if value("ChartOfAccounts").is_some_and(|value| !value.is_empty()) {
                result.push("Account");
            }
            if !enabled(value, "Correspondence") {
                result.push("RecordType");
            }
            if positive(value, "PeriodAdjustmentLength") && newer_than(compatibility, 12) {
                result.push("PeriodAdjustment");
            }
        }
        "Document" | "BusinessProcess" | "Task" => {
            if positive(value, "NumberLength") {
                result.push("Number");
            }
            if kind == "Task" && positive(value, "DescriptionLength") {
                result.push("Description");
            }
        }
        _ => (),
    }
}

/// Unknown compatibility modes do not enable version-dependent fields.
fn newer_than(value: &str, patch: u32) -> bool {
    value == "DontUse"
        || value
            .strip_prefix("Version8_3_")
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|value| value > patch)
        || value.starts_with("Version8_5_")
}

/// Absent or zero-length properties do not enable optional standard fields.
fn positive<'a>(value: &dyn Fn(&str) -> Option<&'a str>, name: &str) -> bool {
    value(name)
        .and_then(|value| value.parse::<u32>().ok())
        .is_some_and(|value| value > 0)
}

/// XML Schema permits both word and digit spellings of booleans.
fn enabled<'a>(value: &dyn Fn(&str) -> Option<&'a str>, name: &str) -> bool {
    matches!(value(name), Some("true" | "1"))
}

/// Values must belong to direct metadata properties in the expected namespace.
fn text<'a>(properties: Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(properties, name)?.text()
}

#[cfg(test)]
mod tests;
