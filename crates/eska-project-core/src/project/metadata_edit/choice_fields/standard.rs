//! Fixed source fields and their owner-dependent availability in the installed EDT model.

use super::child;
use roxmltree::Node;

/// Only metadata owners with a reviewed choice-field provider participate.
pub(super) fn supported(kind: &str) -> bool {
    matches!(
        kind,
        "Catalog"
            | "Document"
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
        "BusinessProcess" => vec!["Ref", "DeletionMark", "Date", "Started", "Completed"],
        "Task" => vec!["Ref", "DeletionMark", "Date", "Executed"],
        "ExchangePlan" => vec![
            "Ref",
            "DeletionMark",
            "Code",
            "Description",
            "SentNo",
            "ReceivedNo",
            "ThisNode",
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
