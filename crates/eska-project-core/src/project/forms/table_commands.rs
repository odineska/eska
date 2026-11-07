//! Design-time additional menus for dynamic lists. Commands carry no executable behavior.

use super::{
    AutoCommandItem, EffectiveElement, FormPicture, FormResource, PresentationText, PropertySource,
    appearance::text,
    commands::{effective_metadata, excludes, metadata_caption, metadata_text},
    presentation::metadata_property,
};
use crate::project::{
    metadata_model::{MetadataKind, MetadataValue, ObjectId},
    metadata_parser::ParsedObject,
};

pub(super) fn sections(
    table: &EffectiveElement,
    owner: Option<&ParsedObject>,
    resources: &[FormResource],
) -> Vec<Vec<AutoCommandItem>> {
    let kind = owner.map(|object| object.metadata.kind());
    let object = matches!(
        kind,
        Some(MetadataKind::Catalog | MetadataKind::Document | MetadataKind::DocumentJournal)
    );
    let register = kind == Some(MetadataKind::InformationRegister);
    let hierarchical = owner.is_some_and(|owner| {
        kind == Some(MetadataKind::Catalog)
            && metadata_text(owner, "Hierarchical", resources) == Some("true")
    });
    let mut primary = Vec::new();
    if object || register {
        let mut create = item("Create");
        if kind == Some(MetadataKind::DocumentJournal)
            && let Some(owner) = owner
        {
            create.shortcut = None;
            create.items = creation_requests(owner, resources)
                .iter()
                .filter_map(|id| creation_item(id, resources))
                .collect();
        }
        primary.push(create);
        if hierarchical
            && owner.and_then(|owner| metadata_text(owner, "HierarchyType", resources))
                != Some("HierarchyItems")
        {
            primary.push(item("CreateFolder"));
        }
        primary.extend(items(&["Copy", "Change"]));
        if object {
            primary.push(item("SetDeletionMark"));
        }
        let mut delete = item("Delete");
        if register {
            delete.shortcut = Some("Del");
            delete.picture = Some(picture("DeleteListItem"));
        }
        primary.push(delete);
    }
    primary.push(item("Refresh"));
    if hierarchical {
        let mut view = item("ViewMode");
        view.items = items(&["HierarchicalList", "List", "Tree"]);
        primary.push(view);
        primary.extend(items(&["MoveItem", "LevelDown", "LevelUp"]));
    }
    let mut result = vec![primary];
    let period = matches!(
        kind,
        Some(MetadataKind::Document | MetadataKind::DocumentJournal)
    ) || register
        && owner
            .and_then(|owner| metadata_text(owner, "InformationRegisterPeriodicity", resources))
            .is_some_and(|periodicity| periodicity != "Nonperiodical");
    if period {
        result.push(items(&["SetPeriod"]));
    }
    if kind == Some(MetadataKind::DocumentJournal)
        || kind == Some(MetadataKind::Document)
            && owner.and_then(|owner| metadata_text(owner, "Posting", resources)) != Some("Deny")
    {
        result.push(items(&["Post", "UndoPosting"]));
    }
    let search = search_actions(table, object || register);
    if period {
        result.push(search);
    } else {
        result[0].extend(search);
    }
    result.push(items(&[
        "ListSettings",
        "SelectSettings",
        "SaveSettings",
        "RestoreDefaultSettings",
    ]));
    let mut output = items(&["OutputList"]);
    if table.scalar("SearchControlLocation") != Some("None") {
        output.push(search_control(table));
    }
    result.push(output);
    for group in &mut result {
        group.retain_mut(|item| {
            if excluded(table, item) {
                return false;
            }
            let submenu = !item.items.is_empty();
            item.items.retain(|child| !excluded(table, child));
            !submenu || !item.items.is_empty()
        });
    }
    result.retain(|group| !group.is_empty());
    result
}

fn search_actions(table: &EffectiveElement, records: bool) -> Vec<AutoCommandItem> {
    let mut search = vec![
        item("FindByCurrentValue"),
        find_item(table),
        item("CancelSearch"),
    ];
    if records {
        // The Configurator's list preview includes history even with DataHistory=DontUse.
        search.push(item("ChangeHistory"));
        if !matches!(
            table.scalar("AllowGettingCurrentRowURL"),
            Some("false" | "0")
        ) {
            search.push(item("GetCurrentRowURL"));
        }
    }
    search
}

