//! Form-level conditional appearance is stored with the form's attributes, in DCS namespaces.

mod codec;

use std::collections::HashSet;

use roxmltree::Node;
use serde::{Deserialize, Serialize};

use super::{FormEditError, document, invalid, render};
use crate::project::{
    forms::FORM_NAMESPACE,
    metadata_edit::{EditPlan, Replacement},
    metadata_model::LocalizedText,
    metadata_parser::simple_text,
};

const SETTINGS: &str = "http://v8.1c.ru/8.1/data-composition-system/settings";
const DCS: &str = "http://v8.1c.ru/8.1/data-composition-system/core";
const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const UI: &str = "http://v8.1c.ru/8.1/data/ui";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const XS: &str = "http://www.w3.org/2001/XMLSchema";
const MAX_RULES: usize = 512;
const MAX_ROWS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceRule {
    /// Original zero-based position in this snapshot. None inserts a new rule.
    pub original: Option<usize>,
    pub enabled: Option<bool>,
    pub fields: Option<Vec<AppearanceField>>,
    pub conditions: Option<Vec<AppearanceCondition>>,
    pub format: Option<Vec<AppearanceSetting>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceField {
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AppearanceCondition {
    Comparison {
        enabled: bool,
        field: String,
        operator: AppearanceOperator,
        operand: Option<AppearanceOperand>,
    },
    Group {
        enabled: bool,
        group: AppearanceGroup,
        items: Vec<Self>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum AppearanceGroup {
    #[serde(rename = "AndGroup")]
    And,
    #[serde(rename = "OrGroup")]
    Or,
    #[serde(rename = "NotGroup")]
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum AppearanceOperator {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    Filled,
    NotFilled,
}

impl AppearanceOperator {
    const ALL: [Self; 8] = [
        Self::Equal,
        Self::NotEqual,
        Self::Greater,
        Self::GreaterOrEqual,
        Self::Less,
        Self::LessOrEqual,
        Self::Filled,
        Self::NotFilled,
    ];

    const fn needs_operand(self) -> bool {
        !matches!(self, Self::Filled | Self::NotFilled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AppearanceOperand {
    Field(String),
    String(String),
    Boolean(bool),
    Number(String),
    Reference(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceSetting {
    /// Language-independent parameter key.
    pub parameter: AppearanceParameter,
    pub enabled: bool,
    pub value: AppearanceValue,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceParameter {
    Background,
    Foreground,
    Font,
    Text,
    Visible,
    Enabled,
    ReadOnly,
    Display,
    MarkIncomplete,
}

impl AppearanceParameter {
    const ALL: [Self; 9] = [
        Self::Background,
        Self::Foreground,
        Self::Font,
        Self::Text,
        Self::Visible,
        Self::Enabled,
        Self::ReadOnly,
        Self::Display,
        Self::MarkIncomplete,
    ];

    const fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Background => ("ЦветФона", "BackColor"),
            Self::Foreground => ("ЦветТекста", "TextColor"),
            Self::Font => ("Шрифт", "Font"),
            Self::Text => ("Текст", "Text"),
            Self::Visible => ("Видимость", "Visibility"),
            Self::Enabled => ("Доступность", "Availability"),
            Self::ReadOnly => ("ТолькоПросмотр", "ReadOnly"),
            Self::Display => ("Отображать", "Show"),
            Self::MarkIncomplete => ("ОтметкаНезаполненного", "MarkIncomplete"),
        }
    }

    const fn editor(self) -> &'static str {
        match self {
            Self::Background | Self::Foreground => "color",
            Self::Font => "font",
            Self::Text => "localized",
            _ => "boolean",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AppearanceValue {
    Color(String),
    Boolean(bool),
    Localized(Vec<LocalizedText>),
    Font(AppearanceFont),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppearanceFont {
    pub reference: String,
    pub effects: Vec<FontEffect>,
    pub scale: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FontEffect {
    Bold,
    Italic,
    Underline,
    Strikeout,
}

impl FontEffect {
    const ALL: [(Self, &'static str); 4] = [
        (Self::Bold, "bold"),
        (Self::Italic, "italic"),
        (Self::Underline, "underline"),
        (Self::Strikeout, "strikeout"),
    ];
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSchema {
    pub rules: Vec<AppearanceRule>,
    pub comparisons: Vec<AppearanceComparisonOption>,
    pub parameters: Vec<AppearanceParameterOption>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceComparisonOption {
    pub operator: AppearanceOperator,
    pub needs_operand: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceParameterOption {
    pub parameter: AppearanceParameter,
    pub editor: &'static str,
}

pub(super) fn inspect(root: Node<'_, '_>) -> Option<AppearanceSchema> {
    let block = container(root).ok()?;
    let rules = rule_nodes(block)
        .ok()?
        .into_iter()
        .enumerate()
        .map(|(index, node)| read_rule(node, index))
        .collect();
    Some(AppearanceSchema {
        rules,
        comparisons: AppearanceOperator::ALL
            .into_iter()
            .map(|operator| AppearanceComparisonOption {
                operator,
                needs_operand: operator.needs_operand(),
            })
            .collect(),
        parameters: AppearanceParameter::ALL
            .into_iter()
            .map(|parameter| AppearanceParameterOption {
                parameter,
                editor: parameter.editor(),
            })
            .collect(),
    })
}

fn container<'a, 'input>(
    root: Node<'a, 'input>,
) -> Result<Option<Node<'a, 'input>>, FormEditError> {
    // Reject ambiguous and nonstandard placement without making the rest of the form read-only.
    if document::child(root, "ConditionalAppearance").is_some() {
        return Err(invalid("conditionalAppearance", "unsupported_placement"));
    }
    let Some(attrs) = document::child(root, "Attributes") else {
        return Ok(None);
    };
    let mut blocks = attrs
        .children()
        .filter(|node| node.has_tag_name((FORM_NAMESPACE, "ConditionalAppearance")));
    let result = blocks.next();
    if blocks.next().is_some() {
        return Err(invalid("conditionalAppearance", "duplicate_section"));
    }
    Ok(result)
}

fn rule_nodes<'a, 'input>(
    block: Option<Node<'a, 'input>>,
) -> Result<Vec<Node<'a, 'input>>, FormEditError> {
    let Some(block) = block else {
        return Ok(Vec::new());
    };
    let mut rules = Vec::new();
    for node in block.children() {
        if node.has_tag_name((SETTINGS, "item")) {
            rules.push(node);
        } else if node.is_element()
            || node.is_text() && !node.text().unwrap_or("").trim().is_empty()
        {
            return Err(invalid("conditionalAppearance", "unsupported_section"));
        }
        if rules.len() > MAX_RULES {
            return Err(invalid("conditionalAppearance", "too_many_rules"));
        }
    }
    Ok(rules)
}

fn read_rule(node: Node<'_, '_>, index: usize) -> AppearanceRule {
    AppearanceRule {
        original: Some(index),
        enabled: codec::rule_enabled(node),
        fields: read_section(node, "selection", codec::fields),
        conditions: read_section(node, "filter", codec::conditions),
        format: read_section(node, "appearance", codec::settings),
    }
}

fn section<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
) -> Result<Option<Node<'a, 'input>>, FormEditError> {
    let mut matches = node
        .children()
        .filter(|node| node.has_tag_name((SETTINGS, name)));
    let result = matches.next();
    if matches.next().is_some() {
        return Err(invalid("conditionalAppearance", "duplicate_section"));
    }
    Ok(result)
}

fn read_section<T>(
    node: Node<'_, '_>,
    name: &str,
    read: impl FnOnce(Node<'_, '_>) -> Option<Vec<T>>,
) -> Option<Vec<T>> {
    section(node, name)
        .ok()?
        .map_or_else(|| Some(Vec::new()), read)
}

pub(super) fn change(
    input: &str,
    root: Node<'_, '_>,
    edits: &[AppearanceRule],
) -> Result<Vec<Replacement>, FormEditError> {
    if edits.len() > MAX_RULES {
        return Err(invalid("conditionalAppearance", "too_many_rules"));
    }
    let block = container(root)?;
    let originals = rule_nodes(block)?;
    let scope = block
        .or_else(|| document::child(root, "Attributes"))
        .unwrap_or(root);
    let available: HashSet<_> = root
        .descendants()
        .filter(|node| document::is_element(*node))
        .filter_map(|node| node.attribute("name"))
        .collect();
    let mut reused = HashSet::new();
    let mut rules = Vec::with_capacity(edits.len());
    for edit in edits {
        let original = if let Some(index) = edit.original {
            if !reused.insert(index) {
                return Err(invalid("conditionalAppearance", "duplicate_rule"));
            }
            Some(
                *originals
                    .get(index)
                    .ok_or(invalid("conditionalAppearance", "unknown_rule"))?,
            )
        } else {
            None
        };
        if let Some(fields) = &edit.fields {
            let mut names = HashSet::new();
            let previous = original
                .and_then(|node| read_section(node, "selection", codec::fields))
                .unwrap_or_default();
            if fields.len() > MAX_ROWS
                || fields.iter().any(|field| {
                    !names.insert(&field.name)
                        || !(available.contains(field.name.as_str())
                            || previous.iter().any(|old| old.name == field.name))
                })
            {
                return Err(invalid("conditionalAppearance", "invalid_formatted_field"));
            }
        }
        rules.push(if let Some(original) = original {
            update_rule(input, original, edit)?
        } else {
            codec::new_rule(scope, edit)?
        });
    }

    if let Some(block) = block {
        let mut replacements: Vec<_> = originals
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let range = node.range();
                let text = rules.get(index).map_or("", String::as_str);
                (input[range.clone()] != *text).then(|| Replacement {
                    range,
                    text: text.to_owned(),
                })
            })
            .collect();
        if rules.len() > originals.len() {
            replacements.push(document::insert(
                input,
                block,
                &rules[originals.len()..].join(""),
                None,
            )?);
        }
        return Ok(replacements);
    }
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let tag = render::tag(scope, "ConditionalAppearance");
    let fragment = format!("<{tag}>{}</{tag}>", rules.join(""));
    if scope != root {
        return Ok(vec![document::insert(input, scope, &fragment, None)?]);
    }
    let tag = render::tag(root, "Attributes");
    let before = document::child(root, "Commands").or_else(|| document::child(root, "Parameters"));
    Ok(vec![document::insert(
        input,
        root,
        &format!("<{tag}>{fragment}</{tag}>"),
        before,
    )?])
}

fn update_rule(
    input: &str,
    node: Node<'_, '_>,
    edit: &AppearanceRule,
) -> Result<String, FormEditError> {
    let current = read_rule(node, edit.original.unwrap_or_default());
    let enabled = match (edit.enabled, current.enabled) {
        (Some(_), None) => return Err(invalid("conditionalAppearance", "unsupported_rule_use")),
        (Some(next), Some(previous)) if next != previous => Some(codec::render_enabled(node, next)),
        _ => None,
    };
    let order = ["use", "selection", "filter", "appearance"];
    let replacements = [
        enabled,
        section_change(
            node,
            current.fields,
            edit.fields.as_deref(),
            codec::render_fields,
        )?,
        section_change(
            node,
            current.conditions,
            edit.conditions.as_deref(),
            codec::render_conditions,
        )?,
        section_change(
            node,
            current.format,
            edit.format.as_deref(),
            codec::render_settings,
        )?,
    ];
    let mut changes = Vec::new();
    let mut append = String::new();
    for (index, text) in replacements.into_iter().enumerate() {
        let Some(text) = text else { continue };
        if let Some(existing) = section(node, order[index])? {
            changes.push(Replacement {
                range: existing.range(),
                text,
            });
        } else {
            let before = node.children().find(|child| {
                let name = child.tag_name();
                name.namespace() == Some(SETTINGS) && order[index + 1..].contains(&name.name())
            });
            if before.is_some() {
                changes.push(document::insert(input, node, &text, before)?);
            } else {
                // One insertion also handles an existing self-closing rule.
                append.push_str(&text);
            }
        }
    }
    if !append.is_empty() {
        changes.push(document::insert(input, node, &append, None)?);
    }
    let range = node.range();
    let relative = changes
        .into_iter()
        .map(|change| Replacement {
            range: change.range.start - range.start..change.range.end - range.start,
            text: change.text,
        })
        .collect();
    Ok(EditPlan::from_replacements(&input[range], relative)?
        .output()
        .to_owned())
}

fn section_change<T: PartialEq>(
    node: Node<'_, '_>,
    current: Option<Vec<T>>,
    updated: Option<&[T]>,
    render: impl FnOnce(Node<'_, '_>, &[T]) -> Result<String, FormEditError>,
) -> Result<Option<String>, FormEditError> {
    let Some(updated) = updated else {
        return Ok(None);
    };
    let current = current.ok_or(invalid("conditionalAppearance", "unsupported_rule_section"))?;
    (current != updated)
        .then(|| render(node, updated))
        .transpose()
}

/// Match both formatted element identities and typed data field operands in form-level rules.
pub(super) fn references(node: Node<'_, '_>, target: &super::FormTarget, name: &str) -> bool {
    let tag = node.tag_name();
    if tag.namespace() != Some(SETTINGS)
        || !node.ancestors().any(|parent| {
            parent.has_tag_name((FORM_NAMESPACE, "ConditionalAppearance"))
                && parent
                    .parent()
                    .is_some_and(|parent| parent.has_tag_name((FORM_NAMESPACE, "Attributes")))
        })
    {
        return false;
    }
    let Some(value) = simple_text(node) else {
        return false;
    };
    match target {
        super::FormTarget::Element { .. } => {
            tag.name() == "field" && name.to_lowercase() == value.to_lowercase()
        }
        super::FormTarget::Attribute { name } => {
            let value = value.to_lowercase();
            let name = name.to_lowercase();
            matches!(tag.name(), "left" | "right")
                && codec::typed(node, DCS, "Field")
                && (value == name
                    || value
                        .strip_prefix(&name)
                        .is_some_and(|rest| rest.starts_with('.')))
        }
        _ => false,
    }
}
