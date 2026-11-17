use roxmltree::Node;

use super::super::{FormEditError, document, invalid, render, schema};
use crate::project::{
    forms::{AuthoringSource, FORM_NAMESPACE},
    metadata_edit::{Replacement, escape_attribute, text_plan},
    metadata_model::{MetadataProperty, MetadataValue, PropertyKey},
    metadata_parser,
};

pub(super) fn set(
    input: &str,
    node: Node<'_, '_>,
    property: &MetadataProperty,
) -> Result<Vec<Replacement>, FormEditError> {
    unique_property(node, &property.key)?;
    let definition = schema::validate_property(node, property)?;
    if definition.is_some() {
        validate_command_dependencies(node, property)?;
    }
    if schema::defines_data_type(node) {
        validate_type(property, node)?;
    }
    if definition.is_some_and(|definition| definition.source == AuthoringSource::Qualifier) {
        let MetadataValue::Text(text) = &property.value else {
            return Err(invalid("property", "invalid_qualifier"));
        };
        if !property.qualifiers.is_empty() {
            return Err(invalid("property", "invalid_qualifier"));
        }
        return qualifier(input, node, &property.key.name, text);
    }
    let current = schema::find_property(node, &property.key);
    if let Some(current) = current {
        let existing = metadata_parser::values::property(current, &mut Vec::new());
        if existing == *property {
            return Ok(Vec::new());
        }
        if current.has_tag_name((FORM_NAMESPACE, "Type")) && schema::defines_data_type(node) {
            super::super::types::validate(current)?;
        }
        if existing.qualifiers == property.qualifiers
            && let MetadataValue::Text(text) = &property.value
            && matches!(existing.value, MetadataValue::Text(_))
        {
            return Ok(text_plan(input, current.range(), text)?
                .replacements()
                .to_vec());
        }
        if current
            .descendants()
            .any(|node| node.is_comment() || node.is_pi())
        {
            return Err(invalid("property", "unsupported_value"));
        }
        return Ok(vec![Replacement {
            range: current.range(),
            text: render::property(property, node)?,
        }]);
    }
    let before = node.children().filter(Node::is_element).find(|child| {
        schema::structural(child.tag_name().name()) || document::element(*child).is_some()
    });
    Ok(vec![document::insert(
        input,
        node,
        &render::property(property, node)?,
        before,
    )?])
}

fn validate_command_dependencies(
    node: Node<'_, '_>,
    property: &MetadataProperty,
) -> Result<(), FormEditError> {
    if !matches!(&property.value, MetadataValue::Text(value) if matches!(value.as_str(), "false" | "0"))
    {
        return Ok(());
    }
    let commands = || {
        node.document()
            .root_element()
            .descendants()
            .filter(|child| child.has_tag_name((FORM_NAMESPACE, "CommandName")))
    };
    let in_use = match (node.tag_name().name(), property.key.name.as_str()) {
        ("Form", "Customizable") => commands().any(|command| {
            command.text().map(str::trim) == Some("Form.StandardCommand.CustomizeForm")
        }),
        ("Table", "ChangeRowSet") => {
            let name = node.attribute("name").unwrap_or_default();
            let prefix = format!("Form.Item.{name}.StandardCommand.");
            commands().any(|command| {
                let Some(value) = command.text().map(str::trim) else {
                    return false;
                };
                let local = command
                    .ancestors()
                    .find(|parent| parent.has_tag_name((FORM_NAMESPACE, "Table")))
                    == Some(node);
                let action = value
                    .strip_prefix(&prefix)
                    .or_else(|| local.then_some(value));
                matches!(
                    action,
                    Some("Add" | "Copy" | "Create" | "Delete" | "MoveItem" | "SetDeletionMark")
                )
            })
        }
        _ => false,
    };
    // Designer rejects imports if a form disables commands still assigned to its buttons.
    if in_use {
        return Err(invalid("property", "used_standard_commands"));
    }
    Ok(())
}

pub(super) fn validate_type(
    property: &MetadataProperty,
    context: Node<'_, '_>,
) -> Result<(), FormEditError> {
    if property.key.namespace.as_deref() == Some(FORM_NAMESPACE) && property.key.name == "Type" {
        let fragment = render::property(property, context)?;
        let wrapper = format!(
            "<root{}>{fragment}</root>",
            context
                .namespaces()
                .map(|namespace| {
                    let suffix = namespace
                        .name()
                        .map_or_else(String::new, |prefix| format!(":{prefix}"));
                    Ok(format!(
                        " xmlns{suffix}=\"{}\"",
                        escape_attribute(namespace.uri())?
                    ))
                })
                .collect::<Result<String, FormEditError>>()?
        );
        let xml = document::xml(&wrapper)?;
        let node = xml
            .root_element()
            .first_element_child()
            .ok_or(invalid("type", "invalid_shape"))?;
        super::super::types::validate(node)?;
    }
    Ok(())
}

