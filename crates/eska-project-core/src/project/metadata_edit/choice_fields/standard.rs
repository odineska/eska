//! Platform standard fields and their owner-dependent availability in Designer XML.

use super::child;
use crate::project::{
    metadata_model::{MetadataProperty, MetadataValue},
    metadata_parser::ParsedObject,
};
use roxmltree::Node;

/// Metadata exposes latent settings too: a disabled data field can still be inspected in Designer.
pub fn parsed_fields(owner: &ParsedObject, compatibility: &str) -> Vec<&'static str> {
    catalog(
        owner.metadata.kind().designer_tag(),
        compatibility,
        &|name| scalar(owner, name),
    )
}

pub fn scalar<'a>(owner: &'a ParsedObject, name: &str) -> Option<&'a str> {
    property(owner, name).and_then(|field| match &field.value {
        MetadataValue::Text(value)
        | MetadataValue::QualifiedText { text: value, .. }
        | MetadataValue::TypedText { text: value, .. } => Some(value.as_str()),
        _ => None,
    })
}

/// Match the reviewed native editor limit without manufacturing fields from invalid counts.
pub fn extra_dimension_count(value: &str) -> Option<u32> {
    value
        .parse()
        .ok()
        .filter(|count| *count <= super::super::ext_dimensions::MAX_COUNT)
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
    catalog(kind, compatibility, value)
        .into_iter()
        .filter(|name| available(kind, name, value, has_owners))
        .collect()
}

/// One metadata catalog feeds both the read-only tree and applicable data-field choices.
fn catalog<'a>(
    kind: &str,
    compatibility: &str,
    value: &dyn Fn(&str) -> Option<&'a str>,
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
            fields.extend(["Code", "Description"]);
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
        "CalculationRegister" => vec![
            "RegistrationPeriod",
            "Active",
            "LineNumber",
            "Recorder",
            "CalculationType",
            "ActionPeriod",
            "BegOfActionPeriod",
            "EndOfActionPeriod",
            "BegOfBasePeriod",
            "EndOfBasePeriod",
            "ReversingEntry",
        ],
        "InformationRegister" => {
            vec!["Active", "LineNumber", "Recorder"]
        }
        _ => Vec::new(),
    };
    append_metadata(kind, value, compatibility, &mut result);
    result
}

/// Version-dependent additions differ from data availability: old platforms lack the metadata field.
fn append_metadata<'a>(
    kind: &str,
    value: &dyn Fn(&str) -> Option<&'a str>,
    compatibility: &str,
    result: &mut Vec<&'static str>,
) {
    match kind {
        "Catalog" => result.extend(["Parent", "IsFolder", "Owner"]),
        "ChartOfAccounts" => {
            result.extend(["Parent", "Type", "OffBalance", "Order"]);
        }
        "ChartOfCalculationTypes" => {
            result.push("ActionPeriodIsBasic");
        }
        "ChartOfCharacteristicTypes" => {
            result.extend(["ValueType", "Parent", "IsFolder"]);
        }
        "InformationRegister" => {
            result.push("Period");
        }
        "AccumulationRegister" if value("RegisterType") == Some("Balance") => {
            result.push("RecordType");
        }
        "AccountingRegister" => {
            result.push("Account");
            if !enabled(value, "Correspondence") {
                result.push("RecordType");
            }
            if positive(value, "PeriodAdjustmentLength") && newer_than(compatibility, 12) {
                result.push("PeriodAdjustment");
            }
        }
        "Document" | "BusinessProcess" | "Task" => {
            result.push("Number");
            if kind == "Task" {
                result.push("Description");
            }
        }
        _ => (),
    }
}

/// Disabled metadata settings must never become writable data paths or query columns.
fn available<'a>(
    kind: &str,
    name: &str,
    value: &dyn Fn(&str) -> Option<&'a str>,
    has_owners: bool,
) -> bool {
    match name {
        "Code" | "Description" if kind != "ExchangePlan" => {
            positive(value, &format!("{name}Length"))
        }
        "Number" if kind != "DocumentJournal" => positive(value, "NumberLength"),
        "Parent" if kind != "ChartOfAccounts" => enabled(value, "Hierarchical"),
        "IsFolder" => {
            enabled(value, "Hierarchical")
                && (kind != "Catalog" || value("HierarchyType") == Some("HierarchyFoldersAndItems"))
        }
        "Owner" => has_owners,
        "Order" if kind == "ChartOfAccounts" => positive(value, "OrderLength"),
        "ActionPeriodIsBasic" => enabled(value, "ActionPeriodUse"),
        "Active" | "LineNumber" | "Recorder" if kind == "InformationRegister" => {
            value("WriteMode") == Some("RecorderSubordinate")
        }
        "Period" if kind == "InformationRegister" => {
            value("InformationRegisterPeriodicity").is_some_and(|value| value != "Nonperiodical")
        }
        "RecordType" if kind == "AccumulationRegister" => value("RegisterType") == Some("Balance"),
        "RecordType" if kind == "AccountingRegister" => !enabled(value, "Correspondence"),
        "Account" => value("ChartOfAccounts").is_some_and(|value| !value.is_empty()),
        "CalculationType" => {
            value("ChartOfCalculationTypes").is_some_and(|value| !value.is_empty())
        }
        "ActionPeriod" | "BegOfActionPeriod" | "EndOfActionPeriod" => {
            enabled(value, "ActionPeriod")
        }
        "BegOfBasePeriod" | "EndOfBasePeriod" => enabled(value, "BasePeriod"),
        "Type" if kind == "DocumentJournal" => false,
        _ => true,
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
