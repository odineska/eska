//! Semantic dispatch is independent from file publication and frontend presentation.

mod definitions;
mod elements;
mod properties;
pub(super) use elements::layout_kind;

use roxmltree::Document;

use super::{
    AssignedFormId, FormEditError, FormOperation, FormTarget, RemovedFormTarget, document, invalid,
};
use crate::project::metadata_edit::Replacement;

pub(super) struct Assignments<'a> {
    operation: usize,
    values: &'a mut Vec<AssignedFormId>,
}

impl Assignments<'_> {
    fn push(&mut self, target: FormTarget, id: Option<i64>) {
        self.values.push(AssignedFormId {
            operation: self.operation,
            target,
            id,
        });
    }
}

pub(super) fn apply(
    input: &str,
    xml: &Document<'_>,
    operation: &FormOperation,
    index: usize,
    assigned: &mut Vec<AssignedFormId>,
    removed: &mut Vec<RemovedFormTarget>,
) -> Result<Vec<Replacement>, FormEditError> {
    let mut assignments = Assignments {
        operation: index,
        values: assigned,
    };
    match operation {
        FormOperation::SetConditionalAppearance { rules } => {
            super::appearance::change(input, xml.root_element(), rules)
        }
        FormOperation::SetProperty { target, property } => {
            properties::set(input, document::target(xml, target)?, property)
        }
        FormOperation::RemoveProperty { target, key } => {
            properties::remove(document::target(xml, target)?, key)
        }
        FormOperation::SetEvent {
            target,
            event,
            handler,
        } => properties::event(
            input,
            document::target(xml, target)?,
            event,
            handler.as_deref(),
        ),
        FormOperation::AddElement {
            parent,
            before,
            element,
        } => elements::add(input, xml, parent, *before, element, &mut assignments),
        FormOperation::CopyElement {
            target,
            parent,
            before,
            name,
        } => elements::copy(input, xml, target, parent, *before, name, &mut assignments),
        FormOperation::MoveElement {
            target,
            parent,
            before,
        } => elements::relocate(input, xml, target, parent, *before),
        FormOperation::AddDefinition {
            section,
            parent,
            name,
            properties,
        } => definitions::add(
            input,
            xml,
            *section,
            parent.as_ref(),
            name,
            properties,
            &mut assignments,
        ),
        FormOperation::Remove { target, force } => {
            if matches!(target, FormTarget::Root {}) {
                return Err(invalid("target", "cannot_remove_root"));
            }
            let node = document::target(xml, target)?;
            if !force {
                let paths = references(xml, node);
                if !paths.is_empty() {
                    return Err(FormEditError::References {
                        target: target.clone(),
                        paths,
                    });
                }
            }
            // Resolve descendants at this step: prior moves/additions may change the removed tree.
            removed.push(RemovedFormTarget {
                target: target.clone(),
                names: node
                    .descendants()
                    .filter(|node| document::target_of(*node).is_some())
                    .filter_map(|node| node.attribute("name").map(str::to_lowercase))
                    .collect(),
                force: *force,
            });
            Ok(vec![Replacement {
                range: node.range(),
                text: String::new(),
            }])
        }
    }
}

fn references(xml: &Document<'_>, removed: roxmltree::Node<'_, '_>) -> Vec<String> {
    let ids: Vec<_> = removed
        .descendants()
        .filter(|node| document::element(*node).is_some())
        .filter_map(|node| node.attribute("id"))
        .collect();
    let targets: Vec<_> = removed
        .descendants()
        .filter_map(|node| {
            document::target_of(node)
                .map(|target| (target, node.attribute("name").unwrap_or_default()))
        })
        .collect();
    let mut paths = Vec::new();
    for node in xml.descendants().filter(roxmltree::Node::is_element) {
        if removed.range().contains(&node.range().start)
            || node.children().any(|child| child.is_element())
        {
            continue;
        }
        if targets
            .iter()
            .any(|(target, name)| super::appearance::references(node, target, name))
        {
            paths.push(format!("ConditionalAppearance@{}", node.range().start));
        }
        if node.tag_name().namespace() != Some(crate::project::forms::FORM_NAMESPACE) {
            continue;
        }
        let Some(text) = node.text() else {
            continue;
        };
        let text = text.trim();
        let local = node.tag_name().name();
        let by_id = id_reference(local) && ids.contains(&text);
        let by_name = targets
            .iter()
            .any(|(target, name)| named_reference(local, text, target, name));
        if by_id || by_name {
            paths.push(format!("{}@{}", local, node.range().start));
        }
    }
    paths
}

pub(super) fn id_reference(name: &str) -> bool {
    matches!(
        name,
        "AssociatedTableElementId" | "CommandBar" | "ContextMenu" | "ElementId" | "ItemId"
    ) || name.ends_with("ElementId")
}

/// Named element links include an optional command/source prefix and a command suffix.
/// Their namespace is distinct from form commands, attributes and BSL method names.
pub(super) fn element_name_reference<'a>(
    property: &str,
    value: &'a str,
) -> Option<(&'a str, &'a str, &'a str)> {
    let reference = match property {
        "Element" | "Item" => value,
        "CommandSource" => value.strip_prefix("Item.")?,
        "CommandName" => value.strip_prefix("Form.Item.")?,
        _ => return None,
    };
    let offset = value.len() - reference.len();
    let end = reference.find('.').unwrap_or(reference.len());
    (end != 0).then_some((&value[..offset], &reference[..end], &reference[end..]))
}

/// Designer keeps element, attribute and command names in independent namespaces.
fn named_reference(property: &str, value: &str, target: &FormTarget, element_name: &str) -> bool {
    let (name, reference, descendants) = match target {
        FormTarget::Element { .. } => (
            element_name,
            element_name_reference(property, value).map(|(_, name, _)| name),
            false,
        ),
        FormTarget::Attribute { name } => (
            name.as_str(),
            (property.ends_with("DataPath") || matches!(property, "DataSource" | "Path"))
                .then_some(value),
            true,
        ),
        FormTarget::Command { name } => (
            name.as_str(),
            (property == "CommandName")
                .then(|| value.strip_prefix("Form.Command.").unwrap_or(value)),
            false,
        ),
        FormTarget::Root {} | FormTarget::Parameter { .. } => return false,
    };
    reference.is_some_and(|reference| {
        let name = name.to_lowercase();
        let reference = reference.to_lowercase();
        reference == name
            || descendants
                && reference
                    .strip_prefix(&name)
                    .is_some_and(|rest| rest.starts_with('.'))
    })
}
