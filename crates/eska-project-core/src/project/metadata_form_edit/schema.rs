//! Discovery and validation use the inspector's property and event catalog.

use std::path::PathBuf;

use roxmltree::Node;
use serde::Serialize;

use super::{FormEditError, FormTarget, document, invalid};
use crate::project::{
    forms::{
        AuthoringProperty, AuthoringSource, ElementKind, FORM_NAMESPACE, PaletteEditor,
        PlatformPropertyValue, authoring_properties,
    },
    metadata_edit::snapshot,
    metadata_model::{MetadataProperty, MetadataValue, ObjectId, PropertyKey},
    metadata_parser,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormEditProperty {
    pub property: MetadataProperty,
    pub writable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormEditNode {
    pub target: FormTarget,
    pub parent: Option<FormTarget>,
    pub kind: String,
    pub name: Option<String>,
    pub properties: Vec<FormEditProperty>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormElementKindSchema {
    pub kind: ElementKind,
    pub catalog: Vec<AuthoringProperty>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDefinitionSchema {
    pub kind: &'static str,
    pub catalog: Vec<AuthoringProperty>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormEditSchema {
    pub form: ObjectId,
    pub path: PathBuf,
    pub snapshot: String,
    pub writable: bool,
    pub nodes: Vec<FormEditNode>,
    pub element_kinds: Vec<FormElementKindSchema>,
    pub definition_kinds: Vec<FormDefinitionSchema>,
}

/// Inspect exactly one managed-form payload, including existing properties unknown to the palette.
///
/// # Errors
/// Rejects ambiguous identities or borrowed forms that cannot be edited as a standalone tree.
pub fn inspect(
    form: &ObjectId,
    path: PathBuf,
    input: &str,
    writable: bool,
) -> Result<FormEditSchema, FormEditError> {
    document::validate(input)?;
    let document = document::xml(input)?;
    let nodes = document
        .root_element()
        .descendants()
        .filter_map(|node| {
            let target = document::target_of(node)?;
            let properties = node
                .children()
                .filter(Node::is_element)
                .filter(|child| {
                    (child.tag_name().namespace() != Some(FORM_NAMESPACE)
                        || !structural(child.tag_name().name())
                        || child.tag_name().name() == "Events")
                        && !document::is_element(*child)
                })
                .map(|node| {
                    let property = metadata_parser::values::property(node, &mut Vec::new());
                    let writable = writable
                        && node.tag_name().name() != "Events"
                        && supported_value(&property.value);
                    FormEditProperty { property, writable }
                })
                .collect();
            Some(FormEditNode {
                target,
                parent: node.ancestors().skip(1).find_map(document::target_of),
                kind: node.tag_name().name().to_owned(),
                name: node.attribute("name").map(str::to_owned),
                properties,
            })
        })
        .collect();
    Ok(FormEditSchema {
        form: form.clone(),
        path,
        snapshot: snapshot(input),
        writable,
        nodes,
        element_kinds: ElementKind::ALL
            .iter()
            .copied()
            .map(|kind| FormElementKindSchema {
                kind,
                catalog: authoring_properties(kind),
            })
            .collect(),
        definition_kinds: ["Attribute", "Column", "Command", "Parameter"]
            .into_iter()
            .map(|kind| FormDefinitionSchema {
                kind,
                catalog: definition_catalog(kind),
            })
            .collect(),
    })
}

pub(super) fn catalog(node: Node<'_, '_>) -> Vec<AuthoringProperty> {
    document::element(node).map_or_else(
        || definition_catalog(node.tag_name().name()),
        authoring_properties,
    )
}

pub(super) fn definition_catalog(kind: &str) -> Vec<AuthoringProperty> {
    use PaletteEditor::{Boolean, Dialog, Event, Localized, Number, Picture, Shortcut, Text};
    let settings: &[(&str, PaletteEditor)] = match kind {
        "Attribute" | "Column" => &[
            ("Type", Dialog),
            ("Title", Localized),
            ("MainAttribute", Boolean),
            ("SavedData", Boolean),
            ("Save", Dialog),
            ("UseAlways", Dialog),
            ("FillCheck", Boolean),
            ("Settings", Dialog),
            ("FunctionalOptions", Dialog),
        ],
        "Command" => &[
            ("Title", Localized),
            ("Action", Event),
            ("ToolTip", Localized),
            ("CurrentRowUse", Text),
            ("Picture", Picture),
            ("Representation", Text),
            ("ModifiesSavedData", Boolean),
            ("Shortcut", Shortcut),
            ("AssociatedTableElementId", Number),
            ("FunctionalOptions", Dialog),
        ],
        "Parameter" => &[("Type", Dialog), ("KeyParameter", Boolean)],
        _ => &[],
    };
    settings
        .iter()
        .map(|(name, editor)| AuthoringProperty {
            id: name,
            name,
            source: AuthoringSource::Property,
            editor: *editor,
            default: PlatformPropertyValue::Unknown,
            values: Vec::new(),
        })
        .collect()
}

pub(super) fn structural(name: &str) -> bool {
    matches!(
        name,
        "ChildItems" | "Attributes" | "Commands" | "Parameters" | "BaseForm" | "Columns" | "Events"
    )
}

pub(super) fn validate_property(
    node: Node<'_, '_>,
    property: &MetadataProperty,
) -> Result<Option<AuthoringProperty>, FormEditError> {
    if property.key.namespace.as_deref() == Some(FORM_NAMESPACE)
        && (structural(&property.key.name)
            || document::element(node).is_some()
                && ElementKind::from_xml_name(&property.key.name).is_some()
                && property.key.name != "Type")
    {
        return Err(invalid("property", "structural_property"));
    }
    if property.key.namespace.as_deref() != Some(FORM_NAMESPACE) {
        let existing =
            find_property(node, &property.key).ok_or(invalid("property", "unknown_property"))?;
        if existing
            .children()
            .any(|child| child.is_comment() || child.is_pi())
        {
            return Err(invalid("property", "unsupported_value"));
        }
        return Ok(None);
    }
    let definition = catalog(node).into_iter().find(|definition| {
        definition.name == property.key.name
            && matches!(
                definition.source,
                AuthoringSource::Property | AuthoringSource::Qualifier
            )
    });
    if let Some(definition) = &definition {
        validate_value(definition, &property.value)?;
    } else if find_property(node, &property.key).is_none() {
        return Err(invalid("property", "unknown_property"));
    }
    Ok(definition)
}

pub(super) fn validate_value(
    definition: &AuthoringProperty,
    value: &MetadataValue,
) -> Result<(), FormEditError> {
    if !supported_value(value) {
        return Err(invalid("property", "unsupported_value"));
    }
    match definition.editor {
        PaletteEditor::Boolean if !matches!(value, MetadataValue::Text(text) if matches!(text.as_str(), "true" | "false" | "1" | "0")) => {
            Err(invalid("property", "invalid_boolean"))
        }
        PaletteEditor::Number if !matches!(value, MetadataValue::Text(text) if text.parse::<f64>().is_ok_and(f64::is_finite)) => {
            Err(invalid("property", "invalid_number"))
        }
        PaletteEditor::Enum
            if !definition.values.is_empty()
                && !matches!(value, MetadataValue::Text(text) if definition.values.contains(&text.as_str())) =>
        {
            Err(invalid("property", "invalid_enum"))
        }
        PaletteEditor::Event if !matches!(value, MetadataValue::Text(text) if text.is_empty() || crate::project::metadata_rename::validate_name(text).is_ok()) => {
            Err(invalid("property", "invalid_handler"))
        }
        _ => Ok(()),
    }
}

pub(super) fn find_property<'a, 'input>(
    node: Node<'a, 'input>,
    key: &PropertyKey,
) -> Option<Node<'a, 'input>> {
    node.children().find(|node| {
        node.is_element()
            && node.tag_name().namespace() == key.namespace.as_deref()
            && node.tag_name().name() == key.name
    })
}

fn supported_value(value: &MetadataValue) -> bool {
    match value {
        MetadataValue::Unsupported(_) => false,
        MetadataValue::Record(properties) => properties
            .iter()
            .all(|property| supported_value(&property.value)),
        _ => true,
    }
}
