use super::Assignments;
use roxmltree::{Document, Node};
use std::fmt::Write;

use super::super::{
    FormDefinitionKind, FormEditError, FormTarget, document, invalid, render, schema,
};
use crate::project::{
    forms::FORM_NAMESPACE,
    metadata_edit::{Replacement, escape_attribute},
    metadata_model::MetadataProperty,
    metadata_rename::validate_name,
};

pub(super) fn add(
    input: &str,
    xml: &Document<'_>,
    section: FormDefinitionKind,
    parent: Option<&FormTarget>,
    name: &str,
    properties: &[MetadataProperty],
    assigned: &mut Assignments<'_>,
) -> Result<Vec<Replacement>, FormEditError> {
    validate_name(name).map_err(FormEditError::Name)?;
    let owner = parent
        .map(|parent| document::target(xml, parent))
        .transpose()?
        .unwrap_or_else(|| xml.root_element());
    let column = parent.is_some_and(|parent| !matches!(parent, FormTarget::Root {}));
    if column
        && (section != FormDefinitionKind::Attribute
            || !matches!(parent, Some(FormTarget::Attribute { .. })))
    {
        return Err(invalid("parent", "incompatible_definition"));
    }
    let section_name = if column { "Columns" } else { section.section() };
    let tag_name = if column { "Column" } else { section.tag() };
    let container = document::child(owner, section_name);
    if container.is_some_and(|container| {
        container.children().any(|node| {
            node.attribute("name")
                .is_some_and(|current| current.to_lowercase() == name.to_lowercase())
        })
    }) {
        return Err(FormEditError::Collision(name.to_owned()));
    }
    let id = if section == FormDefinitionKind::Parameter {
        None
    } else {
        let previous = container
            .into_iter()
            .flat_map(|container| container.children())
            .filter_map(|node| node.attribute("id").and_then(|id| id.parse::<i64>().ok()))
            .max()
            .unwrap_or(0)
            .max(0);
        if previous >= i64::from(i32::MAX) {
            return Err(invalid("id", "exhausted"));
        }
        Some(previous + 1)
    };
    let qualified_name = if let Some(FormTarget::Attribute { name: owner }) = parent {
        format!("{owner}.{name}")
    } else {
        name.to_owned()
    };
    let target = match section {
        FormDefinitionKind::Attribute => FormTarget::Attribute {
            name: qualified_name,
        },
        FormDefinitionKind::Command => FormTarget::Command {
            name: qualified_name,
        },
        FormDefinitionKind::Parameter => FormTarget::Parameter {
            name: qualified_name,
        },
    };
    assigned.push(target, id);
    let scope = container.unwrap_or(owner);
    let content = definition_properties(scope, tag_name, section, properties)?;
    let tag = render::tag(scope, tag_name);
    let id = id.map_or_else(String::new, |id| format!(" id=\"{id}\""));
    let fragment = format!(
        "<{tag} name=\"{}\"{id}>{content}</{tag}>",
        escape_attribute(name)?
    );
    if let Some(container) = container {
        return Ok(vec![document::insert(input, container, &fragment, None)?]);
    }
    let section_tag = render::tag(owner, section_name);
    Ok(vec![document::insert(
        input,
        owner,
        &format!("<{section_tag}>{fragment}</{section_tag}>"),
        None,
    )?])
}

fn definition_properties(
    owner: Node<'_, '_>,
    tag_name: &str,
    section: FormDefinitionKind,
    properties: &[MetadataProperty],
) -> Result<String, FormEditError> {
    let catalog = schema::definition_catalog(tag_name);
    let mut content = String::new();
    let mut seen = Vec::new();
    for property in properties {
        if seen.contains(&property.key) {
            return Err(invalid("property", "duplicate_property"));
        }
        seen.push(property.key.clone());
        let definition = catalog
            .iter()
            .find(|definition| {
                definition.name == property.key.name
                    && property.key.namespace.as_deref() == Some(FORM_NAMESPACE)
            })
            .ok_or(invalid("property", "unknown_property"))?;
        schema::validate_value(definition, &property.value)?;
        super::properties::validate_type(property, owner)?;
        content.push_str(&render::property(property, owner)?);
    }
    // An empty Type means a runtime-typed form value; it is an explicit native default.
    if section != FormDefinitionKind::Command
        && !properties
            .iter()
            .any(|property| property.key.name == "Type")
    {
        let tag = render::tag(owner, "Type");
        let _ = write!(content, "<{tag}/>");
    }
    Ok(content)
}
