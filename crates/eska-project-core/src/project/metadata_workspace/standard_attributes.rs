//! Read-only projections of implicit platform fields and their saved overrides.

use super::{ProjectSession, WorkspaceError};
use crate::project::{
    configurator::{ChildrenState, TreeError, TreeNode},
    forms::{FieldType, FieldValueKind, standard_type_for},
    metadata_edit::choice_fields::standard,
    metadata_model::{
        CollectionKind, MetadataKind, MetadataProperty, MetadataValue, NodeId, ObjectId,
        PropertyKey,
    },
    metadata_parser::{ParsedObject, PropertiesMode},
};

mod dependencies;

const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
const XS: &str = "http://www.w3.org/2001/XMLSchema";

#[derive(Default)]
struct AttributeContext {
    names: Vec<String>,
    extra_dimension_type: Option<MetadataProperty>,
    extra_dimension_value: Option<MetadataProperty>,
}

impl ProjectSession {
    /// Inspect an exposed standard field without creating an editable metadata object.
    ///
    /// # Errors
    /// Returns an unknown node for a fabricated identity or a field absent from the metadata catalog.
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
        let object = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == owner)
            .ok_or_else(|| WorkspaceError::UnknownNode(id.clone()))?;
        let context = self.standard_attribute_context(object)?;
        if !context.names.contains(name) {
            return Err(WorkspaceError::UnknownNode(id.clone()));
        }
        let mut fields = vec![field(MD, "Name", MetadataValue::Text(name.clone()))];
        let value_type = if name.starts_with("ExtDimensionType") {
            context.extra_dimension_type
        } else if name.starts_with("ExtDimension") {
            context.extra_dimension_value
        } else {
            type_property(&standard_type_for(name, object, &|key| {
                standard::property(object, key)
            }))
        };
        if let Some(value) = value_type {
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

    /// Resolve only the explicitly assigned chart when its standard-attribute group is expanded.
    pub(super) fn expand_standard_attributes(
        &mut self,
        owner: &ObjectId,
    ) -> Result<(), WorkspaceError> {
        let parsed = self.load(owner, PropertiesMode::All)?;
        self.verify_source(owner)?;
        let object = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == owner)
            .ok_or_else(|| WorkspaceError::UnknownObject(owner.clone()))?;
        let context = self.standard_attribute_context(object)?;
        let id = NodeId::Collection {
            owner: owner.clone(),
            kind: CollectionKind::StandardAttributes,
        };
        let previous = self.node(&id)?.children.clone();
        let nodes: Vec<_> = context
            .names
            .into_iter()
            .map(|name| TreeNode::standard_attribute(owner, object.metadata.kind(), name))
            .collect();
        let children = nodes.iter().map(|node| node.id.clone()).collect();
        for child in previous {
            self.nodes.remove(&child);
        }
        for node in nodes {
            self.nodes.insert(node.id.clone(), node);
        }
        if let Some(group) = self.nodes.get_mut(&id) {
            group.children = children;
            group.state = if group.children.is_empty() {
                ChildrenState::Empty
            } else {
                ChildrenState::NonEmpty
            };
        }
        Ok(())
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
                .chain(value.allowed_sign.map(|sign| {
                    (
                        "AllowedSign",
                        match sign {
                            crate::project::forms::NumberSign::Any => "Any",
                            crate::project::forms::NumberSign::Nonnegative => "Nonnegative",
                        }
                        .into(),
                    )
                }))
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
