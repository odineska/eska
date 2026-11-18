//! The wizard gathers semantic values; the core remains the only creation validator.

use std::{
    io::{self, Write},
    ops::RangeInclusive,
};

use eska_presentation::{
    arguments::project::metadata::{
        AddArgs, CreationKind as ArgumentKind, DateFractions as ArgumentDate,
    },
    localization::{LocalizationValue, Localizer},
};
use eska_project_core::project::{
    configurator::TreeOptions,
    metadata_create::{CreateRequest, CreateType, CreationSchema, DateFractions, capabilities},
    metadata_model::{CollectionKind, MetadataKind, NodeId},
    metadata_workspace::ProjectSession,
};
use eska_terminal::interactive::confirm_overwrite;
use serde_json::Value;

use crate::ide::{creation as shared, editing};

use super::{arguments, failure};

pub(super) fn request(
    project: &mut ProjectSession,
    args: &AddArgs,
    locale: &Localizer,
) -> Result<CreateRequest, Value> {
    let mut filled = args.clone();
    if filled.kind.is_none() {
        let value = choose(
            locale,
            "metadata-create-kind-prompt",
            &[
                ("attribute".into(), locale.text("platform-kind-attribute")),
                (
                    "table-part".into(),
                    locale.text("platform-kind-tabular-section"),
                ),
            ],
            None,
        )?;
        filled.kind = Some(if value == "attribute" {
            ArgumentKind::Attribute
        } else {
            ArgumentKind::TablePart
        });
    }
    if filled.owner.is_none() {
        filled.owner = Some(owner(project, &filled, locale)?);
    }
    let id = arguments::object_id(
        filled
            .owner
            .as_deref()
            .ok_or_else(|| arguments::required(&["owner"]))?,
    )?;
    let schema = project.creation_schema(&id).map_err(shared::failure)?;
    if !schema.writable {
        return Err(failure("property_read_only"));
    }
    if filled.name.is_none() {
        filled.name = Some(required_text(locale, "metadata-create-name-prompt")?);
    }
    if filled.kind == Some(ArgumentKind::Attribute) {
        fill_type(&mut filled, &schema, locale)?;
    }
    if filled.synonym.is_empty() && !schema.languages.is_empty() {
        let synonym = line(locale, "metadata-create-synonym-prompt", None)?;
        if !synonym.is_empty() {
            let languages: Vec<_> = schema
                .languages
                .iter()
                .map(|language| (language.code.clone(), language.name.clone()))
                .collect();
            let language = if languages.len() == 1 {
                languages[0].0.clone()
            } else {
                choose(locale, "metadata-create-language-prompt", &languages, None)?
            };
            filled.synonym.push(format!("{language}={synonym}"));
        }
    }
    if filled.comment.is_none() {
        filled.comment = Some(line(locale, "metadata-create-comment-prompt", None)?);
    }
    arguments::request(&filled, &schema)
}

