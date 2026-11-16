//! Localized terminal presentation never changes semantic values in machine responses.

use eska_presentation::localization::{LocalizationValue, Localizer};
use eska_project_core::project::{
    metadata_create::{CreateRequest, CreateType, CreationKind, CreationPlan, DateFractions},
    metadata_model::PropertyKey,
};
use serde_json::Value;

pub(super) fn success(value: &Value, locale: &Localizer) {
    let object = value["objectId"].as_str().unwrap_or_default();
    if let Some(prepared) = value.get("prepared")
        && let Ok(request) = serde_json::from_value::<CreateRequest>(prepared["request"].clone())
    {
        for line in review_lines(&request, object, value["source"]["value"].as_str(), locale) {
            println!("{line}");
        }
        return;
    }
    let key = if value.get("prepared").is_some() {
        "metadata-create-preview"
    } else {
        "metadata-create-written"
    };
    println!(
        "{}",
        locale.format(key, &[("object", LocalizationValue::Text(object))])
    );
    if let Some(path) = value["source"]["value"].as_str() {
        println!(
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
    let code = error["kind"].as_str().unwrap_or("unknown_error");
    if code == "creation_arguments_required" {
        let fields = error["details"]["fields"]
            .as_array()
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        eprintln!(
            "{}",
            locale.format(
                "metadata-create-required",
                &[("fields", LocalizationValue::Text(&fields))]
            )
        );
        return;
    }
    let key = match code {
        "creation_cancelled" => "metadata-create-cancelled",
        "creation_terminal_required" => "metadata-create-terminal-required",
        "creation_no_owners" => "metadata-create-no-owners",
        "creation_empty_choices" => "metadata-create-empty-choices",
        "creation_prompt_failed" => "metadata-create-prompt-failed",
        "creation_committed_refresh_required" => "metadata-create-committed",
        "creation_collision" => "metadata-create-collision",
        "creation_unsupported_owner" => "metadata-create-unsupported-owner",
        "creation_invalid_name" => "metadata-create-invalid-name",
        "creation_input_invalid" | "creation_invalid" | "property_invalid" | "invalid_request" => {
            "metadata-create-input-invalid"
        }
        "property_conflict" => "metadata-create-conflict",
        "property_read_only" => "metadata-create-read-only",
        "property_edit_busy" => "metadata-create-busy",
        "property_recovery_required" => "metadata-create-recovery",
        "property_write_failed" => "metadata-create-write-failed",
        "creation_identity_unavailable" => "metadata-create-identity-failed",
        "input_unavailable" => "metadata-create-input-unavailable",
        "input_too_large" => "metadata-create-input-too-large",
        _ => {
            eprintln!(
                "{}",
                locale.format(
                    "metadata-create-failed",
                    &[("reason", LocalizationValue::Text(code))]
                )
            );
            return;
        }
    };
    eprintln!("{}", locale.text(key));
}

pub(super) fn review(plan: &CreationPlan, locale: &Localizer) {
    for line in review_lines(
        &plan.prepared.request,
        plan.object_id.as_str(),
        Some(&plan.path.to_string_lossy()),
        locale,
    ) {
        eprintln!("{line}");
    }
}

fn review_lines(
    request: &CreateRequest,
    object: &str,
    path: Option<&str>,
    locale: &Localizer,
) -> Vec<String> {
    let mut lines = vec![
        locale.format(
            "metadata-create-preview",
            &[("object", LocalizationValue::Text(object))],
        ),
        format!(
            "{}: {}",
            locale.text("metadata-create-owner-prompt"),
            request.owner
        ),
    ];
    for value in &request.types {
        lines.push(format!(
            "{}: {}",
            locale.text("metadata-create-type-prompt"),
            type_caption(value, locale)
        ));
    }
    for synonym in &request.synonyms {
        lines.push(format!(
            "{} ({}): {}",
            locale.text("platform-property-md-Synonym"),
            synonym.language,
            synonym.content
        ));
    }
    if !request.comment.is_empty() {
        lines.push(format!(
            "{}: {}",
            locale.text("platform-property-md-Comment"),
            request.comment
        ));
    }
    if let Some(path) = path {
        lines.push(locale.format(
            "metadata-create-file",
            &[("path", LocalizationValue::Text(path))],
        ));
    }
    lines
}

fn type_caption(value: &CreateType, locale: &Localizer) -> String {
    let name = match value {
        CreateType::String { .. } => "string",
        CreateType::Number { .. } => "decimal",
        CreateType::Boolean { .. } => "boolean",
        CreateType::Date { .. } => "dateTime",
        CreateType::Reference { object } => {
            return format!(
                "{}: {object}",
                locale.text("metadata-create-reference-prompt")
            );
        }
    };
    let key = PropertyKey {
        namespace: Some("http://www.w3.org/2001/XMLSchema".into()),
        name: name.into(),
    };
    let caption = locale.type_caption(&key).unwrap_or_else(|| name.into());
    match value {
        CreateType::String { length, fixed } => format!(
            "{caption} ({length}, {})",
            enum_caption(
                locale,
                "AllowedLength",
                if *fixed { "Fixed" } else { "Variable" }
            )
        ),
        CreateType::Number {
            digits,
            fraction_digits,
            non_negative,
        } => format!(
            "{caption} ({digits}, {fraction_digits}, {})",
            enum_caption(
                locale,
                "AllowedSign",
                if *non_negative { "Nonnegative" } else { "Any" }
            )
        ),
        CreateType::Date { fractions } => format!(
            "{caption} ({})",
            enum_caption(locale, "DateFractions", date_token(*fractions))
        ),
        CreateType::Boolean { .. } | CreateType::Reference { .. } => caption,
    }
}

fn enum_caption(locale: &Localizer, domain: &str, value: &str) -> String {
    locale
        .enum_caption(domain, value)
        .unwrap_or_else(|| value.into())
}

const fn date_token(value: DateFractions) -> &'static str {
    match value {
        DateFractions::Date => "Date",
        DateFractions::Time => "Time",
        DateFractions::DateTime => "DateTime",
    }
}

pub(super) fn equivalent(request: &CreateRequest, selected: Option<&str>, locale: &Localizer) {
    let mut args = vec![
        "eska".to_owned(),
        "metadata".into(),
        "add".into(),
        match request.kind {
            CreationKind::Attribute => "attribute".into(),
            CreationKind::TablePart => "table-part".into(),
        },
    ];
    if let Some(selected) = selected {
        option(&mut args, "project", selected);
    }
    option(&mut args, "owner", request.owner.as_str());
    option(&mut args, "name", &request.name);
    for value in &request.types {
        option(&mut args, "type", super::arguments::type_name(value));
        match value {
            CreateType::String { length, fixed } => {
                option(&mut args, "length", &length.to_string());
                if *fixed {
                    args.push("--fixed-length".into());
                }
            }
            CreateType::Number {
                digits,
                fraction_digits,
                non_negative,
            } => {
                option(&mut args, "digits", &digits.to_string());
                option(&mut args, "fraction-digits", &fraction_digits.to_string());
                if *non_negative {
                    args.push("--nonnegative".into());
                }
            }
            CreateType::Date { fractions } => option(
                &mut args,
                "date-fractions",
                match fractions {
                    DateFractions::Date => "date",
                    DateFractions::Time => "time",
                    DateFractions::DateTime => "date-time",
                },
            ),
            CreateType::Reference { object } => option(&mut args, "reference", object.as_str()),
            CreateType::Boolean { .. } => {}
        }
    }
    for synonym in &request.synonyms {
        option(
            &mut args,
            "synonym",
            &format!("{}={}", synonym.language, synonym.content),
        );
    }
    if !request.comment.is_empty() {
        option(&mut args, "comment", &request.comment);
    }
    args.push("--no-input".into());
    eprintln!(
        "{}\n{}",
        locale.text("metadata-create-equivalent"),
        args.join(" ")
    );
}

fn option(args: &mut Vec<String>, key: &str, value: &str) {
    args.push(format!("--{key}"));
    args.push(quote(value));
}

/// Single-quoted values are shell literals; Windows output targets PowerShell.
fn quote(value: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn equivalent_command_quotes_shell_metacharacters_as_data() {
        let source = "name'$()`; data";
        assert_eq!(
            quote(source),
            if cfg!(windows) {
                "'name''$()`; data'"
            } else {
                "'name'\\''$()`; data'"
            }
        );
    }
}