fn find_item(table: &EffectiveElement) -> AutoCommandItem {
    let mut find = item("Find");
    if table.scalar("SearchStringLocation") != Some("None") {
        find.caption = caption("AdvancedSearch");
        find.shortcut = Some("Alt+F");
    }
    find
}

fn search_control(table: &EffectiveElement) -> AutoCommandItem {
    let mut control = item("SearchControl");
    let mut history = item("SearchHistory");
    history.enabled = Some(false);
    history.begin_group = Some(true);
    let mut cancel = item("CancelSearch");
    cancel.begin_group = Some(true);
    control.items = vec![
        item("FindByCurrentValue"),
        find_item(table),
        history,
        cancel,
    ];
    control
}

pub(super) fn creation_requests(owner: &ParsedObject, resources: &[FormResource]) -> Vec<ObjectId> {
    if owner.metadata.kind() != MetadataKind::DocumentJournal {
        return Vec::new();
    }
    let Some(MetadataValue::Record(fields)) =
        metadata_property(owner, "RegisteredDocuments", resources).map(|property| &property.value)
    else {
        return Vec::new();
    };
    fields
        .iter()
        .filter(|field| {
            field.key.namespace.as_deref() == Some("http://v8.1c.ru/8.3/xcf/readable")
                && field.key.name == "Item"
        })
        .filter_map(|field| text(&field.value)?.trim().strip_prefix("Document."))
        .map(|name| ObjectId::from_parts(None, MetadataKind::Document.as_str(), name))
        .collect()
}

fn creation_item(id: &ObjectId, resources: &[FormResource]) -> Option<AutoCommandItem> {
    let (resource, owner) = effective_metadata(id, resources)?;
    let mut command = AutoCommandItem::standard(id.as_str(), "Create", false);
    command.caption = metadata_caption(owner, resources);
    command.order_by_caption = Some(true);
    command.origin = Some(PropertySource {
        source: resource.source.clone(),
        range: owner.range.clone(),
    });
    Some(command)
}

fn excluded(table: &EffectiveElement, item: &AutoCommandItem) -> bool {
    item.key
        .strip_prefix("Table.StandardCommand.")
        .is_some_and(|name| excludes(table, name))
}

fn items(names: &[&str]) -> Vec<AutoCommandItem> {
    names.iter().map(|name| item(name)).collect()
}

fn caption(name: &str) -> PresentationText {
    PresentationText::Standard {
        key: format!("command.{name}"),
    }
}

fn picture(name: &str) -> FormPicture {
    FormPicture::Standard { name: name.into() }
}

fn item(name: &str) -> AutoCommandItem {
    let mut command = AutoCommandItem::standard("Table", name, false);
    command.shortcut = match name {
        "Create" => Some("Ins"),
        "CreateFolder" => Some("Ctrl+F9"),
        "Copy" => Some("F9"),
        "Change" => Some("F2"),
        "SetDeletionMark" => Some("Del"),
        "Delete" => Some("Shift+Del"),
        "Refresh" => Some("F5"),
        "MoveItem" => Some("Ctrl+Shift+M"),
        "LevelDown" => Some("Ctrl+Down"),
        "LevelUp" => Some("Ctrl+Up"),
        "FindByCurrentValue" => Some("Ctrl+Alt+F"),
        "Find" => Some("Ctrl+F"),
        "CancelSearch" => Some("Ctrl+Q"),
        _ => None,
    };
    let image = match name {
        "Copy" => "CloneListItem",
        "SetDeletionMark" => "MarkObjectForDeletion",
        "Delete" => "DeleteListItemDirectly",
        "FindByCurrentValue" | "SearchHistory" | "RestoreDefaultSettings" => "",
        _ => name,
    };
    command.picture = (!image.is_empty()).then(|| picture(image));
    command.caption = match name {
        "SetDeletionMark" => caption("ListToggleDeletionMark"),
        "FindByCurrentValue" => caption("SearchByCurrentValue"),
        "UndoPosting" => caption("CancelPosting"),
        _ => command.caption,
    };
    command
}

#[cfg(test)]
mod tests;
