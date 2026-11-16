//! Designer metadata selectors retain their declared target class and collection cardinality.

use crate::project::metadata_model::MetadataKind;

/// Native command groups coexist with named metadata groups in the same Designer property.
pub const COMMAND_GROUPS: &[&str] = &[
    "NavigationPanelOrdinary",
    "NavigationPanelImportant",
    "NavigationPanelSeeAlso",
    "ActionsPanelCreate",
    "ActionsPanelTools",
    "ActionsPanelReports",
    "FormCommandBarImportant",
    "FormCommandBarCreateBasedOn",
    "FormNavigationPanelGoTo",
    "FormNavigationPanelImportant",
    "FormNavigationPanelSeeAlso",
];

pub fn command_group(value: &str) -> bool {
    COMMAND_GROUPS.contains(&value)
        || parts(value)
            .is_some_and(|parts| matches!(parts.as_slice(), [(MetadataKind::CommandGroup, _)]))
}

pub fn accepts(domain: &str, parts: &[(MetadataKind, &str)]) -> bool {
    use MetadataKind as K;
    match (domain, parts) {
        (
            "BasicRegister",
            [
                (
                    K::InformationRegister
                    | K::AccumulationRegister
                    | K::AccountingRegister
                    | K::CalculationRegister,
                    _,
                ),
            ],
        )
        | ("Document", [(K::Document, _)])
        | (
            "FunctionalOptionParameterUse",
            [(K::Catalog, _)] | [(K::InformationRegister, _), (K::Dimension, _)],
        )
        | (
            "FunctionalOptionLocation",
            [(K::Constant, _)]
            | [(K::Catalog, _), (K::Attribute, _)]
            | [(K::InformationRegister, _), (K::Attribute | K::Resource, _)],
        ) => true,
        ("MdObject", [first, ..]) => first.0 != K::Configuration,
        _ => target(domain)
            .is_some_and(|(kind, _)| parts.last().is_some_and(|(target, _)| *target == kind)),
    }
}

/// Only reviewed direct references and owner-specific form classes are editable selectors.
pub fn target(domain: &str) -> Option<(MetadataKind, Option<MetadataKind>)> {
    use MetadataKind as K;
    let direct = match domain {
        "BasicForm" => K::Form,
        "Role" => K::Role,
        "CommonForm" => K::CommonForm,
        "Language" => K::Language,
        "Style" => K::Style,
        "SettingsStorage" => K::SettingsStorage,
        "DocumentNumerator" => K::DocumentNumerator,
        "Catalog" => K::Catalog,
        "ChartOfAccounts" => K::ChartOfAccounts,
        "ChartOfCalculationTypes" => K::ChartOfCalculationTypes,
        "ChartOfCharacteristicTypes" => K::ChartOfCharacteristicTypes,
        "Task" => K::Task,
        "InformationRegister" => K::InformationRegister,
        _ => {
            let parent = domain.strip_suffix("Form")?;
            let parent = K::from_xml_tag(parent).ok()?;
            return Some((K::Form, Some(parent)));
        }
    };
    Some((direct, None))
}

/// Designer paths alternate exact type and object name, never filesystem coordinates.
#[must_use]
pub fn parts(raw: &str) -> Option<Vec<(MetadataKind, &str)>> {
    let tokens: Vec<_> = raw.split('.').collect();
    if tokens.len() > 64 || !tokens.len().is_multiple_of(2) {
        return None;
    }
    tokens
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            if pair[1].is_empty() || pair[1].chars().any(char::is_whitespace) {
                return None;
            }
            Some((MetadataKind::from_xml_tag(pair[0]).ok()?, pair[1]))
        })
        .collect()
}
