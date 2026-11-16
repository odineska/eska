//! Translate command-line intent into the same typed request used by the IDE.

use eska_presentation::arguments::project::metadata::{AddArgs, CreationKind as ArgumentKind};
use eska_project_core::project::{
    metadata_create::{CreateRequest, CreateType, CreationKind, CreationSchema, DateFractions},
    metadata_model::{LocalizedText, ObjectId},
};
use serde_json::{Value, json};

use super::input_error;

pub(super) fn object_id(value: &str) -> Result<ObjectId, Value> {
    serde_json::from_value(json!(value)).map_err(|_| input_error("owner", "invalid_object_id"))
}

pub(super) const fn kind(value: ArgumentKind) -> CreationKind {
    match value {
        ArgumentKind::Attribute => CreationKind::Attribute,
        ArgumentKind::TablePart => CreationKind::TablePart,
    }
}

pub(super) const fn type_name(value: &CreateType) -> &'static str {
    match value {
        CreateType::String { .. } => "string",
        CreateType::Number { .. } => "number",
        CreateType::Boolean { .. } => "boolean",
        CreateType::Date { .. } => "date",
        CreateType::Reference { .. } => "reference",
    }
}

/// Defaults come from the current owner schema, never from a second CLI rule table.
pub(super) fn request(args: &AddArgs, schema: &CreationSchema) -> Result<CreateRequest, Value> {
    let kind = args.kind.map(kind).ok_or_else(|| required(&["kind"]))?;
    let name = args.name.clone().ok_or_else(|| required(&["name"]))?;
    let types = if kind == CreationKind::Attribute {
        vec![value_type(args, schema)?]
    } else {
        if args.type_args.value_type.is_some() || has_qualifiers(args) {
            return Err(input_error("type", "not_applicable"));
        }
        Vec::new()
    };
    let synonyms = args
        .synonym
        .iter()
        .map(|value| {
            let (language, content) = value
                .split_once('=')
                .ok_or_else(|| input_error("synonym", "expected_language_equals_text"))?;
            Ok(LocalizedText {
                language: language.into(),
                content: content.into(),
            })
        })
        .collect::<Result<Vec<_>, Value>>()?;
    Ok(CreateRequest {
        owner: schema.owner.clone(),
        snapshot: args
            .snapshot
            .clone()
            .unwrap_or_else(|| schema.snapshot.clone()),
        kind,
        name,
        synonyms,
        comment: args.comment.clone().unwrap_or_default(),
        types,
    })
}

fn value_type(args: &AddArgs, schema: &CreationSchema) -> Result<CreateType, Value> {
    let requested = args
        .type_args
        .value_type
        .as_deref()
        .ok_or_else(|| required(&["type"]))?;
    validate_qualifier_arguments(args, requested)?;
    if requested == "reference" {
        let reference = args
            .type_args
            .reference
            .as_deref()
            .ok_or_else(|| required(&["reference"]))?;
        return Ok(CreateType::Reference {
            object: object_id(reference)?,
        });
    }
    let mut value = schema
        .capabilities
        .primitive_types
        .iter()
        .find(|choice| type_name(&choice.default) == requested)
        .map(|choice| choice.default.clone())
        .ok_or_else(|| input_error("type", "unsupported_type"))?;
    match &mut value {
        CreateType::String { length, fixed } => {
            *length = args.type_args.length.unwrap_or(*length);
            *fixed = args.type_args.fixed_length;
        }
        CreateType::Number {
            digits,
            fraction_digits,
            non_negative,
        } => {
            if let Some(value) = args.type_args.digits {
                *digits = u8::try_from(value).map_err(|_| input_error("digits", "out_of_range"))?;
            }
            if let Some(value) = args.type_args.fraction_digits {
                *fraction_digits = u8::try_from(value)
                    .map_err(|_| input_error("fractionDigits", "out_of_range"))?;
            }
            *non_negative = args.type_args.nonnegative;
        }
        CreateType::Date { fractions } => {
            if let Some(value) = args.type_args.date_fractions {
                use eska_presentation::arguments::project::metadata::DateFractions as ArgumentDate;
                *fractions = match value {
                    ArgumentDate::Date => DateFractions::Date,
                    ArgumentDate::Time => DateFractions::Time,
                    ArgumentDate::DateTime => DateFractions::DateTime,
                };
            }
        }
        CreateType::Boolean { .. } | CreateType::Reference { .. } => {}
    }
    Ok(value)
}

fn validate_qualifier_arguments(args: &AddArgs, requested: &str) -> Result<(), Value> {
    for (present, allowed, field) in [
        (
            args.type_args.length.is_some() || args.type_args.fixed_length,
            "string",
            "length",
        ),
        (
            args.type_args.digits.is_some()
                || args.type_args.fraction_digits.is_some()
                || args.type_args.nonnegative,
            "number",
            "digits",
        ),
        (
            args.type_args.date_fractions.is_some(),
            "date",
            "dateFractions",
        ),
        (args.type_args.reference.is_some(), "reference", "reference"),
    ] {
        if present && requested != allowed {
            return Err(input_error(field, "not_applicable"));
        }
    }
    Ok(())
}

const fn has_qualifiers(args: &AddArgs) -> bool {
    args.type_args.length.is_some()
        || args.type_args.fixed_length
        || args.type_args.digits.is_some()
        || args.type_args.fraction_digits.is_some()
        || args.type_args.nonnegative
        || args.type_args.date_fractions.is_some()
        || args.type_args.reference.is_some()
}

pub(super) fn missing(args: &AddArgs) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if args.kind.is_none() {
        missing.push("kind");
    }
    if args.owner.is_none() {
        missing.push("owner");
    }
    if args.name.is_none() {
        missing.push("name");
    }
    if args.kind == Some(ArgumentKind::Attribute) {
        if args.type_args.value_type.is_none() {
            missing.push("type");
        } else if args.type_args.value_type.as_deref() == Some("reference")
            && args.type_args.reference.is_none()
        {
            missing.push("reference");
        }
    }
    missing
}

pub(super) fn required(fields: &[&str]) -> Value {
    json!({"kind":"creation_arguments_required","details":{"fields":fields}})
}
