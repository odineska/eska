//! Checked byte-level operations retain unknown XML and all untouched source bytes.

use std::collections::{HashSet, VecDeque};

use roxmltree::{Document, Node};

use super::{
    AssignedFormId, FormChanges, FormEditError, FormEditRequest, FormTarget, RemovedFormTarget,
    invalid,
};
use crate::project::{
    forms::{self, ElementKind, FORM_NAMESPACE, FormIssue},
    metadata_edit::{
        EditError, EditPlan, MAX_SOURCE_HISTORY_BYTES, MAX_SOURCE_HISTORY_STEPS, Replacement,
        SavedEdit, snapshot, xml_replacements,
    },
};

/// Minimal managed form accepted and normalized by the platform for both form owners.
#[must_use]
pub fn empty_form() -> String {
    concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\" version=\"2.20\">\n",
        "\t<AutoCommandBar name=\"FormCommandBar\" id=\"-1\"/>\n",
        "\t<Attributes/>\n",
        "</Form>\n"
    )
    .to_owned()
}

/// Compute semantic changes without writing files or allocating nondeterministic identities.
///
/// # Errors
/// Rejects stale snapshots, ambiguous source identities and unsupported semantic changes.
pub fn plan(
    input: &str,
    request: &FormEditRequest,
) -> Result<(EditPlan, Vec<AssignedFormId>), FormEditError> {
    let changes = plan_complete(input, request)?;
    Ok((changes.edit, changes.assigned_ids))
}

pub fn plan_complete(input: &str, request: &FormEditRequest) -> Result<FormChanges, FormEditError> {
    plan_with_steps(input, request, &[])
}

pub fn plan_with_steps(
    input: &str,
    request: &FormEditRequest,
    steps: &[usize],
) -> Result<FormChanges, FormEditError> {
    if snapshot(input) != request.snapshot {
        return Err(EditError::Conflict.into());
    }
    if request.operations.len() > 1024 {
        return Err(invalid("operations", "too_many"));
    }
    if !steps.is_empty()
        && (steps.last() != Some(&request.operations.len())
            || steps[0] == 0
            || steps.windows(2).any(|pair| pair[0] >= pair[1]))
    {
        return Err(invalid("steps", "invalid_boundaries"));
    }
    validate(input)?;
    let mut output = input.to_owned();
    let mut assigned = Vec::new();
    let mut removed = Vec::new();
    let mut previous = (!steps.is_empty()).then(|| input.to_owned());
    let mut boundaries = steps.iter().copied().peekable();
    let mut history = VecDeque::new();
    for (index, operation) in request.operations.iter().enumerate() {
        let document = xml(&output)?;
        let replacements = super::operations::apply(
            &output,
            &document,
            operation,
            index,
            &mut assigned,
            &mut removed,
        )?;
        xml_replacements(&output, replacements)?
            .output()
            .clone_into(&mut output);
        validate(&output)?;
        if boundaries.peek() == Some(&(index + 1))
            && let Some(previous) = &mut previous
        {
            boundaries.next();
            let edit = xml_replacements(previous, difference(previous, &output))?;
            if !edit.is_empty() {
                history.push_back(edit.history());
                while history.len() > MAX_SOURCE_HISTORY_STEPS
                    || history.iter().map(SavedEdit::bytes).sum::<usize>()
                        > MAX_SOURCE_HISTORY_BYTES
                {
                    history.pop_front();
                }
            }
            previous.clone_from(&output);
        }
    }
    let replacements = difference(input, &output);
    Ok(FormChanges {
        edit: xml_replacements(input, replacements)?,
        assigned_ids: assigned,
        removed,
        steps: history.into(),
    })
}

