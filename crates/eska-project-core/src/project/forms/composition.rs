//! Apply the delta against each stored borrowing baseline, retaining foreign extension changes.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use serde::Serialize;

use super::{
    ElementIdentity, ElementKey, ElementKind, FormDefinition, FormDocument, FormElement,
    LocatedQualifier, is_form_property,
};
use crate::project::{
    metadata_model::{MetadataProperty, PropertyKey},
    metadata_parser::LocatedProperty,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertySource {
    pub source: String,
    pub range: Range<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcedProperty {
    pub origin: PropertySource,
    pub property: MetadataProperty,
}

impl SourcedProperty {
    pub(super) fn has_form_name(&self, name: &str) -> bool {
        is_form_property(&self.property.key, name)
    }

    pub(super) fn text(&self) -> Option<&str> {
        super::appearance::text(&self.property.value)
    }
}

fn find_property<'a>(values: &'a [SourcedProperty], name: &str) -> Option<&'a SourcedProperty> {
    values.iter().find(|value| value.has_form_name(name))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcedQualifier {
    pub key: PropertyKey,
    pub text: String,
    pub origin: PropertySource,
}

impl SourcedQualifier {
    fn new(value: &LocatedQualifier, source: &str) -> Self {
        Self {
            key: value.key.clone(),
            text: value.text.clone(),
            origin: origin(source, value.range.clone()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveElement {
    pub identity: ElementIdentity,
    pub origin: PropertySource,
    pub kind: Option<ElementKind>,
    pub xml_name: PropertyKey,
    pub name: Option<String>,
    pub qualifiers: Vec<SourcedQualifier>,
    pub properties: Vec<SourcedProperty>,
    pub children: Vec<Self>,
    pub auxiliary: Vec<Self>,
}

impl EffectiveElement {
    pub(super) fn property(&self, name: &str) -> Option<&SourcedProperty> {
        find_property(&self.properties, name)
    }

    pub(super) fn scalar(&self, name: &str) -> Option<&str> {
        self.property(name).and_then(SourcedProperty::text)
    }

    /// Find a composed identity, including elements outside the main layout tree.
    #[must_use]
    pub fn find(&self, identity: &ElementIdentity) -> Option<&Self> {
        if &self.identity == identity {
            return Some(self);
        }
        self.children
            .iter()
            .chain(&self.auxiliary)
            .find_map(|child| child.find(identity))
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveDefinition {
    pub origin: PropertySource,
    pub xml_name: PropertyKey,
    pub name: Option<String>,
    pub qualifiers: Vec<(PropertyKey, String)>,
    pub properties: Vec<SourcedProperty>,
}

impl EffectiveDefinition {
    pub(super) fn property(&self, name: &str) -> Option<&SourcedProperty> {
        find_property(&self.properties, name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionIssue {
    InvalidDocument,
    MissingBase,
    MissingBorrowedElement,
    DuplicateName,
    InvalidHierarchy,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormCompositionDiagnostic {
    pub issue: CompositionIssue,
    pub source: String,
    pub element: Option<ElementKey>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposedForm {
    pub root: EffectiveElement,
    pub attributes: Vec<EffectiveDefinition>,
    pub attribute_properties: Vec<SourcedProperty>,
    pub commands: Vec<EffectiveDefinition>,
    pub parameters: Vec<EffectiveDefinition>,
    pub diagnostics: Vec<FormCompositionDiagnostic>,
}

impl ComposedForm {
    pub(super) fn main_attribute(&self) -> Option<&EffectiveDefinition> {
        self.attributes.iter().find(|attribute| {
            attribute.properties.iter().any(|value| {
                value.has_form_name("MainAttribute") && matches!(value.text(), Some("true" | "1"))
            })
        })
    }

    /// Form-level settings under Attributes follow the root's direct properties in the inspector.
    pub fn inspectable_properties<'a>(
        &'a self,
        element: &'a EffectiveElement,
    ) -> impl Iterator<Item = &'a SourcedProperty> {
        let attributes = if element.identity == self.root.identity {
            self.attribute_properties.as_slice()
        } else {
            &[]
        };
        element.properties.iter().chain(attributes)
    }
}

/// Compose ordered application layers. Missing baselines never masquerade as a complete form.
#[must_use]
pub fn compose(base: &FormDocument, extensions: &[&FormDocument]) -> ComposedForm {
    let mut diagnostics = Vec::new();
    inspect(base, &mut diagnostics);
    if base.base.is_some() {
        issue(&mut diagnostics, base, CompositionIssue::MissingBase, None);
    }
    let mut graph = Graph::default();
    let root = graph.insert_tree(&base.root, &base.source, &HashMap::new());
    let mut attributes = definitions(&base.attributes, &base.source);
    let mut attribute_properties = properties(&base.attribute_properties, &base.source);
    let mut commands = definitions(&base.commands, &base.source);
    let mut parameters = definitions(&base.parameters, &base.source);
    for extension in extensions {
        if let Some(root) = root.as_ref() {
            let mut reachable = HashSet::new();
            graph.reachable(root, &mut reachable);
            graph.nodes.retain(|id, _| reachable.contains(id));
        }
        inspect(extension, &mut diagnostics);
        let Some(snapshot) = extension.base.as_ref() else {
            issue(
                &mut diagnostics,
                extension,
                CompositionIssue::MissingBase,
                None,
            );
            continue;
        };
        let mut bindings = HashMap::new();
        graph.bind(
            &snapshot.root,
            root.as_ref(),
            extension,
            &mut bindings,
            &mut diagnostics,
        );
        graph.apply_tree(&extension.root, &snapshot.root, extension, &bindings);
        patch_definitions(
            &mut attributes,
            &snapshot.attributes,
            &extension.attributes,
            &extension.source,
        );
        patch_properties(
            &mut attribute_properties,
            &snapshot.attribute_properties,
            &extension.attribute_properties,
            &extension.source,
        );
        patch_definitions(
            &mut commands,
            &snapshot.commands,
            &extension.commands,
            &extension.source,
        );
        patch_definitions(
            &mut parameters,
            &snapshot.parameters,
            &extension.parameters,
            &extension.source,
        );
    }
    let fallback = effective(&base.root, &base.source);
    let root = root
        .and_then(|root| graph.expand(&root, &mut HashSet::new(), &mut diagnostics))
        .unwrap_or(fallback);
    ComposedForm {
        root,
        attributes,
        attribute_properties,
        commands,
        parameters,
        diagnostics,
    }
}

fn inspect(document: &FormDocument, diagnostics: &mut Vec<FormCompositionDiagnostic>) {
    if document.has_issues()
        || document
            .base
            .as_ref()
            .is_some_and(|base| !base.diagnostics.is_empty())
    {
        issue(
            diagnostics,
            document,
            CompositionIssue::InvalidDocument,
            None,
        );
    }
}

fn issue(
    diagnostics: &mut Vec<FormCompositionDiagnostic>,
    document: &FormDocument,
    issue: CompositionIssue,
    element: Option<ElementKey>,
) {
    diagnostics.push(FormCompositionDiagnostic {
        issue,
        source: document.source.clone(),
        element,
    });
}

fn origin(source: &str, range: Range<usize>) -> PropertySource {
    PropertySource {
        source: source.into(),
        range,
    }
}

fn properties(values: &[LocatedProperty], source: &str) -> Vec<SourcedProperty> {
    values
        .iter()
        .map(|value| SourcedProperty {
            origin: origin(source, value.range.clone()),
            property: value.property.clone(),
        })
        .collect()
}

fn effective(value: &FormElement, source: &str) -> EffectiveElement {
    EffectiveElement {
        identity: value.identity.clone().unwrap_or_else(|| ElementIdentity {
            source: source.into(),
            part: super::FormPart::Current,
            key: ElementKey::Root,
        }),
        origin: origin(source, value.range.clone()),
        kind: value.kind,
        xml_name: value.xml_name.clone(),
        name: value.name.clone(),
        qualifiers: value
            .qualifiers
            .iter()
            .map(|value| SourcedQualifier::new(value, source))
            .collect(),
        properties: properties(&value.properties, source),
        children: Vec::new(),
        auxiliary: Vec::new(),
    }
}

fn definitions(values: &[FormDefinition], source: &str) -> Vec<EffectiveDefinition> {
    values
        .iter()
        .map(|value| EffectiveDefinition {
            origin: origin(source, value.range.clone()),
            xml_name: value.xml_name.clone(),
            name: value.name.clone(),
            qualifiers: value.qualifiers.clone(),
            properties: properties(&value.properties, source),
        })
        .collect()
}

/// Absence in both snapshots means the extension did not borrow this property.
fn patch_properties(
    target: &mut Vec<SourcedProperty>,
    baseline: &[LocatedProperty],
    modified: &[LocatedProperty],
    source: &str,
) {
    for before in baseline {
        if !modified
            .iter()
            .any(|after| after.property.key == before.property.key)
        {
            target.retain(|value| value.property.key != before.property.key);
        }
    }
    for after in modified {
        if baseline
            .iter()
            .any(|before| before.property == after.property)
        {
            continue;
        }
        let replacement = SourcedProperty {
            origin: origin(source, after.range.clone()),
            property: after.property.clone(),
        };
        if let Some(value) = target
            .iter_mut()
            .find(|value| value.property.key == after.property.key)
        {
            *value = replacement;
        } else {
            target.push(replacement);
        }
    }
}

fn patch_qualifiers(
    target: &mut Vec<SourcedQualifier>,
    baseline: &[LocatedQualifier],
    modified: &[LocatedQualifier],
    source: &str,
) {
    target.retain(|value| {
        !baseline.iter().any(|before| before.key == value.key)
            || modified.iter().any(|after| after.key == value.key)
    });
    for after in modified {
        if baseline
            .iter()
            .any(|before| before.key == after.key && before.text == after.text)
        {
            continue;
        }
        let replacement = SourcedQualifier::new(after, source);
        if let Some(value) = target.iter_mut().find(|value| value.key == after.key) {
            *value = replacement;
        } else {
            target.push(replacement);
        }
    }
}

fn patch_definitions(
    target: &mut Vec<EffectiveDefinition>,
    baseline: &[FormDefinition],
    modified: &[FormDefinition],
    source: &str,
) {
    target.retain(|value| {
        !baseline.iter().any(|before| {
            before.name == value.name
                && before.xml_name == value.xml_name
                && !modified
                    .iter()
                    .any(|after| after.name == before.name && after.xml_name == before.xml_name)
        })
    });
    for after in modified {
        let before = baseline
            .iter()
            .find(|before| before.name == after.name && before.xml_name == after.xml_name);
        if let (Some(before), Some(value)) = (
            before,
            target
                .iter_mut()
                .find(|value| value.name == after.name && value.xml_name == after.xml_name),
        ) {
            patch_properties(
                &mut value.properties,
                &before.properties,
                &after.properties,
                source,
            );
            if before.qualifiers != after.qualifiers {
                value.qualifiers.clone_from(&after.qualifiers);
            }
        } else if let Some(value) = definitions(std::slice::from_ref(after), source).pop() {
            target.push(value);
        }
    }
}

#[derive(Default)]
struct Graph {
    nodes: HashMap<ElementIdentity, Entry>,
}
struct Entry {
    value: EffectiveElement,
    children: Vec<ElementIdentity>,
    auxiliary: Vec<ElementIdentity>,
}

impl Graph {
    fn reachable(&self, id: &ElementIdentity, output: &mut HashSet<ElementIdentity>) {
        if output.insert(id.clone())
            && let Some(entry) = self.nodes.get(id)
        {
            for child in entry.children.iter().chain(&entry.auxiliary) {
                self.reachable(child, output);
            }
        }
    }
    fn insert_tree(
        &mut self,
        value: &FormElement,
        source: &str,
        bindings: &HashMap<ElementKey, ElementIdentity>,
    ) -> Option<ElementIdentity> {
        let raw = value.identity.as_ref()?;
        let id = bindings
            .get(&raw.key)
            .cloned()
            .unwrap_or_else(|| raw.clone());
        if !self.nodes.contains_key(&id) {
            let mut element = effective(value, source);
            element.identity = id.clone();
            self.nodes.insert(
                id.clone(),
                Entry {
                    value: element,
                    children: Vec::new(),
                    auxiliary: Vec::new(),
                },
            );
            let children = value
                .children
                .iter()
                .filter_map(|child| self.insert_tree(child, source, bindings))
                .collect();
            let auxiliary = value
                .auxiliary
                .iter()
                .filter_map(|child| self.insert_tree(child, source, bindings))
                .collect();
            if let Some(entry) = self.nodes.get_mut(&id) {
                entry.children = children;
                entry.auxiliary = auxiliary;
            }
        }
        Some(id)
    }

    fn bind(
        &self,
        baseline: &FormElement,
        root: Option<&ElementIdentity>,
        document: &FormDocument,
        bindings: &mut HashMap<ElementKey, ElementIdentity>,
        diagnostics: &mut Vec<FormCompositionDiagnostic>,
    ) {
        if let Some(raw) = baseline.identity.as_ref() {
            let candidates: Vec<_> = if raw.key == ElementKey::Root {
                root.into_iter().collect()
            } else {
                self.nodes
                    .iter()
                    .filter(|(_, entry)| {
                        entry.value.name == baseline.name
                            && entry.value.kind == baseline.kind
                            && (baseline.name.as_ref().is_some_and(|name| !name.is_empty())
                                || entry.value.identity.key == raw.key)
                    })
                    .map(|(id, _)| id)
                    .collect()
            };
            if candidates.len() == 1 {
                bindings.insert(raw.key, candidates[0].clone());
            } else {
                issue(
                    diagnostics,
                    document,
                    if candidates.is_empty() {
                        CompositionIssue::MissingBorrowedElement
                    } else {
                        CompositionIssue::DuplicateName
                    },
                    Some(raw.key),
                );
            }
        }
        for child in baseline.children.iter().chain(&baseline.auxiliary) {
            self.bind(child, root, document, bindings, diagnostics);
        }
    }

    fn apply_tree(
        &mut self,
        modified: &FormElement,
        baseline: &FormElement,
        document: &FormDocument,
        bindings: &HashMap<ElementKey, ElementIdentity>,
    ) {
        let mut originals = HashMap::new();
        flatten(baseline, &mut originals);
        self.patch_node(modified, &originals, document, bindings);
    }

    fn patch_node(
        &mut self,
        modified: &FormElement,
        originals: &HashMap<ElementKey, &FormElement>,
        document: &FormDocument,
        bindings: &HashMap<ElementKey, ElementIdentity>,
    ) {
        let Some(raw) = modified.identity.as_ref() else {
            return;
        };
        let before = originals.get(&raw.key).copied();
        // An unresolved borrowed element cannot be silently introduced as a new extension element.
        if before.is_some() && !bindings.contains_key(&raw.key) {
            return;
        }
        let Some(id) = self.insert_tree(modified, &document.source, bindings) else {
            return;
        };
        for child in modified.children.iter().chain(&modified.auxiliary) {
            self.patch_node(child, originals, document, bindings);
        }
        let Some(before) = before else {
            return;
        };
        let old_children = mapped(&before.children, bindings);
        let new_children = current_ids(&modified.children, bindings);
        let old_auxiliary = mapped(&before.auxiliary, bindings);
        let new_auxiliary = current_ids(&modified.auxiliary, bindings);
        // Moving a borrowed node detaches it from its previous parent, including cross-group moves.
        for moved in new_children
            .iter()
            .filter(|child| !old_children.contains(child))
            .chain(
                new_auxiliary
                    .iter()
                    .filter(|child| !old_auxiliary.contains(child)),
            )
        {
            for (parent, entry) in &mut self.nodes {
                if parent != &id {
                    entry.children.retain(|child| child != moved);
                    entry.auxiliary.retain(|child| child != moved);
                }
            }
        }
        if let Some(entry) = self.nodes.get_mut(&id) {
            patch_properties(
                &mut entry.value.properties,
                &before.properties,
                &modified.properties,
                &document.source,
            );
            if before.name != modified.name {
                entry.value.name.clone_from(&modified.name);
            }
            patch_qualifiers(
                &mut entry.value.qualifiers,
                &before.qualifiers,
                &modified.qualifiers,
                &document.source,
            );
            patch_order(&mut entry.children, &old_children, &new_children);
            patch_order(&mut entry.auxiliary, &old_auxiliary, &new_auxiliary);
        }
    }

    fn expand(
        &mut self,
        id: &ElementIdentity,
        visited: &mut HashSet<ElementIdentity>,
        diagnostics: &mut Vec<FormCompositionDiagnostic>,
    ) -> Option<EffectiveElement> {
        if !visited.insert(id.clone()) {
            diagnostics.push(FormCompositionDiagnostic {
                issue: CompositionIssue::InvalidHierarchy,
                source: id.source.clone(),
                element: Some(id.key),
            });
            return None;
        }
        let entry = self.nodes.remove(id)?;
        let mut value = entry.value;
        value.children = entry
            .children
            .iter()
            .filter_map(|child| self.expand(child, visited, diagnostics))
            .collect();
        value.auxiliary = entry
            .auxiliary
            .iter()
            .filter_map(|child| self.expand(child, visited, diagnostics))
            .collect();
        Some(value)
    }
}

fn flatten<'a>(value: &'a FormElement, output: &mut HashMap<ElementKey, &'a FormElement>) {
    if let Some(id) = value.identity.as_ref() {
        output.insert(id.key, value);
    }
    for child in value.children.iter().chain(&value.auxiliary) {
        flatten(child, output);
    }
}
fn mapped(
    values: &[FormElement],
    bindings: &HashMap<ElementKey, ElementIdentity>,
) -> Vec<ElementIdentity> {
    values
        .iter()
        .filter_map(|value| {
            value
                .identity
                .as_ref()
                .and_then(|id| bindings.get(&id.key))
                .cloned()
        })
        .collect()
}
fn current_ids(
    values: &[FormElement],
    bindings: &HashMap<ElementKey, ElementIdentity>,
) -> Vec<ElementIdentity> {
    values
        .iter()
        .filter_map(|value| value.identity.as_ref())
        .map(|id| bindings.get(&id.key).unwrap_or(id).clone())
        .collect()
}

/// Preserve elements introduced by earlier extensions and apply only changed ordering constraints.
fn patch_order(
    target: &mut Vec<ElementIdentity>,
    baseline: &[ElementIdentity],
    modified: &[ElementIdentity],
) {
    if baseline == modified {
        return;
    }
    target.retain(|id| !baseline.contains(id) || modified.contains(id));
    let mut cursor = modified
        .iter()
        .find_map(|id| target.iter().position(|value| value == id))
        .unwrap_or(target.len());
    for id in modified {
        if let Some(index) = target.iter().position(|value| value == id) {
            if index >= cursor {
                cursor = index + 1;
                continue;
            }
            target.remove(index);
            cursor = cursor.saturating_sub(1);
        }
        target.insert(cursor, id.clone());
        cursor += 1;
    }
}

#[cfg(test)]
mod tests;
