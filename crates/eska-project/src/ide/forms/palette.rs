//! Project the selected element's palette without repeating its XML property values.

use super::{Entry, dto, failure, limit, navigation};
use crate::ide::{dto::Labels, params};
use eska_project_core::project::forms::{
    FormScriptVariant, PaletteCaption, PaletteCategory, PaletteEditor, PaletteRow, PaletteValue,
    PlatformPropertyValue,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    revision: String,
    identity: Value,
    #[serde(default)]
    include_event_rows: bool,
}

pub(super) fn inspect(
    labels: &Labels,
    entry: &Entry,
    form_session_id: &str,
    args: &Value,
) -> Result<Value, Value> {
    let input: Input = params::decode(args)?;
    navigation::check_revision(entry, &input.revision)?;
    let identity = navigation::identity(&input.identity)?;
    let palette = entry
        .preview
        .property_palette(&identity)
        .ok_or_else(|| failure("target_unavailable"))?;
    let mut categories = Vec::new();
    let rows = palette.rows.map(|rows| {
        rows.into_iter()
            .filter(|row| {
                if let Some(definition) = row.definition {
                    let category = definition.category;
                    if category == PaletteCategory::Events && !input.include_event_rows {
                        return false;
                    }
                    if !categories.contains(&category) {
                        categories.push(category);
                    }
                }
                true
            })
            .map(|row| row_dto(labels, &row, entry.preview.presentation.script_variant))
            .collect::<Vec<_>>()
    });
    let categories = rows.as_ref().map(|_| {
        categories
            .into_iter()
            .map(PaletteCategory::id)
            .map(|id| json!({"id":id,"caption":translate(labels, &format!("form-palette-category-{id}"))}))
            .collect::<Vec<_>>()
    });
    let value = json!({"formSessionId":form_session_id,"revision":input.revision,
        "identity":dto::identity(&identity),"rows":rows,"categories":categories});
    if serde_json::to_vec(&value)
        .map_err(|_| failure("invalid_model"))?
        .len()
        > super::MAX_MODEL_BYTES
    {
        return Err(limit());
    }
    Ok(value)
}

const fn source_kind(extension: bool) -> &'static str {
    if extension { "extension" } else { "form" }
}

fn row_dto(
    labels: &Labels,
    row: &PaletteRow<'_>,
    script_variant: Option<FormScriptVariant>,
) -> Value {
    let boolean = row
        .definition
        .is_some_and(|definition| matches!(definition.editor, PaletteEditor::Boolean));
    let (raw_id, value) = match row.value {
        PaletteValue::Declared {
            index,
            property,
            occurrence,
            extension,
        } => {
            let property = &property.property;
            let key = &property.key;
            let mut value = json!({"kind":"declared","propertyIndex":index,"occurrence":occurrence,
                "sourceKind":source_kind(extension)});
            if boolean && super::super::property_types::boolean_scalar(&property.value, true) {
                value["scalarType"] = json!("boolean");
            }
            if let Some(caption) = row
                .definition
                .and_then(|definition| definition.value_caption(&property.value))
            {
                value["caption"] = value_caption(labels, caption, script_variant);
            }
            (
                format!("xml:{}", json!([key.namespace, key.name, occurrence])),
                value,
            )
        }
        PaletteValue::DeclaredEvent {
            index,
            entry_index,
            occurrence,
            extension,
        } => (
            String::new(),
            json!({"kind":"declaredEvent","propertyIndex":index,"entryIndex":entry_index,
                "occurrence":occurrence,"sourceKind":source_kind(extension)}),
        ),
        PaletteValue::Qualifier {
            attribute,
            extension,
        } => {
            let key = &attribute.key;
            let mut value = json!({"kind":"qualifier","key":key,"text":attribute.text,
                "origin":dto::origin(&attribute.origin),"sourceKind":source_kind(extension)});
            if boolean && super::super::property_types::boolean_literal(&attribute.text) {
                value["scalarType"] = json!("boolean");
            }
            if let Some(caption) = row
                .definition
                .and_then(|definition| definition.text_caption(&attribute.text))
            {
                value["caption"] = value_caption(labels, caption, script_variant);
            }
            (
                format!("attribute:{}", json!([key.namespace, key.name])),
                value,
            )
        }
        PaletteValue::Name(name) => (String::new(), json!({"kind":"name","value":name})),
        PaletteValue::Kind(kind) => {
            let caption = translate(labels, &format!("form-palette-type-{}", kind.xml_name()));
            (
                String::new(),
                json!({"kind":"elementKind","value":kind,"caption":caption}),
            )
        }
        PaletteValue::Absent(value) => (String::new(), absent(labels, value)),
    };
    let (id, category, editor, caption) = row.definition.map_or_else(
        || (raw_id, Value::Null, Value::Null, Value::Null),
        |definition| {
            (
                format!("platform:{}", definition.id),
                json!(definition.category),
                json!(definition.editor),
                translate(labels, &format!("form-palette-property-{}", definition.id)),
            )
        },
    );
    let mut result =
        json!({"id":id,"category":category,"editor":editor,"caption":caption,"value":value});
    if !row.available {
        result["available"] = json!(false);
    }
    result
}

fn value_caption(
    labels: &Labels,
    value: PaletteCaption<'_>,
    script_variant: Option<FormScriptVariant>,
) -> Value {
    match value {
        PaletteCaption::Platform(key) => translate(labels, &format!("form-palette-value-{key}")),
        PaletteCaption::Identifier(name) => json!({"ru-RU":name,"en-US":name}),
        PaletteCaption::ScriptMember { item, member } => {
            let key = format!("form-palette-value-{member}");
            let ru = labels.ru.text(&key);
            let en = labels.en.text(&key);
            let (ru, en) = match script_variant {
                Some(FormScriptVariant::Russian) => (&ru, &ru),
                Some(FormScriptVariant::English) => (&en, &en),
                None => (&ru, &en),
            };
            json!({"ru-RU":format!("{item}.{ru}"),"en-US":format!("{item}.{en}")})
        }
    }
}

fn absent(labels: &Labels, default: PlatformPropertyValue) -> Value {
    let caption = match default {
        PlatformPropertyValue::Auto => Some("auto"),
        PlatformPropertyValue::AutoStretch => Some("autoStretch"),
        PlatformPropertyValue::Choice(value) => Some(value),
        PlatformPropertyValue::Dialog => Some("open"),
        PlatformPropertyValue::Unknown => Some("unknown"),
        PlatformPropertyValue::Boolean(_)
        | PlatformPropertyValue::Number(_)
        | PlatformPropertyValue::Empty => None,
    };
    json!({"kind":"absent","default":default,"caption":caption.map(|key|translate(labels,&format!("form-palette-value-{key}")))})
}

fn translate(labels: &Labels, key: &str) -> Value {
    json!({"ru-RU":labels.ru.text(key),"en-US":labels.en.text(key)})
}
