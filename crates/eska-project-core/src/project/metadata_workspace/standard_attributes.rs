//! Read-only projections of implicit platform fields and their saved overrides.

use super::{ProjectSession, WorkspaceError};
use crate::project::{
    forms::{FieldType, FieldValueKind, standard_type_for},
    metadata_edit::choice_fields::standard,
    metadata_model::{MetadataProperty, MetadataValue, NodeId, PropertyKey},
    metadata_parser::PropertiesMode,
};

const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
const XS: &str = "http://www.w3.org/2001/XMLSchema";

impl ProjectSession {
    /// Inspect an exposed standard field without creating an editable metadata object.
    ///
    /// # Errors
    /// Returns an unknown node for a fabricated identity or a field disabled by owner settings.
    pub fn standard_attribute_properties(
        &mut self,
        id: &NodeId,
    ) -> Result<Vec<MetadataProperty>, WorkspaceError> {
        self.node(id)?;
        let NodeId::StandardAttribute { owner, name } = id else {
            return Err(WorkspaceError::UnknownNode(id.clone()));
        };
        let parsed = self.load(owner, PropertiesMode::All)?;
        self.verify_source(owner)?;
        let compatibility = &self.standard_attribute_compatibility;
        let object = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == owner)
            .filter(|object| {
                standard::parsed_fields(object, compatibility).contains(&name.as_str())
            })
            .ok_or_else(|| WorkspaceError::UnknownNode(id.clone()))?;
        let mut fields = vec![field(MD, "Name", MetadataValue::Text(name.clone()))];
        let value_type = standard_type_for(name, object, &|key| standard::property(object, key));
        if let Some(value) = type_property(&value_type) {
            fields.push(value);
        }
        // The wrapper's name is a selector, not a second editable property.
        if let Some(MetadataProperty {
            value: MetadataValue::Record(attributes),
            ..
        }) = standard::property(object, "StandardAttributes")
            && let Some(MetadataProperty {
                value: MetadataValue::Record(overrides),
                ..
            }) = attributes.iter().find(|attribute| {
                attribute.key.namespace.as_deref() == Some(READABLE)
                    && attribute.key.name == "StandardAttribute"
                    && attribute.qualifiers.iter().any(|(key, value)| {
                        key.namespace.is_none() && key.name == "name" && value == name
                    })
            })
        {
            fields.extend(overrides.iter().cloned());
        }
        Ok(fields)
    }
}

fn field(namespace: &str, name: &str, value: MetadataValue) -> MetadataProperty {
    MetadataProperty {
        key: PropertyKey {
            namespace: Some(namespace.into()),
            name: name.into(),
        },
        qualifiers: Vec::new(),
        value,
    }
}

/// Reuse the metadata Type representation so all clients share existing type localization.
fn type_property(value: &FieldType) -> Option<MetadataProperty> {
    let primitive = match value.kind {
        FieldValueKind::String => Some("string"),
        FieldValueKind::Number => Some("decimal"),
        FieldValueKind::Boolean => Some("boolean"),
        FieldValueKind::Date => Some("dateTime"),
        _ => None,
    };
    let mut keys = value.types.clone();
    if keys.is_empty()
        && let Some(name) = primitive
    {
        keys.push(PropertyKey {
            namespace: Some(XS.into()),
            name: name.into(),
        });
    }
    if keys.is_empty() {
        return None;
    }
    let mut types: Vec<_> = keys
        .into_iter()
        .map(|key| {
            field(
                CORE,
                "Type",
                MetadataValue::QualifiedText {
                    text: key.name.clone(),
                    key,
                },
            )
        })
        .collect();
    let (qualifier, constraints): (&str, Vec<(&str, String)>) = match value.kind {
        FieldValueKind::String => (
            "StringQualifiers",
            value
                .length
                .into_iter()
                .map(|length| ("Length", length.to_string()))
                .collect(),
        ),
        FieldValueKind::Number => (
            "NumberQualifiers",
            value
                .digits
                .into_iter()
                .map(|digits| ("Digits", digits.to_string()))
                .chain(
                    value
                        .fraction
                        .into_iter()
                        .map(|fraction| ("FractionDigits", fraction.to_string())),
                )
                .collect(),
        ),
        FieldValueKind::Date => (
            "DateQualifiers",
            value
                .date_parts
                .iter()
                .map(|parts| ("DateFractions", parts.clone()))
                .collect(),
        ),
        _ => ("", Vec::new()),
    };
    if !constraints.is_empty() {
        types.push(field(
            CORE,
            qualifier,
            MetadataValue::Record(
                constraints
                    .into_iter()
                    .map(|(name, value)| field(CORE, name, MetadataValue::Text(value)))
                    .collect(),
            ),
        ));
    }
    Some(field(MD, "Type", MetadataValue::Record(types)))
}