/// Root discovery reads declaration lists; only the selected descriptor is opened for table parts.
fn owner(
    project: &mut ProjectSession,
    args: &AddArgs,
    locale: &Localizer,
) -> Result<String, Value> {
    let kinds = capabilities().owner_kinds;
    let mut pending = vec![project.root().clone()];
    let mut choices = Vec::new();
    while let Some(node) = pending.pop() {
        if let NodeId::Object(id) = &node {
            let summary = project
                .object(id)
                .map_err(|error| editing::failure(error.into()))?;
            if kinds.contains(&summary.kind) {
                choices.push((
                    id.to_string(),
                    owner_caption(summary.kind, &summary.name, locale),
                ));
                continue;
            }
        }
        let children = project
            .children(&node, TreeOptions::default())
            .map_err(|error| editing::failure(error.into()))?;
        for child in children {
            match &child.id {
                NodeId::Object(_)
                    if child
                        .metadata_kind
                        .is_some_and(|kind| kinds.contains(&kind)) =>
                {
                    pending.push(child.id.clone());
                }
                NodeId::Collection {
                    kind: CollectionKind::Common,
                    ..
                } => pending.push(child.id.clone()),
                NodeId::Collection {
                    kind: CollectionKind::Metadata(kind),
                    ..
                } if kinds.contains(kind) => {
                    pending.push(child.id.clone());
                }
                _ => {}
            }
        }
    }
    if choices.is_empty() {
        return Err(failure("creation_no_owners"));
    }
    choices.sort_by(|left, right| left.1.cmp(&right.1));
    let selected = choose(locale, "metadata-create-owner-prompt", &choices, None)?;
    if args.kind != Some(ArgumentKind::Attribute) {
        return Ok(selected);
    }
    let id = arguments::object_id(&selected)?;
    let mut owners = choices
        .into_iter()
        .filter(|choice| choice.0 == selected)
        .collect::<Vec<_>>();
    let children = project
        .children(&NodeId::Object(id), TreeOptions::default())
        .map_err(|error| editing::failure(error.into()))?;
    let groups: Vec<_> = children
        .iter()
        .filter(|node| {
            matches!(
                node.id,
                NodeId::Collection {
                    kind: CollectionKind::Metadata(MetadataKind::TabularSection),
                    ..
                }
            )
        })
        .map(|node| node.id.clone())
        .collect();
    for group in groups {
        let children = project
            .children(&group, TreeOptions::default())
            .map_err(|error| editing::failure(error.into()))?
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        for child in children {
            if let NodeId::Object(id) = &child.id {
                let summary = project
                    .object(id)
                    .map_err(|error| editing::failure(error.into()))?;
                owners.push((
                    id.to_string(),
                    owner_caption(summary.kind, &summary.name, locale),
                ));
            }
        }
    }
    if owners.len() == 1 {
        Ok(selected)
    } else {
        choose(
            locale,
            "metadata-create-owner-prompt",
            &owners,
            Some(&selected),
        )
    }
}

fn owner_caption(kind: MetadataKind, name: &str, locale: &Localizer) -> String {
    format!(
        "{}: {name}",
        locale.text(&format!("platform-kind-{}", kind.as_str()))
    )
}

fn fill_type(args: &mut AddArgs, schema: &CreationSchema, locale: &Localizer) -> Result<(), Value> {
    if args.type_args.value_type.is_none() {
        let mut choices: Vec<_> = schema
            .capabilities
            .primitive_types
            .iter()
            .map(|choice| {
                (
                    arguments::type_name(&choice.default).to_owned(),
                    locale
                        .type_caption(&choice.key)
                        .unwrap_or_else(|| choice.key.name.clone()),
                )
            })
            .collect();
        if !schema.references.is_empty() {
            choices.push((
                "reference".into(),
                locale.text("metadata-create-reference-prompt"),
            ));
        }
        args.type_args.value_type = Some(choose(
            locale,
            "metadata-create-type-prompt",
            &choices,
            None,
        )?);
    }
    let selected = args
        .type_args
        .value_type
        .as_deref()
        .ok_or_else(|| arguments::required(&["type"]))?;
    if selected == "reference" {
        if args.type_args.reference.is_none() {
            let choices: Vec<_> = schema
                .references
                .iter()
                .map(|reference| {
                    (
                        reference.object.to_string(),
                        owner_caption(reference.kind, &reference.name, locale),
                    )
                })
                .collect();
            args.type_args.reference = Some(choose(
                locale,
                "metadata-create-reference-prompt",
                &choices,
                None,
            )?);
        }
        return Ok(());
    }
    let default = schema
        .capabilities
        .primitive_types
        .iter()
        .find(|choice| arguments::type_name(&choice.default) == selected)
        .map(|choice| &choice.default)
        .ok_or_else(|| super::input_error("type", "unsupported_type"))?;
    match default {
        CreateType::String { length, fixed } => fill_string(args, schema, locale, *length, *fixed),
        CreateType::Number {
            digits,
            fraction_digits,
            non_negative,
        } => fill_number(
            args,
            schema,
            locale,
            *digits,
            *fraction_digits,
            *non_negative,
        ),
        CreateType::Date { fractions } if args.type_args.date_fractions.is_none() => {
            fill_date(args, locale, *fractions)
        }
        CreateType::Boolean { .. } | CreateType::Date { .. } | CreateType::Reference { .. } => {
            Ok(())
        }
    }
}

