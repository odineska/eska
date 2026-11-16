use super::Assignments;
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write,
};

use roxmltree::{Document, Node};

use super::super::{FormEditError, FormElementSpec, FormTarget, document, invalid, render, schema};
use crate::project::{
    forms::{ElementFamily, ElementKind, FORM_NAMESPACE, authoring_properties},
    metadata_edit::{EditPlan, Replacement, escape_attribute, text_plan},
    metadata_rename::validate_name,
};

pub(super) fn add(
    input: &str,
    xml: &Document<'_>,
    parent: &FormTarget,
    before: Option<i64>,
    spec: &FormElementSpec,
    assigned: &mut Assignments<'_>,
) -> Result<Vec<Replacement>, FormEditError> {
    let parent = document::target(xml, parent)?;
    let mut allocation = Allocation::new(xml)?;
    let scope = document::child(parent, "ChildItems").unwrap_or(parent);
    let fragment = allocation.render(spec, scope, assigned, 0, false)?;
    layout_parent(parent, kind(&spec.kind)?)?;
    Ok(vec![insert_child(input, xml, parent, before, &fragment)?])
}

pub(super) fn copy(
    input: &str,
    xml: &Document<'_>,
    selected: &FormTarget,
    parent: &FormTarget,
    before: Option<i64>,
    name: &str,
    assigned: &mut Assignments<'_>,
) -> Result<Vec<Replacement>, FormEditError> {
    validate_name(name).map_err(FormEditError::Name)?;
    let node = document::target(xml, selected)?;
    let parent = document::target(xml, parent)?;
    let node_kind = movable(node)?;
    layout_parent(parent, node_kind)?;
    let mut allocation = Allocation::new(xml)?;
    allocation.name(name)?;
    let mut ids = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut changes = Vec::new();
    for item in node
        .descendants()
        .filter(|item| document::is_element(*item))
    {
        let id = item
            .attribute_node("id")
            .ok_or(invalid("element", "missing_id"))?;
        let next = allocation.id()?;
        ids.insert(id.value().to_owned(), next.to_string());
        changes.push(Replacement {
            range: id.range_value(),
            text: next.to_string(),
        });
        assigned.push(FormTarget::Element { id: next }, Some(next));
        if let Some(attribute) = item.attribute_node("name") {
            let new_name = if item == node {
                name.to_owned()
            } else {
                allocation.copy_name(attribute.value())?
            };
            names.insert(attribute.value().to_owned(), new_name.clone());
            changes.push(Replacement {
                range: attribute.range_value(),
                text: escape_attribute(&new_name)?,
            });
        }
    }
    for item in node
        .descendants()
        .filter(Node::is_element)
        .filter(|item| item.tag_name().namespace() == Some(FORM_NAMESPACE))
        .filter(|item| !item.children().any(|child| child.is_element()))
    {
        let Some(value) = item.text() else {
            continue;
        };
        let property = item.tag_name().name();
        let replacement = if super::id_reference(property) {
            ids.get(value).cloned()
        } else if matches!(property, "Element" | "Item") {
            remap_name(value, &names)
        } else {
            None
        };
        if let Some(value) = replacement {
            changes.extend(
                text_plan(input, item.range(), &value)?
                    .replacements()
                    .iter()
                    .cloned(),
            );
        }
    }
    let rewritten = EditPlan::from_replacements(input, changes)?;
    let shifted_end = node.range().end + rewritten.output().len() - input.len();
    let fragment = &rewritten.output()[node.range().start..shifted_end];
    let scope = document::child(parent, "ChildItems").unwrap_or(parent);
    let fragment = namespace_fragment(fragment, node, scope)?;
    Ok(vec![insert_child(input, xml, parent, before, &fragment)?])
}

pub(super) fn relocate(
    input: &str,
    xml: &Document<'_>,
    selected: &FormTarget,
    parent: &FormTarget,
    before: Option<i64>,
) -> Result<Vec<Replacement>, FormEditError> {
    let node = document::target(xml, selected)?;
    let parent = document::target(xml, parent)?;
    layout_parent(parent, movable(node)?)?;
    if node.range().contains(&parent.range().start) {
        return Err(invalid("parent", "cycle"));
    }
    if before.is_some_and(
        |id| matches!(selected, FormTarget::Element { id: selected } if id == *selected),
    ) {
        return Ok(Vec::new());
    }
    let scope = document::child(parent, "ChildItems").unwrap_or(parent);
    let fragment = namespace_fragment(&input[node.range()], node, scope)?;
    let insertion = insert_child(input, xml, parent, before, &fragment)?;
    Ok(vec![
        Replacement {
            range: node.range(),
            text: String::new(),
        },
        insertion,
    ])
}

