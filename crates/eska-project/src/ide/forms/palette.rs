//! Project the selected element's palette without repeating its XML property values.

use super::{Entry, dto, failure, limit, navigation};
use crate::ide::{dto::Labels, params};
use eska_project_core::project::forms::{
    EffectiveElement, PaletteCategory, PaletteRow, PaletteValue, PlatformPropertyValue,
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
            .map(|row| row_dto(labels, &row, palette.element))
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

fn row_dto(labels: &Labels, row: &PaletteRow<'_>, element: &EffectiveElement) -> Value {
    let (raw_id, value) = match row.value {
        PaletteValue::Declared {
            index,
            occurrence,
            extension,
        } => {
            let property = &element.properties[index].property;
            let key = &property.key;
            let mut value = json!({"kind":"declared","propertyIndex":index,"occurrence":occurrence,
                "sourceKind":source_kind(extension)});
            if let Some(caption) = row
                .definition
                .and_then(|definition| definition.enum_caption(&property.value))
            {
                value["caption"] = translate(labels, &format!("form-palette-value-{caption}"));
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
            (
                format!("attribute:{}", json!([key.namespace, key.name])),
                json!({"kind":"qualifier","key":key,"text":attribute.text,
                    "origin":dto::origin(&attribute.origin),"sourceKind":source_kind(extension)}),
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