fn fill_string(
    args: &mut AddArgs,
    schema: &CreationSchema,
    locale: &Localizer,
    length: u32,
    fixed: bool,
) -> Result<(), Value> {
    if !args.type_args.fixed_length {
        args.type_args.fixed_length = enum_choice(
            locale,
            "platform-property-core-AllowedLength",
            "AllowedLength",
            &["Variable", "Fixed"],
            if fixed { "Fixed" } else { "Variable" },
        )? == "Fixed";
    }
    if args.type_args.length.is_none() {
        let limits = &schema.capabilities.limits;
        let (min, max) = if args.type_args.fixed_length {
            (
                limits.string_fixed_length_min,
                limits.string_fixed_length_max,
            )
        } else {
            (limits.string_length_min, limits.string_length_max)
        };
        args.type_args.length = Some(number(
            locale,
            "platform-property-core-Length",
            length.clamp(min, max),
            min..=max,
        )?);
    }
    Ok(())
}

fn fill_number(
    args: &mut AddArgs,
    schema: &CreationSchema,
    locale: &Localizer,
    digits: u8,
    fraction_digits: u8,
    non_negative: bool,
) -> Result<(), Value> {
    if args.type_args.digits.is_none() {
        args.type_args.digits = Some(number(
            locale,
            "platform-property-core-Digits",
            u32::from(digits),
            u32::from(schema.capabilities.limits.number_digits_min)
                ..=u32::from(schema.capabilities.limits.number_digits_max),
        )?);
    }
    let selected_digits = args.type_args.digits.unwrap_or_else(|| u32::from(digits));
    if args.type_args.fraction_digits.is_none() {
        args.type_args.fraction_digits = Some(number(
            locale,
            "platform-property-core-FractionDigits",
            u32::from(fraction_digits).min(selected_digits),
            0..=selected_digits,
        )?);
    }
    if !args.type_args.nonnegative {
        args.type_args.nonnegative = enum_choice(
            locale,
            "platform-property-core-AllowedSign",
            "AllowedSign",
            &["Any", "Nonnegative"],
            if non_negative { "Nonnegative" } else { "Any" },
        )? == "Nonnegative";
    }
    Ok(())
}

fn fill_date(
    args: &mut AddArgs,
    locale: &Localizer,
    fractions: DateFractions,
) -> Result<(), Value> {
    let default = match fractions {
        DateFractions::Date => "Date",
        DateFractions::Time => "Time",
        DateFractions::DateTime => "DateTime",
    };
    let selected = enum_choice(
        locale,
        "platform-property-core-DateFractions",
        "DateFractions",
        &["Date", "Time", "DateTime"],
        default,
    )?;
    args.type_args.date_fractions = Some(match selected.as_str() {
        "Date" => ArgumentDate::Date,
        "Time" => ArgumentDate::Time,
        _ => ArgumentDate::DateTime,
    });
    Ok(())
}

fn enum_choice(
    locale: &Localizer,
    label: &str,
    domain: &str,
    values: &[&str],
    default: &str,
) -> Result<String, Value> {
    let choices: Vec<_> = values
        .iter()
        .map(|value| {
            (
                (*value).to_owned(),
                locale
                    .enum_caption(domain, value)
                    .unwrap_or_else(|| (*value).to_owned()),
            )
        })
        .collect();
    choose(locale, label, &choices, Some(default))
}

