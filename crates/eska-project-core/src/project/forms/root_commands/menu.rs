//! Standard actions in the Configurator's additional menu for an object form.

use std::collections::HashSet;

use super::{ObjectCommands, metadata_text, origin};
use crate::project::{
    forms::{
        AutoCommandItem, FormResource,
        commands::{effective_metadata, metadata_caption},
        presentation::{literal, metadata_property},
        resources::{configuration_objects, has_readable_reference},
    },
    metadata_model::MetadataKind,
    metadata_parser::ParsedObject,
};

pub(super) fn sections(
    commands: ObjectCommands,
    owner: Option<(&FormResource, &ParsedObject)>,
    resources: &[FormResource],
) -> Vec<Vec<AutoCommandItem>> {
    let mut primary = items(&[
        commands.close_action(),
        "Write",
        "Reread",
        "Copy",
        "SetDeletionMark",
        "Delete",
    ]);
    let mut navigation = vec![show_in_list(owner, resources)];
    if owner.and_then(|(_, object)| metadata_text(object, "DataHistory", resources)) == Some("Use")
    {
        navigation.push(item("ChangeHistory"));
    }
    if commands.posting() {
        vec![primary, items(&["Post", "UndoPosting"]), navigation]
    } else {
        primary.extend(navigation);
        vec![primary]
    }
}

fn items(names: &[&str]) -> Vec<AutoCommandItem> {
    names.iter().map(|name| item(name)).collect()
}

fn item(name: &str) -> AutoCommandItem {
    AutoCommandItem::standard("Form", name, false)
}

fn show_in_list(
    owner: Option<(&FormResource, &ParsedObject)>,
    resources: &[FormResource],
) -> AutoCommandItem {
    let mut command = item("ShowInList");
    if let Some((resource, object)) = owner
        && object.metadata.kind() == MetadataKind::Document
    {
        let reference = format!("Document.{}", object.metadata.name());
        let journals = journals(&reference, resources);
        if !journals.is_empty() {
            command.items.push(list_item(resource, object, resources));
            command.items.extend(journals);
        }
    }
    command
}

fn journals(reference: &str, resources: &[FormResource]) -> Vec<AutoCommandItem> {
    let mut seen = HashSet::new();
    resources
        .iter()
        .flat_map(|resource| configuration_objects(&resource.descriptor))
        .filter(|candidate| {
            candidate.kind == MetadataKind::DocumentJournal && seen.insert(&candidate.id)
        })
        .filter_map(|candidate| {
            let (resource, journal) = effective_metadata(&candidate.id, resources)?;
            let property = metadata_property(journal, "RegisteredDocuments", resources)?;
            has_readable_reference(&property.value, reference)
                .then(|| list_item(resource, journal, resources))
        })
        .collect()
}

fn list_item(
    resource: &FormResource,
    object: &ParsedObject,
    resources: &[FormResource],
) -> AutoCommandItem {
    let mut command = AutoCommandItem::standard(object.metadata.id().as_str(), "ShowInList", false);
    command.caption = metadata_property(object, "ListPresentation", resources)
        .and_then(|property| literal(&property.value))
        .unwrap_or_else(|| metadata_caption(object, resources));
    command.origin = Some(origin(resource, object));
    command
}