pub(super) fn qualifier(
    input: &str,
    node: Node<'_, '_>,
    name: &str,
    value: &str,
) -> Result<Vec<Replacement>, FormEditError> {
    if let Some(attribute) = node.attribute_node(name) {
        return Ok(vec![Replacement {
            range: attribute.range_value(),
            text: escape_attribute(value)?,
        }]);
    }
    let end = document::opening_end(input, node)?;
    let offset = if input.as_bytes()[end - 2] == b'/' {
        end - 2
    } else {
        end - 1
    };
    Ok(vec![Replacement {
        range: offset..offset,
        text: format!(" {name}=\"{}\"", escape_attribute(value)?),
    }])
}

pub(super) fn remove(
    node: Node<'_, '_>,
    key: &PropertyKey,
) -> Result<Vec<Replacement>, FormEditError> {
    unique_property(node, key)?;
    if schema::structural_property(node, key) {
        return Err(invalid("property", "structural_property"));
    }
    if key.namespace.as_deref() == Some(FORM_NAMESPACE)
        && schema::catalog(node).iter().any(|definition| {
            definition.name == key.name && definition.source == AuthoringSource::Qualifier
        })
    {
        return Ok(node
            .attribute_node(key.name.as_str())
            .map(|attribute| Replacement {
                range: attribute.range(),
                text: String::new(),
            })
            .into_iter()
            .collect());
    }
    Ok(schema::find_property(node, key)
        .map(|node| Replacement {
            range: node.range(),
            text: String::new(),
        })
        .into_iter()
        .collect())
}

fn unique_property(node: Node<'_, '_>, key: &PropertyKey) -> Result<(), FormEditError> {
    if node
        .children()
        .filter(|child| {
            child.is_element()
                && child.tag_name().namespace() == key.namespace.as_deref()
                && child.tag_name().name() == key.name
        })
        .take(2)
        .count()
        > 1
    {
        return Err(invalid("property", "duplicate_property"));
    }
    Ok(())
}

pub(super) fn event(
    input: &str,
    node: Node<'_, '_>,
    name: &str,
    handler: Option<&str>,
) -> Result<Vec<Replacement>, FormEditError> {
    if let Some(handler) = handler {
        crate::project::metadata_rename::validate_name(handler).map_err(FormEditError::Name)?;
    }
    if node.has_tag_name((FORM_NAMESPACE, "Command")) && name == "Action" {
        let key = render::key(FORM_NAMESPACE, "Action");
        return handler.map_or_else(
            || remove(node, &key),
            |handler| {
                set(
                    input,
                    node,
                    &MetadataProperty {
                        key: key.clone(),
                        qualifiers: Vec::new(),
                        value: MetadataValue::Text(handler.to_owned()),
                    },
                )
            },
        );
    }
    let events = document::child(node, "Events");
    let existing: Vec<_> = events
        .into_iter()
        .flat_map(|events| events.children())
        .filter(|event| {
            event.has_tag_name((FORM_NAMESPACE, "Event")) && event.attribute("name") == Some(name)
        })
        .collect();
    if existing.len() > 1 {
        return Err(invalid("event", "duplicate_event"));
    }
    if !schema::catalog(node)
        .iter()
        .any(|definition| definition.source == AuthoringSource::Event && definition.name == name)
        && existing.is_empty()
    {
        return Err(invalid("event", "unknown_event"));
    }
    match (existing.first(), handler) {
        (Some(event), Some(handler)) => Ok(text_plan(input, event.range(), handler)?
            .replacements()
            .to_vec()),
        (Some(event), None) => Ok(vec![Replacement {
            range: event.range(),
            text: String::new(),
        }]),
        (None, None) => Ok(Vec::new()),
        (None, Some(handler)) => {
            let event = MetadataProperty {
                key: render::key(FORM_NAMESPACE, "Event"),
                qualifiers: vec![(
                    PropertyKey {
                        namespace: None,
                        name: "name".to_owned(),
                    },
                    name.to_owned(),
                )],
                value: MetadataValue::Text(handler.to_owned()),
            };
            let fragment = render::property(&event, events.unwrap_or(node))?;
            if let Some(events) = events {
                return Ok(vec![document::insert(input, events, &fragment, None)?]);
            }
            let tag = render::tag(node, "Events");
            Ok(vec![document::insert(
                input,
                node,
                &format!("<{tag}>{fragment}</{tag}>"),
                None,
            )?])
        }
    }
}