pub(super) fn validate(input: &str) -> Result<(), FormEditError> {
    let form = forms::parse("authoring", input.to_owned())?;
    if form.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.issue,
            FormIssue::InvalidIdentity | FormIssue::DuplicateIdentity | FormIssue::RepeatedSection
        )
    }) {
        return Err(invalid("form", "ambiguous_structure"));
    }
    // A borrowed extension form is a delta plus a stored baseline, not an ordinary editable tree.
    if form.base.is_some() {
        return Err(invalid("form", "borrowed_form"));
    }
    let document = xml(input)?;
    for container in document.descendants().filter(|node| {
        node.tag_name().namespace() == Some(FORM_NAMESPACE)
            && (matches!(
                node.tag_name().name(),
                "Attributes" | "Commands" | "Parameters"
            ) && node.parent() == Some(document.root_element())
                || node.tag_name().name() == "Columns"
                    && node.parent().and_then(target_of).is_some())
    }) {
        let mut names = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for item in container.children().filter(Node::is_element) {
            if let Some(name) = item.attribute("name")
                && !names.insert(name.to_lowercase())
            {
                return Err(invalid("form", "duplicate_definition_name"));
            }
            if let Some(id) = item.attribute("id") {
                let id = id
                    .parse::<i64>()
                    .map_err(|_| invalid("id", "invalid_integer"))?;
                if !ids.insert(id) {
                    return Err(invalid("form", "duplicate_definition_id"));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn xml(input: &str) -> Result<Document<'_>, FormEditError> {
    crate::project::metadata_parser::parse_xml(input, 500_000)
        .map_err(|_| EditError::InvalidXml.into())
}

pub fn removed_targets(before: &str, after: &str) -> Result<Vec<RemovedFormTarget>, FormEditError> {
    let before = xml(before)?;
    let after = xml(after)?;
    let remaining: HashSet<_> = after.descendants().filter_map(named_target).collect();
    Ok(before
        .descendants()
        .filter_map(named_target)
        .filter(|target| !remaining.contains(target))
        .map(|(target, name)| RemovedFormTarget {
            target,
            names: vec![name],
            force: false,
        })
        .collect())
}

fn named_target(node: Node<'_, '_>) -> Option<(FormTarget, String)> {
    Some((target_of(node)?, node.attribute("name")?.to_lowercase()))
}

pub(super) fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.has_tag_name((FORM_NAMESPACE, name)))
}

pub(super) fn element(node: Node<'_, '_>) -> Option<ElementKind> {
    (is_element(node) && node.tag_name().namespace() == Some(FORM_NAMESPACE))
        .then(|| ElementKind::from_xml_name(node.tag_name().name()))
        .flatten()
}

pub(super) fn is_element(node: Node<'_, '_>) -> bool {
    if !node.is_element() {
        return false;
    }
    if node == node.document().root_element() {
        return node.has_tag_name((FORM_NAMESPACE, "Form"));
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.has_tag_name((FORM_NAMESPACE, "ChildItems")) {
        return parent.parent().is_some_and(is_element);
    }
    node.tag_name().namespace() == Some(FORM_NAMESPACE)
        && ElementKind::from_xml_name(node.tag_name().name()).is_some_and(|kind| {
            matches!(
                kind.family(),
                forms::ElementFamily::Auxiliary | forms::ElementFamily::Addition
            )
        })
        && is_element(parent)
}

pub(super) fn target<'a, 'input>(
    document: &'a Document<'input>,
    target: &FormTarget,
) -> Result<Node<'a, 'input>, FormEditError> {
    let root = document.root_element();
    let found = match target {
        FormTarget::Root {} => Some(root),
        FormTarget::Element { id } => root.descendants().find(|node| {
            is_element(*node)
                && node
                    .attribute("id")
                    .and_then(|value| value.parse::<i64>().ok())
                    == Some(*id)
        }),
        FormTarget::Attribute { name }
        | FormTarget::Command { name }
        | FormTarget::Parameter { name } => {
            let section = match target {
                FormTarget::Attribute { .. } => "Attributes",
                FormTarget::Command { .. } => "Commands",
                _ => "Parameters",
            };
            let mut parts = name.split('.');
            let mut found = child(root, section).and_then(|container| {
                container.children().find(|node| {
                    node.tag_name().namespace() == Some(FORM_NAMESPACE)
                        && node.attribute("name") == parts.clone().next()
                })
            });
            parts.next();
            for part in parts {
                found = found
                    .and_then(|node| child(node, "Columns"))
                    .and_then(|container| {
                        container.children().find(|node| {
                            node.has_tag_name((FORM_NAMESPACE, "Column"))
                                && node.attribute("name") == Some(part)
                        })
                    });
            }
            found
        }
    };
    found.ok_or_else(|| FormEditError::MissingTarget(target.clone()))
}

pub(super) fn target_of(node: Node<'_, '_>) -> Option<FormTarget> {
    if node == node.document().root_element() {
        return Some(FormTarget::Root {});
    }
    if is_element(node) {
        return node
            .attribute("id")
            .and_then(|id| id.parse().ok())
            .map(|id| FormTarget::Element { id });
    }
    let name = node.attribute("name")?.to_owned();
    if node.tag_name().namespace() != Some(FORM_NAMESPACE) {
        return None;
    }
    let parent = node.parent()?;
    match node.tag_name().name() {
        "Attribute"
            if parent.has_tag_name((FORM_NAMESPACE, "Attributes"))
                && parent.parent() == Some(node.document().root_element()) =>
        {
            Some(FormTarget::Attribute { name })
        }
        "Column"
            if parent.has_tag_name((FORM_NAMESPACE, "Columns"))
                && parent
                    .parent()
                    .and_then(target_of)
                    .is_some_and(|target| matches!(target, FormTarget::Attribute { .. })) =>
        {
            let mut names: Vec<_> = node
                .ancestors()
                .filter_map(|parent| parent.attribute("name"))
                .collect();
            names.reverse();
            Some(FormTarget::Attribute {
                name: names.join("."),
            })
        }
        "Command"
            if parent.has_tag_name((FORM_NAMESPACE, "Commands"))
                && parent.parent() == Some(node.document().root_element()) =>
        {
            Some(FormTarget::Command { name })
        }
        "Parameter"
            if parent.has_tag_name((FORM_NAMESPACE, "Parameters"))
                && parent.parent() == Some(node.document().root_element()) =>
        {
            Some(FormTarget::Parameter { name })
        }
        _ => None,
    }
}

/// Scan only the start tag; quoted values may contain `>`.
pub(super) fn opening_end(input: &str, node: Node<'_, '_>) -> Result<usize, FormEditError> {
    let mut quote = None;
    for (offset, byte) in input.as_bytes()[node.range()].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(current), value) if current == value => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Ok(node.range().start + offset + 1),
            _ => {}
        }
    }
    Err(EditError::InvalidXml.into())
}

