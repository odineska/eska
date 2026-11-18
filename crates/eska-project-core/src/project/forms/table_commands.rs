//! Design-time additional menus for tables. Commands carry no executable behavior.

use std::collections::HashSet;

use super::{
    AutoCommandItem, EffectiveElement, ElementKind, FormPicture, FormResource, PresentationText,
    PropertySource,
    appearance::text,
    commands::{
        effective_metadata, excludes, metadata_caption, metadata_text, table_command_reference,
    },
    presentation::metadata_property,
};
use crate::project::{
    metadata_model::{MetadataKind, MetadataValue, ObjectId},
    metadata_parser::ParsedObject,
};

pub(super) fn ordinary_sections(table: &EffectiveElement) -> Vec<Vec<AutoCommandItem>> {
    let mut primary = Vec::new();
    if allows_row_set_change(table) {
        primary.extend(["Add", "Copy", "Change", "Delete"].map(ordinary_item));
    } else {
        primary.push(ordinary_item("Change"));
    }
    primary.push(ordinary_item("EndEdit"));
    primary.push(item("FindByCurrentValue"));
    primary.extend([find_item(table), ordinary_item("CancelSearch")]);

    let mut result = vec![primary];
    let mut output = Vec::new();
    if allows_row_order_change(table) {
        result.push(["MoveUp", "MoveDown"].map(ordinary_item).into());
        output.extend(["SortListAsc", "SortListDesc"].map(ordinary_item));
    }
    output.push(ordinary_item("OutputList"));
    result.push(output);
    retain_available(table, result)
}

/// A manual bar keeps the picture and shortcut of its explicitly authored standard commands.
pub(super) fn ordinary_authored_commands(table: &EffectiveElement) -> Vec<Vec<AutoCommandItem>> {
    fn collect<'a>(elements: &'a [EffectiveElement], table: &str, commands: &mut HashSet<&'a str>) {
        for element in elements {
            if let Some((owner, name)) = element
                .scalar("CommandName")
                .and_then(table_command_reference)
                && owner.to_lowercase() == table
            {
                commands.insert(name);
            }
            if element.kind == Some(ElementKind::ButtonGroup) {
                collect(&element.children, table, commands);
            }
        }
    }
    let Some(name) = table.name.as_deref() else {
        return Vec::new();
    };
    let Some(bar) = table
        .auxiliary
        .iter()
        .find(|item| item.kind == Some(ElementKind::AutoCommandBar))
    else {
        return Vec::new();
    };
    let mut authored = HashSet::new();
    collect(&bar.children, &name.to_lowercase(), &mut authored);
    let mut sections = ordinary_sections(table);
    for group in &mut sections {
        group.retain(|item| {
            item.key
                .strip_prefix("Table.StandardCommand.")
                .is_some_and(|key| authored.contains(key))
        });
    }
    sections.retain(|group| !group.is_empty());
    sections
}

pub(super) fn allows_row_set_change(table: &EffectiveElement) -> bool {
    !matches!(table.scalar("ChangeRowSet"), Some("false" | "0"))
}

pub(super) fn allows_row_order_change(table: &EffectiveElement) -> bool {
    !matches!(table.scalar("ChangeRowOrder"), Some("false" | "0"))
}

fn ordinary_item(name: &str) -> AutoCommandItem {
    let mut command = item(name);
    command.shortcut = match name {
        "Add" => Some("Ins"),
        "Delete" => Some("Del"),
        "EndEdit" => Some("Shift+F2"),
        "MoveUp" => Some("Ctrl+Shift+Up"),
        "MoveDown" => Some("Ctrl+Shift+Down"),
        _ => command.shortcut,
    };
    command.picture = Some(picture(name));
    if name == "EndEdit" {
        command.caption = caption("TableEndEdit");
    }
    command
}

pub(super) fn dynamic_sections(
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
    retain_available(table, result)
}

fn retain_available(
    table: &EffectiveElement,
    mut result: Vec<Vec<AutoCommandItem>>,
) -> Vec<Vec<AutoCommandItem>> {
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
