//! Compose the main form bar from the effective object and its command interface.

mod menu;

use std::collections::{HashMap, HashSet};

use super::{
    AppearanceDiagnostic, AutoCommandItem, ComposedForm, EffectiveElement, ElementKind,
    FORM_NAMESPACE, FormResource, PropertySource,
    appearance::text,
    command_types,
    commands::{ResolvedCommand, autofill, effective_metadata, metadata_text},
    presentation::definition_type,
    references::{self, CONFIG, command_group_id, metadata_command_id},
    resources::configuration_objects,
};
use crate::project::{
    metadata_model::{MetadataKind, MetadataProperty, MetadataValue, ObjectId, PropertyKey},
    metadata_parser::ParsedObject,
};

#[derive(Default)]
pub(super) struct RootCommands {
    pub inline: Vec<AutoCommandItem>,
    pub object_menu: Vec<Vec<AutoCommandItem>>,
}

pub(super) fn project(
    form: &ComposedForm,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> RootCommands {
    if !autofill(&form.root) {
        return RootCommands::default();
    }
    let interface = interface_items(form);
    let authored = authored_commands(&form.root);
    let owner_type = object_type(form);
    let owner = owner_type.as_ref().and_then(owner_id);
    let RootCommands {
        inline,
        object_menu,
    } = owner_type
        .as_ref()
        .map(|kind| standard_commands(kind, owner.as_ref(), resources))
        .unwrap_or_default();
    let mut bar = CommandBar {
        buttons: inline,
        ..CommandBar::default()
    };
    if let Some(owner_type) = owner_type.as_ref() {
        add_based_on(&mut bar, owner_type, &interface, resources, diagnostics);
        add_metadata_commands(
            &mut bar,
            owner.as_ref(),
            &command_types::reference_type(owner_type),
            &interface,
            &authored,
            resources,
            diagnostics,
        );
    }
    add_explicit_commands(&mut bar, form, &interface, resources, diagnostics);
    if !bar.generate.is_empty() {
        let mut menu = AutoCommandItem::standard("Form", "CreateBasedOn", false);
        menu.items = bar.generate;
        bar.buttons.push(menu);
    }
    bar.buttons
        .extend(bar.menus.into_iter().filter(|menu| !menu.items.is_empty()));
    RootCommands {
        inline: bar.buttons,
        object_menu,
    }
}

/// Standalone Form sources get the standard object commands independently of the main panel.
pub(super) fn project_standalone(
    form: &ComposedForm,
    bar: &EffectiveElement,
    resources: &[FormResource],
) -> RootCommands {
    if matches!(bar.scalar("Autofill"), Some("false" | "0")) {
        return RootCommands::default();
    }
    object_type(form)
        .map(|kind| standard_commands(&kind, owner_id(&kind).as_ref(), resources))
        .unwrap_or_default()
}

fn standard_commands(
    owner_type: &PropertyKey,
    owner: Option<&ObjectId>,
    resources: &[FormResource],
) -> RootCommands {
    let descriptor = owner.and_then(|id| effective_metadata(id, resources));
    let Some(standard) = ObjectCommands::for_type(owner_type, descriptor, resources) else {
        return RootCommands::default();
    };
    RootCommands {
        inline: standard.inline(),
        object_menu: menu::sections(standard, descriptor, resources),
    }
}

fn add_based_on(
    bar: &mut CommandBar,
    owner_type: &PropertyKey,
    interface: &[InterfaceItem<'_>],
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) {
    for command in super::based_on::commands(owner_type, resources) {
        if let Some(placement) = interface
            .iter()
            .rev()
            .find(|item| item.reference == command.key)
        {
            if let Some(items) = bar.items(placement.group, resources, diagnostics) {
                items.push(command);
            }
        } else {
            bar.generate.push(command);
        }
    }
}

pub(super) fn object_type(form: &ComposedForm) -> Option<PropertyKey> {
    definition_type(form.main_attribute()?)
        .types
        .into_iter()
        .find(|kind| kind.namespace.as_deref() == Some(CONFIG) && kind.name.contains("Object."))
}

fn owner_id(owner_type: &PropertyKey) -> Option<ObjectId> {
    let (kind, name) = owner_type.name.split_once('.')?;
    let kind = references::type_kind(kind)?;
    Some(ObjectId::from_parts(None, kind.as_str(), name))
}

#[derive(Clone, Copy)]
enum ObjectCommands {
    Document { posting: bool },
    NonPosting,
}

impl ObjectCommands {
    fn for_type(
        owner_type: &PropertyKey,
        owner: Option<(&FormResource, &ParsedObject)>,
        resources: &[FormResource],
    ) -> Option<Self> {
        match owner_type.name.split_once('.')?.0 {
            "DocumentObject" => Some(Self::Document {
                posting: owner.and_then(|(_, owner)| metadata_text(owner, "Posting", resources))
                    != Some("Deny"),
            }),
            "CatalogObject" | "ChartOfCharacteristicTypesObject" => Some(Self::NonPosting),
            _ => None,
        }
    }

    const fn posting(self) -> bool {
        matches!(self, Self::Document { posting: true })
    }

    const fn close_action(self) -> &'static str {
        if self.posting() {
            "PostAndClose"
        } else {
            "WriteAndClose"
        }
    }

    fn inline(self) -> Vec<AutoCommandItem> {
        let mut items = vec![
            AutoCommandItem::standard("Form", self.close_action(), true),
            AutoCommandItem::standard("Form", "Write", false),
        ];
        if self.posting() {
            items.push(AutoCommandItem::standard("Form", "Post", false));
        }
        items
    }
}

#[derive(Default)]
struct CommandBar {
    buttons: Vec<AutoCommandItem>,
    generate: Vec<AutoCommandItem>,
    menus: Vec<AutoCommandItem>,
}

pub(super) const BAR_GROUP_CATEGORY: &str = "FormCommandBar";

pub(super) enum BarGroup {
    Important,
    Generate,
    Named(ObjectId),
}

impl BarGroup {
    pub(super) fn parse(group: &str) -> Option<Self> {
        match group {
            "FormCommandBarImportant" => Some(Self::Important),
            "FormCommandBarCreateBasedOn" => Some(Self::Generate),
            _ => command_group_id(group).map(Self::Named),
        }
    }
}

impl CommandBar {
    fn items(
        &mut self,
        group: &str,
        resources: &[FormResource],
        diagnostics: &mut Vec<AppearanceDiagnostic>,
    ) -> Option<&mut Vec<AutoCommandItem>> {
        let id = match BarGroup::parse(group)? {
            BarGroup::Important => return Some(&mut self.buttons),
            BarGroup::Generate => return Some(&mut self.generate),
            BarGroup::Named(id) => id,
        };
        let (resource, group) = effective_metadata(&id, resources)?;
        if metadata_text(group, "Category", resources) != Some(BAR_GROUP_CATEGORY) {
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

fn authored_commands(root: &EffectiveElement) -> HashSet<ObjectId> {
    fn collect(element: &EffectiveElement, output: &mut HashSet<ObjectId>) {
        if element.kind == Some(ElementKind::Button)
            && let Some(id) = element.scalar("CommandName").and_then(metadata_command_id)
        {
            output.insert(id);
        }
        for child in &element.children {
            collect(child, output);
        }
    }

    let mut result = HashSet::new();
    if let Some(bar) = root
        .auxiliary
        .iter()
        .find(|node| node.kind == Some(ElementKind::AutoCommandBar))
    {
        collect(bar, &mut result);
    }
    result
}

fn add_metadata_commands(
    bar: &mut CommandBar,
    owner: Option<&ObjectId>,
    parameter: &str,
    interface: &[InterfaceItem<'_>],
    authored: &HashSet<ObjectId>,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) {
    let placements = metadata_placements(interface);
    let objects = || {
        resources
            .iter()
            .flat_map(|resource| &resource.descriptor.objects)
    };
    let loaded_common = objects()
        .filter(|object| object.metadata.kind() == MetadataKind::CommonCommand)
        .map(|object| object.metadata.id())
        .collect::<HashSet<_>>();
    let common = resources
        .iter()
        .flat_map(|resource| configuration_objects(&resource.descriptor))
        .filter(|reference| loaded_common.contains(&reference.id))
        .map(|reference| &reference.id);
    let object_commands = objects()
        .filter(|object| {
            object.metadata.kind() == MetadataKind::Command
                && owner.is_some()
                && object.metadata.parent() == owner
        })
        .map(|object| object.metadata.id());
    let mut seen = HashSet::new();
    // The Designer places global actions before commands belonging to the object in each group.
    for id in common.chain(object_commands) {
        if authored.contains(id) || !seen.insert(id) {
            continue;
        }
        let Some((resource, command)) = effective_metadata(id, resources) else {
            continue;
        };
        let group = placements
            .get(id)
            .copied()
            .or_else(|| metadata_text(command, "Group", resources));
        if !command_types::accepts(command, parameter, resources) {
            continue;
        }
        let Some(items) = group.and_then(|group| bar.items(group, resources, diagnostics)) else {
            continue;
        };
        if let Some(mut item) = ResolvedCommand::Metadata(command).item(
            id.to_string(),
            Some(origin(resource, command)),
            resources,
            diagnostics,
        ) {
            if command.metadata.kind() == MetadataKind::CommonCommand {
                item.order_by_caption = Some(true);
            }
            items.push(item);
        }
    }
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

pub(super) struct InterfaceItem<'a> {
    reference: &'a str,
    group: &'a str,
    added: bool,
}

pub(super) fn metadata_placements<'a>(
    interface: &[InterfaceItem<'a>],
) -> HashMap<ObjectId, &'a str> {
    interface
        .iter()
        .filter_map(|item| metadata_command_id(item.reference).map(|id| (id, item.group)))
        .collect()
}

pub(super) fn interface_items(form: &ComposedForm) -> Vec<InterfaceItem<'_>> {
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

fn origin(resource: &FormResource, object: &ParsedObject) -> PropertySource {
    PropertySource {
        source: resource.source.clone(),
        range: object.range.clone(),
    }
}