pub(super) fn insert(
    input: &str,
    parent: Node<'_, '_>,
    fragment: &str,
    before: Option<Node<'_, '_>>,
) -> Result<Replacement, FormEditError> {
    if let Some(before) = before {
        return Ok(Replacement {
            range: before.range().start..before.range().start,
            text: fragment.to_owned(),
        });
    }
    let open = opening_end(input, parent)?;
    if input.as_bytes().get(open.wrapping_sub(2)) == Some(&b'/') {
        let start = parent.range().start + 1;
        let name_end = input[start..open]
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '>'))
            .map_or(open - 1, |offset| start + offset);
        let name = &input[start..name_end];
        return Ok(Replacement {
            range: open - 2..open,
            text: format!(">{fragment}</{name}>"),
        });
    }
    let end = input[parent.range()]
        .rfind("</")
        .ok_or(EditError::InvalidXml)?
        + parent.range().start;
    Ok(Replacement {
        range: end..end,
        text: fragment.to_owned(),
    })
}

fn difference(before: &str, after: &str) -> Vec<Replacement> {
    if before == after {
        return Vec::new();
    }
    let mut start = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !before.is_char_boundary(start) || !after.is_char_boundary(start) {
        start -= 1;
    }
    let mut suffix = before[start..]
        .bytes()
        .rev()
        .zip(after[start..].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    vec![Replacement {
        range: start..before.len() - suffix,
        text: after[start..after.len() - suffix].to_owned(),
    }]
}
