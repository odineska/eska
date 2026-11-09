//! Explicit form DTOs keep XML i64 IDs lossless in JavaScript and paths reversible on both OSes.

use crate::ide::dto::{self, Labels};
use eska_project_core::project::{
    forms::{
        ComposedForm, EffectiveDefinition, EffectiveElement, ElementIdentity, ElementKey,
        ElementPresentation, FormDependency, FormDependencyRole, FormPart, FormPreparation,
        FormPreview, PropertySource, SourcedProperty,
    },
    metadata_model::MetadataKind,
    metadata_parser::LocatedProperty,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub(super) fn identity(value: &ElementIdentity) -> Value {
    json!({"source":value.source,"part":match value.part { FormPart::Current=>"current",FormPart::StoredBase=>"storedBase" },"key":match value.key { ElementKey::Root=>json!({"kind":"root"}),ElementKey::Xml(value)=>json!({"kind":"xml","value":value.to_string()}) }})
}
pub(super) fn origin(value: &PropertySource) -> Value {
    json!({"source":value.source,"range":value.range})
}
pub(super) fn dependency(value: &FormDependency) -> Value {
    json!({"id":value.id,"root":dto::path(&value.root),"path":dto::path(&value.path),"snapshot":value.snapshot,"version":value.version,"member":value.member,"role":match value.role { FormDependencyRole::Form=>"form",FormDependencyRole::Metadata=>"metadata", FormDependencyRole::Picture=>"picture" }})
}
pub(super) fn preparation(value: &FormPreparation) -> Value {
    json!({"contextFingerprint":value.context_fingerprint,"application":value.application,"limitedContext":value.limited_context,
        "roots":value.roots.iter().map(|root|dto::path(root)).collect::<Vec<_>>(),
        "forms":value.forms.iter().map(|input|json!({"id":input.id,"root":dto::path(&input.root),"path":dto::path(&input.path)})).collect::<Vec<_>>()})
}
fn property(labels: &Labels, value: &SourcedProperty) -> Value {
    let mut result = dto::property(
        labels,
        MetadataKind::Form,
        &LocatedProperty {
            property: value.property.clone(),
            range: value.origin.range.clone(),
        },
    );
    result["origin"] = origin(&value.origin);
    result
}
fn element(
    labels: &Labels,
    form: &ComposedForm,
    value: &EffectiveElement,
    presentations: &HashMap<&ElementIdentity, &ElementPresentation>,
    layout_additions: bool,
) -> Value {
    let presentation = presentations.get(&value.identity).map(|presentation| {
        let mut value = json!({"grouping":presentation.grouping,"united":presentation.united,"displayed":presentation.displayed,"markIncomplete":presentation.mark_incomplete,"titleLocation":presentation.title_location,"picture":presentation.picture,"caption":presentation.caption,"tooltip":presentation.tooltip,"footerText":presentation.footer_text,"valueType":presentation.value_type,"binding":presentation.binding,"choices":presentation.choices,
            "font":presentation.font,"titleFont":presentation.title_font,"foreground":presentation.foreground,"background":presentation.background,"border":presentation.border,
            "titleColor":presentation.title_color,"autoCommands":presentation.auto_commands,"autoCommandItems":presentation.auto_command_items});
        if let Some(column) = &presentation.table_column_appearance {
            value["tableColumnAppearance"] = json!(column);
        }
        if let Some(total) = presentation.tabular_total {
            value["tabularTotal"] = json!(total);
        }
        if let Some(mode) = presentation.button_representation {
            value["buttonRepresentation"] = json!(mode);
        }
        if !presentation.object_command_menu.is_empty() {
            value["objectCommandMenu"] = json!(presentation.object_command_menu);
        }
        if !presentation.table_command_menu.is_empty() {
            value["tableCommandMenu"] = json!(presentation.table_command_menu);
        }
        value
    });
    let legacy_addition = |node: &EffectiveElement| {
        !layout_additions
            && node.kind.is_some_and(|kind| {
                kind.family() == eska_project_core::project::forms::ElementFamily::Addition
            })
    };
    let dto = |child| element(labels, form, child, presentations, layout_additions);
    let children = value
        .children
        .iter()
        .filter(|child| !legacy_addition(child))
        .map(dto)
        .collect::<Vec<_>>();
    let auxiliary = value
        .auxiliary
        .iter()
        .chain(value.children.iter().filter(|child| legacy_addition(child)))
        .map(dto)
        .collect::<Vec<_>>();
    json!({"identity":identity(&value.identity),"origin":origin(&value.origin),"kind":value.kind.map(eska_project_core::project::forms::ElementKind::xml_name),"name":value.name,"xmlName":value.xml_name,
        "properties":form.inspectable_properties(value).map(|value| property(labels,value)).collect::<Vec<_>>(),"presentation":presentation,
        "children":children,"auxiliary":auxiliary})
}
fn definition(labels: &Labels, value: &EffectiveDefinition) -> Value {
    json!({"name":value.name,"xmlName":value.xml_name,"origin":origin(&value.origin),"qualifiers":value.qualifiers,"properties":value.properties.iter().map(|value| property(labels,value)).collect::<Vec<_>>()})
}
pub(super) fn model(
    labels: &Labels,
    preview: &FormPreview,
    api_minor: u32,
) -> Result<(Value, usize), Value> {
    let layout_additions = api_minor >= 43;
    let presentations: HashMap<_, _> = preview
        .presentation
        .elements
        .iter()
        .map(|element| (&element.identity, element))
        .collect();
    let image_sources: HashSet<_> = preview
        .presentation
        .elements
        .iter()
        .flat_map(|element| {
            element.picture.iter().chain(
                element
                    .auto_command_items
                    .iter()
                    .flat_map(|command| std::iter::once(command).chain(&command.items))
                    .filter_map(|command| command.picture.as_ref()),
            )
        })
        .filter_map(|picture| match picture {
            eska_project_core::project::forms::FormPicture::Common { source } => {
                Some(source.as_str())
            }
            _ => None,
        })
        .collect();
    if image_sources.len() > 256 {
        return Err(super::limit());
    }
    let assets = preview
        .resources
        .iter()
        .filter(|resource| image_sources.contains(resource.source.as_str()))
        .filter_map(|resource| {
            resource
                .picture
                .as_ref()
                .map(|picture| json!({"id":resource.source,"picture":dto::picture(picture)}))
        })
        .collect::<Vec<_>>();
    let handlers = eska_project_core::project::forms::handlers(&preview.form);
    if handlers.len() > eska_project_core::project::forms::MAX_FORM_ELEMENTS {
        return Err(super::limit());
    }
    let handlers=handlers.iter().map(|handler|json!({"owner":match &handler.owner {
        eska_project_core::project::forms::FormHandlerOwner::Element(value)=>json!({"kind":"element","identity":identity(value)}),
        eska_project_core::project::forms::FormHandlerOwner::Command(name)=>json!({"kind":"command","name":name}),
    },"event":handler.event,"name":handler.name,"origin":origin(&handler.origin)})).collect::<Vec<_>>();
    let mut model = json!({"root":element(labels,&preview.form,&preview.form.root,&presentations,layout_additions),"importantColor":preview.presentation.important_color,"assets":assets,"handlers":handlers,"attributes":preview.form.attributes.iter().map(|value| definition(labels,value)).collect::<Vec<_>>(),
        "commands":preview.form.commands.iter().map(|value| definition(labels,value)).collect::<Vec<_>>(),"parameters":preview.form.parameters.iter().map(|value| definition(labels,value)).collect::<Vec<_>>(),
        "diagnostics":{"composition":preview.form.diagnostics,"appearance":preview.presentation.diagnostics}});
    model["activityColor"] = json!(preview.presentation.activity_color);
    if api_minor >= 44 {
        model["formHelpAvailable"] = json!(preview.form_help_available);
    }
    if let Some(variant) = preview.presentation.script_variant {
        model["scriptVariant"] = json!(variant);
    }
    let bytes = serde_json::to_vec(&model)
        .map_err(|_| super::failure("invalid_model"))?
        .len();
    if bytes > super::MAX_MODEL_BYTES {
        return Err(super::limit());
    }
    let retained = bytes.saturating_mul(3).saturating_add(
        preview
            .documents
            .iter()
            .map(|document| document.input().len())
            .sum::<usize>(),
    );
    let retained = retained.saturating_add(
        preview
            .resources
            .iter()
            .map(|resource| resource.retained_bytes)
            .sum::<usize>(),
    );
    Ok((model, retained))
}
