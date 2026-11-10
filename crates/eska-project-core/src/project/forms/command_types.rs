//! Match command parameters against the edited object's reference type.

use std::collections::HashSet;

use super::{
    FormResource,
    commands::effective_metadata,
    presentation::metadata_property,
    references::{self, CONFIG},
};
use crate::project::{
    metadata_model::{MetadataKind, MetadataValue, ObjectId, PropertyKey},
    metadata_parser::{self, ParsedObject},
};

pub(super) const PARAMETER: &str = "CommandParameterType";

pub(super) fn reference_type(owner_type: &PropertyKey) -> String {
    owner_type.name.replacen("Object.", "Ref.", 1)
}

pub(super) fn may_declare(input: &str) -> bool {
    input.contains("</CommandParameterType") || input.contains(":CommandParameterType")
}

pub(super) fn dependencies(object: &ParsedObject) -> impl Iterator<Item = ObjectId> {
    let name = match object.metadata.kind() {
        MetadataKind::CommonCommand | MetadataKind::Command => PARAMETER,
        MetadataKind::DefinedType => "Type",
        _ => "",
    };
    object
        .properties
        .iter()
        .flatten()
        .find(|value| {
            value.property.key.name == name
                && value.property.key.namespace.as_deref() == Some(metadata_parser::MD_NAMESPACE)
        })
        .into_iter()
        .flat_map(|value| type_sets(&value.property.value))
}

pub(super) fn accepts(command: &ParsedObject, parameter: &str, resources: &[FormResource]) -> bool {
    let Some(property) = metadata_property(command, PARAMETER, resources)
        .filter(|property| has_parameter(&property.value))
    else {
        return command.metadata.kind() == MetadataKind::Command;
    };
    matches_parameter(&property.value, parameter, |id| {
        let (_, definition) = effective_metadata(id, resources)?;
        metadata_property(definition, "Type", resources).map(|property| &property.value)
    })
}

pub(super) fn has_parameter(value: &MetadataValue) -> bool {
    members(value).next().is_some()
}

pub(super) fn matches_parameter<'a>(
    value: &'a MetadataValue,
    parameter: &str,
    definition: impl Fn(&ObjectId) -> Option<&'a MetadataValue>,
) -> bool {
    let mut values = vec![value];
    let mut seen = HashSet::new();
    while let Some(value) = values.pop() {
        for (slot, key) in members(value) {
            if accepts_type(slot, key, parameter) {
                return true;
            }
            if let Some(id) = defined_type(slot, key)
                && !seen.contains(&id)
                && let Some(value) = definition(&id)
            {
                seen.insert(id);
                values.push(value);
            }
        }
    }
    false
}

pub(super) fn type_sets(value: &MetadataValue) -> impl Iterator<Item = ObjectId> {
    members(value).filter_map(|(slot, key)| defined_type(slot, key))
}

fn members(value: &MetadataValue) -> impl Iterator<Item = (&str, &PropertyKey)> {
    let fields: &[_] = match value {
        MetadataValue::Record(fields) => fields,
        _ => &[],
    };
    fields.iter().filter_map(|field| {
        let MetadataValue::QualifiedText { key, .. } = &field.value else {
            return None;
        };
        Some((field.key.name.as_str(), key))
    })
}

fn accepts_type(slot: &str, key: &PropertyKey, parameter: &str) -> bool {
    slot == "Type"
        && key.namespace.as_deref() == Some(CONFIG)
        && (key.name == parameter
            || parameter.split_once('.').is_some_and(|(family, _)| {
                references::type_kind(family).is_some()
                    && (key.name == family || key.name == "AnyRef")
            }))
}

fn defined_type(slot: &str, key: &PropertyKey) -> Option<ObjectId> {
    if slot != "TypeSet" || key.namespace.as_deref() != Some(CONFIG) {
        return None;
    }
    let name = key.name.strip_prefix("DefinedType.")?;
    (!name.is_empty() && !name.contains(['/', '\\', '\0']))
        .then(|| ObjectId::from_parts(None, MetadataKind::DefinedType.as_str(), name))
}