fn movable(node: Node<'_, '_>) -> Result<ElementKind, FormEditError> {
    let kind = document::element(node).ok_or(invalid("target", "not_element"))?;
    if kind == ElementKind::Form
        || !node
            .parent()
            .is_some_and(|parent| parent.has_tag_name((FORM_NAMESPACE, "ChildItems")))
    {
        return Err(invalid("target", "not_layout_element"));
    }
    Ok(kind)
}

fn kind(value: &str) -> Result<ElementKind, FormEditError> {
    ElementKind::from_xml_name(value)
        .filter(|kind| *kind != ElementKind::Form)
        .ok_or(invalid("element.kind", "unknown_element"))
}

fn layout_parent(parent: Node<'_, '_>, child: ElementKind) -> Result<(), FormEditError> {
    let parent = document::element(parent).ok_or(invalid("parent", "not_element"))?;
    layout_kind(parent, child)
}

fn layout_kind(parent: ElementKind, child: ElementKind) -> Result<(), FormEditError> {
    if !matches!(
        parent.family(),
        ElementFamily::Root | ElementFamily::Group | ElementFamily::Table
    ) && !matches!(
        parent,
        ElementKind::ContextMenu | ElementKind::AutoCommandBar
    ) {
        return Err(invalid("parent", "not_container"));
    }
    if (parent == ElementKind::Pages) != (child == ElementKind::Page)
        || child.family() == ElementFamily::Auxiliary
    {
        return Err(invalid("parent", "incompatible_element"));
    }
    if matches!(
        parent,
        ElementKind::CommandBar
            | ElementKind::AutoCommandBar
            | ElementKind::ContextMenu
            | ElementKind::ButtonGroup
            | ElementKind::Popup
    ) && !matches!(
        child,
        ElementKind::Button | ElementKind::ButtonGroup | ElementKind::Popup
    ) {
        return Err(invalid("parent", "incompatible_element"));
    }
    Ok(())
}

fn insert_child(
    input: &str,
    xml: &Document<'_>,
    parent: Node<'_, '_>,
    before: Option<i64>,
    fragment: &str,
) -> Result<Replacement, FormEditError> {
    let children = document::child(parent, "ChildItems");
    let before = before
        .map(|id| document::target(xml, &FormTarget::Element { id }))
        .transpose()?;
    if before.is_some_and(|before| before.parent() != children) {
        return Err(invalid("before", "not_direct_child"));
    }
    if let Some(children) = children {
        return document::insert(input, children, fragment, before);
    }
    let tag = render::tag(parent, "ChildItems");
    let before = parent.children().filter(Node::is_element).find(|node| {
        matches!(
            node.tag_name().name(),
            "Attributes" | "Commands" | "Parameters"
        )
    });
    document::insert(input, parent, &format!("<{tag}>{fragment}</{tag}>"), before)
}

struct Allocation {
    next: i64,
    names: HashSet<String>,
}

impl Allocation {
    fn new(xml: &Document<'_>) -> Result<Self, FormEditError> {
        let elements: Vec<_> = xml
            .descendants()
            .filter(|node| document::is_element(*node))
            .collect();
        let max = elements
            .iter()
            .filter_map(|node| node.attribute("id").and_then(|id| id.parse::<i64>().ok()))
            .max()
            .unwrap_or(0)
            .max(0);
        Ok(Self {
            next: max.checked_add(1).ok_or(invalid("id", "exhausted"))?,
            names: elements
                .iter()
                .filter_map(|node| node.attribute("name").map(str::to_lowercase))
                .collect(),
        })
    }

    fn id(&mut self) -> Result<i64, FormEditError> {
        if self.next > i64::from(i32::MAX) {
            return Err(invalid("id", "exhausted"));
        }
        let value = self.next;
        self.next += 1;
        Ok(value)
    }

    fn name(&mut self, name: &str) -> Result<(), FormEditError> {
        validate_name(name).map_err(FormEditError::Name)?;
        if !self.names.insert(name.to_lowercase()) {
            return Err(FormEditError::Collision(name.to_owned()));
        }
        Ok(())
    }

    fn copy_name(&mut self, name: &str) -> Result<String, FormEditError> {
        for suffix in 1..=crate::project::forms::MAX_FORM_ELEMENTS {
            let suffix = format!("_Copy{suffix}");
            let mut prefix = String::new();
            let mut length = suffix.len();
            for ch in name.chars() {
                if length + ch.len_utf16() > 80 {
                    break;
                }
                prefix.push(ch);
                length += ch.len_utf16();
            }
            let candidate = format!("{prefix}{suffix}");
            if self.names.insert(candidate.to_lowercase()) {
                return Ok(candidate);
            }
        }
        Err(invalid("name", "exhausted"))
    }

