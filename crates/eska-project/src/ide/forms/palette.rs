//! Project the selected element's palette without repeating its XML property values.

use super::{Entry, dto, failure, limit, navigation};
use crate::ide::{dto::Labels, params};
use eska_project_core::project::forms::{PaletteRow, PaletteValue, PlatformPropertyValue};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Input {
    revision: String,
    identity: Value,
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
    let rows =
        palette.rows.map(|rows| {
            rows.into_iter()
            .map(|row| {
                let (raw_id, value) = match row.value {
                    PaletteValue::Declared {
                        index,
                        occurrence,
                        extension,
                    } => {
                        let key = &palette.element.properties[index].property.key;
                        (
                            format!("xml:{}", json!([key.namespace, key.name, occurrence])),
                            json!({"kind":"declared","propertyIndex":index,"occurrence":occurrence,
                                "sourceKind":if extension { "extension" } else { "form" }}),
                        )
                    }
                    PaletteValue::Qualifier { attribute, extension } => {
                        let key = &attribute.key;
                        (
                            format!("attribute:{}", json!([key.namespace, key.name])),
                            json!({"kind":"qualifier","key":key,"text":attribute.text,
                                "origin":dto::origin(&attribute.origin),
                                "sourceKind":if extension { "extension" } else { "form" }}),
                        )
                    }
                    PaletteValue::Name(name) => {
                        (String::new(), json!({"kind":"name","value":name}))
                    }
                    PaletteValue::Kind(kind) => {
                        let caption =
                            translate(labels, &format!("form-palette-type-{}", kind.xml_name()));
                        (
                            String::new(),
                            json!({"kind":"elementKind","value":kind,"caption":caption}),
                        )
                    }
                    PaletteValue::Absent(value) => (String::new(), absent(labels, value)),
                };
                row_dto(labels, &row, raw_id, value)
            })
            .collect::<Vec<_>>()
        });
    let categories = rows.as_ref().map(|_| {
        ["main", "usage", "appearance", "layout"]
            .into_iter()
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

fn row_dto(labels: &Labels, row: &PaletteRow<'_>, raw_id: String, value: Value) -> Value {
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
    let mut row = json!({"id":id,"category":category,"editor":editor,"caption":caption});
    row["value"] = value;
    row
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
