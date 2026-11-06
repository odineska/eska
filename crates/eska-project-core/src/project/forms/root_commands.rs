//! Compose the main form bar from the effective object and its command interface.

use std::collections::{HashMap, HashSet};

use super::{
    AppearanceDiagnostic, AutoCommandItem, ComposedForm, FORM_NAMESPACE, FormResource,
    PropertySource,
    appearance::text,
    commands::{ResolvedCommand, autofill, effective_metadata},
    presentation::{definition_type, field_type, metadata_property},
    references::{self, CONFIG, command_group_id, metadata_command_id},
};
use crate::project::{
    metadata_model::{MetadataKind, MetadataProperty, MetadataValue, ObjectId, PropertyKey},
    metadata_parser::ParsedObject,
};

pub(super) fn automatic(
    form: &ComposedForm,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> Vec<AutoCommandItem> {
    if !autofill(&form.root) {
        return Vec::new();
    }
    let interface = interface_items(form);
    let mut bar = CommandBar::default();
    if let Some(owner_type) = object_type(form) {
        let owner = owner_id(&owner_type).and_then(|id| effective_metadata(&id, resources));
        bar.buttons = standard_commands(&owner_type, owner.map(|(_, object)| object), resources);
        if let Some((_, owner)) = owner {
            add_owner_commands(
                &mut bar,
                owner,
                &owner_type,
                &interface,
                resources,
                diagnostics,
            );
        }
    }
    add_explicit_commands(&mut bar, form, &interface, resources, diagnostics);
    bar.buttons
        .extend(bar.menus.into_iter().filter(|menu| !menu.items.is_empty()));
    bar.buttons
}

pub(super) fn object_type(form: &ComposedForm) -> Option<PropertyKey> {
    definition_type(form.main_attribute()?)
        .types
        .into_iter()
        .find(|kind| kind.namespace.as_deref() == Some(CONFIG) && kind.name.contains("Object."))
}

pub(super) fn owner_id(owner_type: &PropertyKey) -> Option<ObjectId> {
    let (kind, name) = owner_type.name.split_once('.')?;
    let kind = references::type_kind(kind)?;
    Some(ObjectId::from_parts(None, kind.as_str(), name))
}

fn standard_commands(
    owner_type: &PropertyKey,
    owner: Option<&ParsedObject>,
    resources: &[FormResource],
) -> Vec<AutoCommandItem> {
    let names: &[&str] = if owner_type.name.starts_with("DocumentObject.")
        && owner.and_then(|owner| metadata_text(owner, "Posting", resources)) != Some("Deny")
    {
        &["PostAndClose", "Write", "Post"]
    } else if owner_type.name.starts_with("CatalogObject.")
        || owner_type.name.starts_with("DocumentObject.")
    {
        &["WriteAndClose", "Write"]
    } else {
        &[]
    };
    names
        .iter()
        .enumerate()
        .map(|(index, name)| AutoCommandItem::standard("Form", name, index == 0))
        .collect()
}

#[derive(Default)]
struct CommandBar {
    buttons: Vec<AutoCommandItem>,
    menus: Vec<AutoCommandItem>,
}

impl CommandBar {
    fn items(
        &mut self,
        group: &str,
        resources: &[FormResource],
        diagnostics: &mut Vec<AppearanceDiagnostic>,
    ) -> Option<&mut Vec<AutoCommandItem>> {
        if group == "FormCommandBarImportant" {
            return Some(&mut self.buttons);
        }
        let id = command_group_id(group)?;
        let (resource, group) = effective_metadata(&id, resources)?;
        if metadata_text(group, "Category", resources) != Some("FormCommandBar") {
            return None;
        }
        let key = id.to_string();
        let index = if let Some(index) = self.menus.iter().position(|item| item.key == key) {
            index
        } else {
            self.menus.push(ResolvedCommand::Metadata(group).item(
                key,
                Some(origin(resource, group)),
                resources,
                diagnostics,
            )?);
            self.menus.len() - 1
        };
        Some(&mut self.menus[index].items)
    }
}

fn add_owner_commands(
    bar: &mut CommandBar,
    owner: &ParsedObject,
    owner_type: &PropertyKey,
    interface: &[InterfaceItem<'_>],
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) {
    let placements = interface
        .iter()
        .filter_map(|item| metadata_command_id(item.reference).map(|id| (id, item.group)))
        .collect::<HashMap<_, _>>();
    let parameter_type = owner_type.name.replacen("Object.", "Ref.", 1);
    let mut seen = HashSet::new();
    for object in resources
        .iter()
        .flat_map(|resource| &resource.descriptor.objects)
    {
        let id = object.metadata.id();
        if object.metadata.kind() != MetadataKind::Command
            || object.metadata.parent() != Some(owner.metadata.id())
            || !seen.insert(id)
            || !accepts_parameter(object, &parameter_type, resources)
        {
            continue;
        }
        let group = placements
            .get(id)
            .copied()
            .or_else(|| metadata_text(object, "Group", resources));
        let Some(items) = group.and_then(|group| bar.items(group, resources, diagnostics)) else {
            continue;
        };
        let Some((resource, command)) = effective_metadata(id, resources) else {
            continue;
        };
        items.extend(ResolvedCommand::Metadata(command).item(
            id.to_string(),
            Some(origin(resource, command)),
            resources,
            diagnostics,
        ));
    }
}

fn accepts_parameter(command: &ParsedObject, parameter: &str, resources: &[FormResource]) -> bool {
    let types = metadata_property(command, "CommandParameterType", resources)
        .map(|value| field_type(&value.value))
        .unwrap_or_default()
        .types;
    types.is_empty()
        || types
            .iter()
            .any(|value| value.namespace.as_deref() == Some(CONFIG) && value.name == parameter)
}

fn add_explicit_commands(
    bar: &mut CommandBar,
    form: &ComposedForm,
    interface: &[InterfaceItem<'_>],
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) {
    for item in interface.iter().filter(|item| item.added) {
        let Some(command) = ResolvedCommand::resolve(item.reference, form, resources) else {
            continue;
        };
        let Some(items) = bar.items(item.group, resources, diagnostics) else {
            continue;
        };
        let (key, origin) = match &command {
            ResolvedCommand::Local(command) => {
                (item.reference.into(), Some(command.origin.clone()))
            }
            ResolvedCommand::Metadata(command) => {
                let id = command.metadata.id();
                (
                    id.to_string(),
                    effective_metadata(id, resources)
                        .map(|(resource, command)| origin(resource, command)),
                )
            }
            ResolvedCommand::Standard(_) => (item.reference.into(), None),
        };
        if !items.iter().any(|existing| existing.key == key) {
            items.extend(command.item(key, origin, resources, diagnostics));
        }
    }
}

struct InterfaceItem<'a> {
    reference: &'a str,
    group: &'a str,
    added: bool,
}

fn interface_items(form: &ComposedForm) -> Vec<InterfaceItem<'_>> {
    let Some(interface) = form.root.property("CommandInterface") else {
        return Vec::new();
    };
    let Some(MetadataValue::Record(items)) = field(&interface.property.value, "CommandBar") else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|item| named(item, "Item"))
        .filter_map(|item| {
            let reference = text(field(&item.value, "Command")?)?;
            let group = text(field(&item.value, "CommandGroup")?)?;
            let kind = text(field(&item.value, "Type")?)?;
            // DefaultVisible applies at runtime. The Configurator preview shows these commands.
            matches!(kind, "Auto" | "Added").then_some(InterfaceItem {
                reference,
                group,
                added: kind == "Added",
            })
        })
        .collect()
}

fn field<'a>(value: &'a MetadataValue, name: &str) -> Option<&'a MetadataValue> {
    let MetadataValue::Record(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find(|field| named(field, name))
        .map(|field| &field.value)
}

fn named(field: &MetadataProperty, name: &str) -> bool {
    field.key.namespace.as_deref() == Some(FORM_NAMESPACE) && field.key.name == name
}

fn metadata_text<'a>(
    object: &ParsedObject,
    name: &str,
    resources: &'a [FormResource],
) -> Option<&'a str> {
    metadata_property(object, name, resources).and_then(|value| text(&value.value))
}

fn origin(resource: &FormResource, object: &ParsedObject) -> PropertySource {
    PropertySource {
        source: resource.source.clone(),
        range: object.range.clone(),
    }
}