fn number(
    locale: &Localizer,
    label: &str,
    default: u32,
    range: RangeInclusive<u32>,
) -> Result<u32, Value> {
    loop {
        let value = line(locale, label, Some(&default.to_string()))?;
        if let Ok(value) = value.parse()
            && range.contains(&value)
        {
            return Ok(value);
        }
        eprintln!(
            "{}",
            locale.format(
                "metadata-create-number-invalid",
                &[
                    ("min", LocalizationValue::Number(i64::from(*range.start()))),
                    ("max", LocalizationValue::Number(i64::from(*range.end()))),
                ]
            )
        );
    }
}

pub(in crate::metadata) fn required_text(locale: &Localizer, label: &str) -> Result<String, Value> {
    loop {
        let value = line(locale, label, None)?;
        if eska_project_core::project::metadata_rename::validate_name(&value).is_ok() {
            return Ok(value);
        }
        eprintln!("{}", locale.text("metadata-create-invalid-name"));
    }
}

pub(in crate::metadata) fn line(
    locale: &Localizer,
    label: &str,
    default: Option<&str>,
) -> Result<String, Value> {
    let default_label = default.map_or_default(|value| {
        locale.format(
            "metadata-create-default",
            &[("value", LocalizationValue::Text(value))],
        )
    });
    let prompt = locale.format(
        "metadata-create-input-prompt",
        &[
            ("label", LocalizationValue::Text(&locale.text(label))),
            ("default", LocalizationValue::Text(&default_label)),
        ],
    );
    write!(io::stderr(), "{prompt} ")
        .and_then(|()| io::stderr().flush())
        .map_err(|_| failure("creation_prompt_failed"))?;
    let mut value = String::new();
    if io::stdin()
        .read_line(&mut value)
        .map_err(|_| failure("creation_prompt_failed"))?
        == 0
    {
        return Err(failure("creation_cancelled"));
    }
    let value = value.trim_end_matches(['\r', '\n']);
    if value == "/cancel" {
        return Err(failure("creation_cancelled"));
    }
    Ok(if value.is_empty() {
        default.unwrap_or_default()
    } else {
        value
    }
    .to_owned())
}

pub(in crate::metadata) fn choose(
    locale: &Localizer,
    label: &str,
    choices: &[(String, String)],
    default: Option<&str>,
) -> Result<String, Value> {
    if choices.is_empty() {
        return Err(failure("creation_empty_choices"));
    }
    let mut filter = String::new();
    loop {
        let matches: Vec<_> = choices
            .iter()
            .filter(|(value, caption)| {
                value.to_lowercase().contains(&filter) || caption.to_lowercase().contains(&filter)
            })
            .take(20)
            .collect();
        eprintln!("{}:", locale.text(label));
        for (index, (value, caption)) in matches.iter().enumerate() {
            eprintln!("  {}. {caption} ({value})", index + 1);
        }
        if choices.len() > 20 {
            eprintln!("{}", locale.text("metadata-create-search-prompt"));
        }
        let default_caption = default
            .and_then(|default| choices.iter().find(|choice| choice.0 == default))
            .map(|choice| choice.1.as_str());
        let answer = line(locale, "metadata-create-choice-prompt", default_caption)?;
        if let Some(value) = choices
            .iter()
            .find(|choice| choice.0 == answer || choice.1 == answer)
        {
            return Ok(value.0.clone());
        }
        if let Ok(index) = answer.parse::<usize>() {
            if let Some((value, _)) = index.checked_sub(1).and_then(|index| matches.get(index)) {
                return Ok(value.clone());
            }
            eprintln!("{}", locale.text("metadata-create-choice-invalid"));
        } else {
            filter = answer.to_lowercase();
        }
    }
}

pub(in crate::metadata) fn confirm(locale: &Localizer) -> Result<bool, Value> {
    eprintln!("{}", locale.text("metadata-create-confirm"));
    let result = confirm_overwrite(|| false).map_err(|_| failure("creation_prompt_failed"));
    eprintln!();
    result
}
