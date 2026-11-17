//! Explicit form DTOs keep XML i64 IDs lossless in JavaScript and paths reversible on both OSes.

use crate::ide::{
    dto::{self, Labels},
    property_types::appearance_field,
};
use eska_project_core::project::{
    forms::{
        ComposedForm, EffectiveDefinition, EffectiveElement, ElementIdentity, ElementKey,
        ElementPresentation, FormDependency, FormDependencyRole, FormHandler, FormHandlerOwner,
        FormPart, FormPicture, FormPictureSelection, FormPreparation, FormPreview, FormReference,
        PropertySource, ReferenceSlot, SourcedProperty,
    },
    metadata_model::{MetadataKind, MetadataProperty, MetadataValue, PropertyKey},
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
fn handler(labels: &Labels, value: &FormHandler) -> Value {
    let owner = match &value.owner {
        FormHandlerOwner::Element(value) => json!({"kind":"element","identity":identity(value)}),
        FormHandlerOwner::Command(name) => json!({"kind":"command","name":name}),
    };
    let key = PropertyKey {
        namespace: Some(eska_project_core::project::forms::FORM_NAMESPACE.into()),
        name: value.event.clone(),
    };
    let caption = dto::translations(
        labels.ru.property_caption(None, &key),
        labels.en.property_caption(None, &key),
    );
    json!({"owner":owner,"event":value.event,"name":value.name,
        "caption":caption,"origin":origin(&value.origin)})
}

type References<'a> = HashMap<&'a str, &'a [FormReference]>;

fn property(labels: &Labels, references: &References<'_>, value: &SourcedProperty) -> Value {
    let mut result = dto::property_record(labels, MetadataKind::Form, &value.property);
    let range = &value.origin.range;
    result["range"] = json!(range);
    result["origin"] = origin(&value.origin);
    if let Some(references) = references.get(value.origin.source.as_str()) {
        let start = references.partition_point(|reference| reference.range.start < range.start);
        for reference in references[start..]
            .iter()
            .take_while(|reference| reference.range.start < range.end)
        {
            annotate_appearance(labels, &value.property, &mut result, range.start, reference);
        }
    }
    result
}

fn annotate_appearance(
    labels: &Labels,
    mut property: &MetadataProperty,
    mut wire: &mut Value,
    start: usize,
    reference: &FormReference,
) {
    let Some(indices) = reference.indices_from(start) else {
        return;
    };
    let mut parent = None;
    for index in indices {
        let MetadataValue::Record(fields) = &property.value else {
            return;
        };
        let (Some(field), Some(view)) = (fields.get(index), wire["value"]["fields"].get_mut(index))
        else {
            return;
        };
        parent = Some(property);
        (property, wire) = (field, view);
    }
    let appearance = |field| appearance_field(field, "Color") || appearance_field(field, "Font");
    let nested_reference = reference.slot == ReferenceSlot::Text
        && property.key.namespace.as_deref() == Some("http://v8.1c.ru/8.3/xcf/readable")
        && property.key.name == "Ref"
        && parent.is_some_and(appearance);
    if property.key != reference.property || !(nested_reference || appearance(property)) {
        return;
    }
    let target = &reference.target;
    let Some(caption) = dto::translations(
        labels.ru.appearance_reference_caption(target),
        labels.en.appearance_reference_caption(target),
    ) else {
        return;
    };
    match reference.slot {
        ReferenceSlot::Text if wire["value"]["kind"] == "text" => {
            wire["value"]["caption"] = caption;
            if nested_reference {
                wire["caption"] = labels.caption("form-inspector-qualifier-ref");
            }
        }
        ReferenceSlot::Ref => {
            if let Some(qualifier) = wire["qualifiers"].as_array_mut().and_then(|qualifiers| {
                qualifiers.iter_mut().find(|value| {
                    value["key"]["namespace"].is_null() && value["key"]["name"] == "ref"
                })
            }) {
                qualifier["caption"] = caption;
                qualifier["keyCaption"] = labels.caption("form-inspector-qualifier-ref");
            }
        }
        ReferenceSlot::Text => {}
    }
}

fn element(
    labels: &Labels,
    references: &References<'_>,
    form: &ComposedForm,
    value: &EffectiveElement,
    presentations: &HashMap<&ElementIdentity, &ElementPresentation>,
    api_minor: u32,
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
        if api_minor >= 46
            && let Some(picture) = &presentation.choice_button_picture
        {
            value["choiceButtonPicture"] = json!(picture);
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
        api_minor < 43
            && node.kind.is_some_and(|kind| {
                kind.family() == eska_project_core::project::forms::ElementFamily::Addition
            })
    };
    let dto = |child| element(labels, references, form, child, presentations, api_minor);
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
        "properties":form.inspectable_properties(value).map(|value| property(labels,references,value)).collect::<Vec<_>>(),"presentation":presentation,
        "children":children,"auxiliary":auxiliary})
}
fn definition(
    labels: &Labels,
    references: &References<'_>,
    value: &EffectiveDefinition,
    api_minor: u32,
) -> Value {
    let mut result = json!({"name":value.name,"xmlName":value.xml_name,"origin":origin(&value.origin),"qualifiers":value.qualifiers,"properties":value.properties.iter().map(|value| property(labels,references,value)).collect::<Vec<_>>()});
    if api_minor >= 45
        && let Some(value_type) = value.inspector_type()
    {
        result["valueType"] = json!(value_type);
    }
    result
}
pub(super) fn model(
    labels: &Labels,
    preview: &FormPreview,
    api_minor: u32,
) -> Result<(Value, usize), Value> {
    let references: References = preview
        .documents
        .iter()
        .map(|document| (document.source.as_str(), document.references.as_slice()))
        .collect();
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
            element
                .picture
                .iter()
                .chain(match &element.choice_button_picture {
                    Some(FormPictureSelection::Image(picture)) if api_minor >= 46 => Some(picture),
                    _ => None,
                })
                .chain(
                    element
                        .auto_command_items
                        .iter()
                        .flat_map(|command| std::iter::once(command).chain(&command.items))
                        .filter_map(|command| command.picture.as_ref()),
                )
        })
        .filter_map(|picture| match picture {
            FormPicture::Common { source } => Some(source.as_str()),
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
    let handlers = handlers
        .iter()
        .map(|value| handler(labels, value))
        .collect::<Vec<_>>();
    let definition = |value| definition(labels, &references, value, api_minor);
    let mut model = json!({"root":element(labels,&references,&preview.form,&preview.form.root,&presentations,api_minor),"importantColor":preview.presentation.important_color,"assets":assets,"handlers":handlers,"attributes":preview.form.attributes.iter().map(definition).collect::<Vec<_>>(),
        "commands":preview.form.commands.iter().map(definition).collect::<Vec<_>>(),"parameters":preview.form.parameters.iter().map(definition).collect::<Vec<_>>(),
        "diagnostics":{"composition":preview.form.diagnostics,"appearance":preview.presentation.diagnostics}});
    if api_minor >= 45 {
        model["dataFields"] = json!(inspector_fields(labels, preview));
    }
    if api_minor >= 47 {
        model["expandedData"] = json!(preview.expanded_data);
    }
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

fn inspector_fields(labels: &Labels, preview: &FormPreview) -> Vec<Value> {
    preview.inspector_data().iter().map(|field| {
        let mut row = json!({
            "path":field.path,"valueType":field.value_type,
            "type":field.type_property.map(|value| dto::property_record(labels, MetadataKind::Attribute, value)),
            "origin":field.origin.as_ref().map(origin)
        });
        if let Some(caption) = &field.caption {
            row["caption"] = json!(caption);
            if let eska_project_core::project::forms::PresentationText::Standard { key } = caption
                && let Some(name) = key.strip_prefix("attribute.")
                && let (Some(ru), Some(en)) = (
                    labels.ru.enum_caption("StandardAttribute", name),
                    labels.en.enum_caption("StandardAttribute", name),
                )
            {
                row["caption"] = json!({"kind":"literal","items":[
                    {"language":"ru","content":ru}, {"language":"en","content":en}
                ]});
            }
        }
        row
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_preserves_raw_values_but_exposes_localized_settings_and_boolean_controls() {
        let make = |namespace: &str, name: &str, value| MetadataProperty {
            key: PropertyKey {
                namespace: Some(namespace.into()),
                name: name.into(),
            },
            value,
            qualifiers: Vec::new(),
        };
        let settings = "http://v8.1c.ru/8.1/data-composition-system/settings";
        let form = eska_project_core::project::forms::FORM_NAMESPACE;
        let field = make(
            form,
            "ListSettings",
            MetadataValue::Record(vec![
                make(form, "ManualQuery", MetadataValue::Text("false".into())),
                make(settings, "orderType", MetadataValue::Text("Desc".into())),
                make(
                    "urn:custom",
                    "ManualQuery",
                    MetadataValue::Text("false".into()),
                ),
                make(form, "Comment", MetadataValue::Text("RadioButtons".into())),
            ]),
        );
        let wire = dto::property_record(&Labels::new().unwrap(), MetadataKind::Form, &field);
        assert_eq!(wire["caption"]["ru-RU"], "Настройки списка");
        assert_eq!(wire["caption"]["en-US"], "List settings");
        let fields = wire["value"]["fields"].as_array().unwrap();
        assert_eq!(fields[0]["caption"]["ru-RU"], "Произвольный запрос");
        assert_eq!(fields[0]["value"]["scalarType"], "boolean");
        assert_eq!(fields[0]["value"]["text"], "false");
        assert_eq!(fields[1]["value"]["caption"]["ru-RU"], "По убыванию");
        assert_eq!(fields[1]["value"]["text"], "Desc");
        assert!(fields[2]["caption"].is_null());
        assert!(fields[2]["value"]["scalarType"].is_null());
        assert!(fields[3]["value"]["caption"].is_null());
        assert_eq!(fields[3]["value"]["text"], "RadioButtons");
    }
}
