//! Localized summaries keep machine protocols and terminal prompts separate.

use eska_presentation::localization::{LocalizationValue, Localizer};
use serde_json::Value;

pub(super) fn success(value: &Value, locale: &Localizer) {
    if value.get("prepared").is_some() {
        review(value, locale);
        return;
    }
    if value.get("changed").is_some() {
        let object = value["objectId"]
            .as_str()
            .or_else(|| value["form"].as_str())
            .unwrap_or_default();
        println!(
            "{}",
            locale.format(
                "metadata-authoring-written",
                &[("object", LocalizationValue::Text(object))]
            )
        );
        return;
    }
    // Discovery is already bilingual and retains stable keys for terminal copy/paste.
    if let Ok(text) = serde_json::to_string_pretty(value) {
        println!("{text}");
    }
}

pub(super) fn review(value: &Value, locale: &Localizer) {
    let request = &value["prepared"]["request"];
    let object = value["objectId"]
        .as_str()
        .or_else(|| value["form"].as_str())
        .or_else(|| request["object"].as_str())
        .unwrap_or_default();
    let operation = value["operation"].as_str().unwrap_or("form-edit");
    eprintln!(
        "{}",
        locale.format(
            "metadata-authoring-review",
            &[
                (
                    "operation",
                    LocalizationValue::Text(
                        &locale.text(&format!("metadata-authoring-operation-{operation}"))
                    )
                ),
                ("object", LocalizationValue::Text(object)),
            ]
        )
    );
    if let Some(files) = value["files"].as_array() {
        for file in files {
            let action = file["action"].as_str().unwrap_or("update");
            let path = file["path"]["value"].as_str().unwrap_or_default();
            eprintln!(
                "{}",
                locale.format(
                    "metadata-authoring-file",
                    &[
                        (
                            "action",
                            LocalizationValue::Text(
                                &locale.text(&format!("metadata-authoring-action-{action}"))
                            )
                        ),
                        ("path", LocalizationValue::Text(path)),
                    ]
                )
            );
        }
    }
    if let Some(references) = value["references"]
        .as_array()
        .filter(|items| !items.is_empty())
    {
        eprintln!(
            "{}",
            locale.format(
                "metadata-authoring-references",
                &[(
                    "count",
                    LocalizationValue::Number(i64::try_from(references.len()).unwrap_or(i64::MAX))
                )]
            )
        );
        for reference in references.iter().take(20) {
            eprintln!(
                "  {}",
                reference["path"]["value"].as_str().unwrap_or_default()
            );
        }
    }
    if let Some(path) = value["source"]["value"].as_str() {
        eprintln!(
            "{}",
            locale.format(
                "metadata-create-file",
                &[("path", LocalizationValue::Text(path))]
            )
        );
    }
}

pub(super) fn error(error: &Value, locale: &Localizer) {
    let error = error.get("data").unwrap_or(error);
    let code = error["kind"].as_str().unwrap_or_default();
    let key = match code {
        "object_unsupported" => "metadata-authoring-unsupported",
        "object_invalid" | "object_input_invalid" | "form_invalid" => {
            "metadata-create-input-invalid"
        }
        "object_references" | "form_references" => "metadata-authoring-references-error",
        "object_committed_refresh_required" | "form_committed_refresh_required" => {
            "metadata-create-committed"
        }
        "object_io" => "metadata-create-write-failed",
        "object_property_conflict" => "metadata-authoring-property-conflict",
        "object_confirmation_required" => "metadata-authoring-confirmation-required",
        "form_invalid_xml" => "metadata-authoring-invalid-form",
        "form_target_missing" => "metadata-authoring-target-missing",
        "form_name_collision" => "metadata-create-collision",
        _ => {
            super::super::creation::presentation::error(error, locale);
            return;
        }
    };
    eprintln!("{}", locale.text(key));
    let details = &error["details"];
    if let Some(field) = details["field"].as_str() {
        eprintln!(
            "{}",
            locale.format(
                "metadata-authoring-error-field",
                &[("field", LocalizationValue::Text(field))]
            )
        );
    }
    if let Some(property) = details["property"].as_str() {
        let caption = locale.text(&format!("platform-property-md-{property}"));
        eprintln!(
            "{}",
            locale.format(
                "metadata-authoring-error-field",
                &[("field", LocalizationValue::Text(&caption))]
            )
        );
    }
}
