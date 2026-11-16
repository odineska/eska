//! Discovery defaults and request validation share the same closed semantic vocabulary.

use super::{
    CreateType, CreationCapabilities, CreationKind, CreationLimits, CreationTypeChoice,
    DateFractions,
};
use crate::project::metadata_model::{LocalizedText, MetadataKind, PropertyKey};

pub const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
pub const CORE: &str = "http://v8.1c.ru/8.1/data/core";
pub const XS: &str = "http://www.w3.org/2001/XMLSchema";
pub(super) const XR: &str = "http://v8.1c.ru/8.3/xcf/readable";

/// Only kinds whose minimal Designer declarations are accepted by the native platform are exposed.
#[must_use]
pub const fn supported_owner_kinds() -> &'static [MetadataKind] {
    &[
        MetadataKind::Catalog,
        MetadataKind::Document,
        MetadataKind::DataProcessor,
        MetadataKind::Report,
        MetadataKind::TabularSection,
    ]
}

/// A lightweight node hint shares the same allowlist as full source/policy discovery.
#[must_use]
pub const fn kinds_for_owner(kind: MetadataKind) -> &'static [CreationKind] {
    match kind {
        MetadataKind::Catalog
        | MetadataKind::Document
        | MetadataKind::DataProcessor
        | MetadataKind::Report => &[CreationKind::Attribute, CreationKind::TablePart],
        MetadataKind::TabularSection => &[CreationKind::Attribute],
        _ => &[],
    }
}

/// Keep machine discovery, interactive defaults and domain limits in one definition.
#[must_use]
pub fn capabilities() -> CreationCapabilities {
    let primitive = |name: &str, default| CreationTypeChoice {
        key: PropertyKey {
            namespace: Some(XS.to_owned()),
            name: name.to_owned(),
        },
        default,
    };
    CreationCapabilities {
        owner_kinds: supported_owner_kinds().to_vec(),
        kinds: vec![CreationKind::Attribute, CreationKind::TablePart],
        primitive_types: vec![
            primitive(
                "string",
                CreateType::String {
                    length: 0,
                    fixed: false,
                },
            ),
            primitive(
                "decimal",
                CreateType::Number {
                    digits: 10,
                    fraction_digits: 0,
                    non_negative: false,
                },
            ),
            primitive("boolean", CreateType::Boolean {}),
            primitive(
                "dateTime",
                CreateType::Date {
                    fractions: DateFractions::DateTime,
                },
            ),
        ],
        limits: CreationLimits {
            name_utf16_max: 80,
            string_length_min: 0,
            string_length_max: 1024,
            string_fixed_length_min: 1,
            string_fixed_length_max: 100,
            number_digits_min: 0,
            number_digits_max: 38,
        },
    }
}

/// Preserve the shared localized-text model while keeping new operation input strict.
pub(super) fn deserialize_synonyms<'de, D>(deserializer: D) -> Result<Vec<LocalizedText>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Synonym {
        language: String,
        content: String,
    }
    use serde::Deserialize;
    Ok(Vec::<Synonym>::deserialize(deserializer)?
        .into_iter()
        .map(|synonym| LocalizedText {
            language: synonym.language,
            content: synonym.content,
        })
        .collect())
}
