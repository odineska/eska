//! Explicit form DTOs keep XML i64 IDs lossless in JavaScript and paths reversible on both OSes.

use crate::ide::dto::{self, Labels};
use eska_project_core::project::{
    forms::{
        EffectiveDefinition, EffectiveElement, ElementIdentity, ElementKey, ElementPresentation,
        FormDependency, FormDependencyRole, FormPart, FormPreview, PropertySource, SourcedProperty,
    },
    metadata_model::MetadataKind,
    metadata_parser::LocatedProperty,
};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) fn identity(value: &ElementIdentity) -> Value {
    json!({"source":value.source,"part":match value.part { FormPart::Current=>"current",FormPart::StoredBase=>"storedBase" },"key":match value.key { ElementKey::Root=>json!({"kind":"root"}),ElementKey::Xml(value)=>json!({"kind":"xml","value":value.to_string()}) }})
}
pub(super) fn origin(value: &PropertySource) -> Value {
    json!({"source":value.source,"range":value.range})
}
pub(super) fn dependency(value: &FormDependency) -> Value {
    json!({"id":value.id,"root":dto::path(&value.root),"path":dto::path(&value.path),"snapshot":value.snapshot,"version":value.version,"member":value.member,"role":match value.role { FormDependencyRole::Form=>"form",FormDependencyRole::Metadata=>"metadata" }})
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
    value: &EffectiveElement,
    presentations: &HashMap<&ElementIdentity, &ElementPresentation>,
) -> Value {
    let presentation = presentations.get(&value.identity).map(|presentation| {
        json!({"grouping":presentation.grouping,"displayed":presentation.displayed,"titleLocation":presentation.title_location,"caption":presentation.caption,"tooltip":presentation.tooltip,"valueType":presentation.value_type,"binding":presentation.binding,"choices":presentation.choices,
            "font":presentation.font,"titleFont":presentation.title_font,"foreground":presentation.foreground,"background":presentation.background,"border":presentation.border,
            "titleColor":presentation.title_color,"autoCommands":presentation.auto_commands})
    });
    json!({"identity":identity(&value.identity),"origin":origin(&value.origin),"kind":value.kind.map(eska_project_core::project::forms::ElementKind::xml_name),"name":value.name,"xmlName":value.xml_name,
        "properties":value.properties.iter().map(|value| property(labels,value)).collect::<Vec<_>>(),"presentation":presentation,
        "children":value.children.iter().map(|value| element(labels,value,presentations)).collect::<Vec<_>>(),"auxiliary":value.auxiliary.iter().map(|value| element(labels,value,presentations)).collect::<Vec<_>>()})
}
fn definition(labels: &Labels, value: &EffectiveDefinition) -> Value {
    json!({"name":value.name,"xmlName":value.xml_name,"origin":origin(&value.origin),"qualifiers":value.qualifiers,"properties":value.properties.iter().map(|value| property(labels,value)).collect::<Vec<_>>()})
}
pub(super) fn model(labels: &Labels, preview: &FormPreview) -> Result<(Value, usize), Value> {
    let presentations: HashMap<_, _> = preview
        .presentation
        .elements
        .iter()
        .map(|element| (&element.identity, element))
        .collect();
    let model = json!({"root":element(labels,&preview.form.root,&presentations),"attributes":preview.form.attributes.iter().map(|value| definition(labels,value)).collect::<Vec<_>>(),
        "commands":preview.form.commands.iter().map(|value| definition(labels,value)).collect::<Vec<_>>(),"parameters":preview.form.parameters.iter().map(|value| definition(labels,value)).collect::<Vec<_>>(),
        "diagnostics":{"composition":preview.form.diagnostics,"appearance":preview.presentation.diagnostics}});
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
    Ok((model, retained))
}