    fn render(
        &mut self,
        spec: &FormElementSpec,
        scope: Node<'_, '_>,
        assigned: &mut Assignments<'_>,
        depth: usize,
        auxiliary: bool,
    ) -> Result<String, FormEditError> {
        if depth > 48 {
            return Err(invalid("element", "too_deep"));
        }
        let kind = kind(&spec.kind)?;
        if auxiliary
            && !matches!(
                kind.family(),
                ElementFamily::Auxiliary | ElementFamily::Addition
            )
        {
            return Err(invalid("auxiliary", "not_auxiliary"));
        }
        self.name(&spec.name)?;
        let id = self.id()?;
        assigned.push(FormTarget::Element { id }, Some(id));
        let mut content = String::new();
        let catalog = authoring_properties(kind);
        let mut seen = Vec::new();
        for property in &spec.properties {
            if seen.contains(&property.key) {
                return Err(invalid("property", "duplicate_property"));
            }
            seen.push(property.key.clone());
            let definition = catalog
                .iter()
                .find(|definition| {
                    definition.source == crate::project::forms::AuthoringSource::Property
                        && definition.name == property.key.name
                        && property.key.namespace.as_deref() == Some(FORM_NAMESPACE)
                })
                .ok_or(invalid("property", "unknown_property"))?;
            schema::validate_value(definition, &property.value)?;
            content.push_str(&render::property(property, scope)?);
        }
        for spec in &spec.auxiliary {
            content.push_str(&self.render(spec, scope, assigned, depth + 1, true)?);
        }
        if !spec.children.is_empty() {
            let tag = render::tag(scope, "ChildItems");
            let _ = write!(content, "<{tag}>");
            for child in &spec.children {
                layout_kind(kind, kind_from_spec(child)?)?;
                content.push_str(&self.render(child, scope, assigned, depth + 1, false)?);
            }
            let _ = write!(content, "</{tag}>");
        }
        let tag = render::tag(scope, kind.xml_name());
        Ok(if content.is_empty() {
            format!(
                "<{tag} name=\"{}\" id=\"{id}\"/>",
                escape_attribute(&spec.name)?
            )
        } else {
            format!(
                "<{tag} name=\"{}\" id=\"{id}\">{content}</{tag}>",
                escape_attribute(&spec.name)?
            )
        })
    }
}

fn kind_from_spec(spec: &FormElementSpec) -> Result<ElementKind, FormEditError> {
    kind(&spec.kind)
}

fn remap_name(value: &str, names: &BTreeMap<String, String>) -> Option<String> {
    names.iter().find_map(|(old, new)| {
        if value == old {
            return Some(new.clone());
        }
        value
            .strip_prefix(old)
            .filter(|rest| rest.starts_with('.'))
            .map(|rest| format!("{new}{rest}"))
    })
}

/// Preserve inherited bindings when moving under a different namespace scope.
fn namespace_fragment(
    fragment: &str,
    original: Node<'_, '_>,
    destination: Node<'_, '_>,
) -> Result<String, FormEditError> {
    let mut reader = quick_xml::Reader::from_str(fragment);
    let opening = reader
        .read_event()
        .map_err(|_| invalid("element", "invalid_xml"))?;
    let (quick_xml::events::Event::Start(opening) | quick_xml::events::Event::Empty(opening)) =
        opening
    else {
        return Err(invalid("element", "invalid_xml"));
    };
    let mut explicit = HashSet::new();
    for attribute in opening.attributes() {
        let attribute = attribute.map_err(|_| invalid("element", "invalid_xml"))?;
        if attribute.key.as_ref() == "xmlns" {
            explicit.insert(String::new());
        }
        if let Some(prefix) = attribute.key.as_ref().strip_prefix("xmlns:") {
            explicit.insert(prefix.to_owned());
        }
    }
    let mut declarations = String::new();
    // No default namespace is also inherited state: the destination must not supply one.
    if original.lookup_namespace_uri(None).is_none()
        && destination.lookup_namespace_uri(None).is_some()
        && !explicit.contains("")
    {
        declarations.push_str(" xmlns=\"\"");
    }
    for namespace in original.namespaces() {
        if destination.lookup_namespace_uri(namespace.name()) == Some(namespace.uri()) {
            continue;
        }
        // Explicit declarations are already present on the fragment root.
        if explicit.contains(namespace.name().unwrap_or_default()) {
            continue;
        }
        let suffix = namespace
            .name()
            .map_or_else(String::new, |prefix| format!(":{prefix}"));
        let _ = write!(
            declarations,
            " xmlns{suffix}=\"{}\"",
            escape_attribute(namespace.uri())?
        );
    }
    if declarations.is_empty() {
        return Ok(fragment.to_owned());
    }
    let end = fragment
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '>'))
        .ok_or(invalid("element", "invalid_xml"))?;
    let mut output = fragment.to_owned();
    output.insert_str(end, &declarations);
    Ok(output)
}
