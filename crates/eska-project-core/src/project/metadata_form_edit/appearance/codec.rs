use std::{collections::HashSet, fmt::Write};

use roxmltree::Node;

use super::{
    AppearanceCondition, AppearanceField, AppearanceFont, AppearanceGroup, AppearanceOperand,
    AppearanceOperator, AppearanceParameter, AppearanceRule, AppearanceSetting, AppearanceValue,
    CORE, DCS, FontEffect, FormEditError, MAX_ROWS, SETTINGS, UI, XS, XSI, invalid, simple_text,
};
use crate::project::{
    metadata_edit::{escape_attribute, escape_xml},
    metadata_model::LocalizedText,
};

const MAX_GROUP_DEPTH: usize = 16;
const PREFIXES: &[(&str, &str)] = &[
    ("dcsset", SETTINGS),
    ("dcscor", DCS),
    ("v8", CORE),
    ("v8ui", UI),
    ("xs", XS),
    ("xsi", XSI),
    ("style", "http://v8.1c.ru/8.1/data/ui/style"),
    ("sys", "http://v8.1c.ru/8.1/data/ui/fonts/system"),
    ("web", "http://v8.1c.ru/8.1/data/ui/colors/web"),
    ("win", "http://v8.1c.ru/8.1/data/ui/colors/windows"),
];

pub(super) fn typed(node: Node<'_, '_>, namespace: &str, name: &str) -> bool {
    node.attribute((XSI, "type")).is_some_and(|value| {
        let (prefix, local) = value
            .split_once(':')
            .map_or((None, value), |(p, n)| (Some(p), n));
        local == name && node.lookup_namespace_uri(prefix) == Some(namespace)
    })
}

fn only_type(node: Node<'_, '_>) -> bool {
    node.attributes()
        .all(|attribute| attribute.namespace() == Some(XSI) && attribute.name() == "type")
}

fn children(node: Node<'_, '_>, namespace: &str, names: &[&str]) -> bool {
    node.children().all(|child| {
        if child.is_element() {
            child.tag_name().namespace() == Some(namespace)
                && names.contains(&child.tag_name().name())
        } else {
            child
                .text()
                .is_some_and(|text| child.is_text() && text.trim().is_empty())
        }
    })
}

fn one<'a, 'input>(
    node: Node<'a, 'input>,
    namespace: &str,
    name: &str,
) -> Result<Option<Node<'a, 'input>>, FormEditError> {
    let mut items = node
        .children()
        .filter(|item| item.has_tag_name((namespace, name)));
    let child = items.next();
    if items.next().is_some() {
        return Err(invalid("conditionalAppearance", "duplicate_section"));
    }
    Ok(child)
}

fn scalar(node: Node<'_, '_>) -> Option<String> {
    node.children()
        .all(|child| child.is_text())
        .then(|| simple_text(node))
        .flatten()
}

fn text(node: Node<'_, '_>, namespace: &str, name: &str) -> Option<String> {
    let child = one(node, namespace, name).ok()??;
    (child.attributes().len() == 0)
        .then(|| scalar(child))
        .flatten()
}

fn boolean(text: &str) -> Option<bool> {
    match text.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn enabled(node: Node<'_, '_>, namespace: &str) -> Option<bool> {
    match one(node, namespace, "use").ok()? {
        None => Some(true),
        Some(child) if child.attributes().len() == 0 => boolean(&scalar(child)?),
        _ => None,
    }
}

pub(super) fn rule_enabled(node: Node<'_, '_>) -> Option<bool> {
    enabled(node, SETTINGS)
}

fn list<T>(
    node: Node<'_, '_>,
    namespace: &str,
    mut parse: impl FnMut(Node<'_, '_>) -> Option<T>,
) -> Option<Vec<T>> {
    if node.attributes().len() != 0 || !children(node, namespace, &["item"]) {
        return None;
    }
    let items: Vec<_> = node
        .children()
        .filter(Node::is_element)
        .take(MAX_ROWS + 1)
        .map(&mut parse)
        .collect::<Option<_>>()?;
    (items.len() <= MAX_ROWS).then_some(items)
}

pub(super) fn fields(node: Node<'_, '_>) -> Option<Vec<AppearanceField>> {
    let mut seen = HashSet::new();
    list(node, SETTINGS, |item| {
        if item.attributes().len() != 0 || !children(item, SETTINGS, &["use", "field"]) {
            return None;
        }
        let name = text(item, SETTINGS, "field")?;
        if !seen.insert(name.clone()) {
            return None;
        }
        Some(AppearanceField {
            name,
            enabled: enabled(item, SETTINGS)?,
        })
    })
}

pub(super) fn conditions(node: Node<'_, '_>) -> Option<Vec<AppearanceCondition>> {
    let mut count = 0;
    list(node, SETTINGS, |item| condition(item, 0, &mut count))
}

fn condition(node: Node<'_, '_>, depth: usize, count: &mut usize) -> Option<AppearanceCondition> {
    *count += 1;
    if depth > MAX_GROUP_DEPTH || *count > MAX_ROWS || !only_type(node) {
        return None;
    }
    let enabled = enabled(node, SETTINGS)?;
    if typed(node, SETTINGS, "FilterItemGroup")
        && children(node, SETTINGS, &["use", "groupType", "item"])
    {
        let group = match text(node, SETTINGS, "groupType")?.as_str() {
            "AndGroup" => AppearanceGroup::And,
            "OrGroup" => AppearanceGroup::Or,
            "NotGroup" => AppearanceGroup::Not,
            _ => return None,
        };
        let items = node
            .children()
            .filter(|item| item.has_tag_name((SETTINGS, "item")))
            .map(|item| condition(item, depth + 1, count))
            .collect::<Option<_>>()?;
        return Some(AppearanceCondition::Group {
            enabled,
            group,
            items,
        });
    }
    if !typed(node, SETTINGS, "FilterItemComparison")
        || !children(node, SETTINGS, &["use", "left", "comparisonType", "right"])
    {
        return None;
    }
    let left = one(node, SETTINGS, "left").ok()??;
    if !typed(left, DCS, "Field") || !only_type(left) {
        return None;
    }
    let field = scalar(left)?;
    let comparison = text(node, SETTINGS, "comparisonType")?;
    let operator = AppearanceOperator::ALL
        .into_iter()
        .find(|operator| operator.token() == comparison)?;
    let operand = match one(node, SETTINGS, "right").ok()? {
        Some(right) if operator.needs_operand() => Some(read_operand(right)?),
        None if !operator.needs_operand() => None,
        _ => return None,
    };
    Some(AppearanceCondition::Comparison {
        enabled,
        field,
        operator,
        operand,
    })
}

fn read_operand(node: Node<'_, '_>) -> Option<AppearanceOperand> {
    if !only_type(node) {
        return None;
    }
    let value = scalar(node)?;
    if typed(node, DCS, "Field") {
        Some(AppearanceOperand::Field(value))
    } else if typed(node, DCS, "DesignTimeValue") {
        Some(AppearanceOperand::Reference(value))
    } else if typed(node, XS, "string") {
        Some(AppearanceOperand::String(value))
    } else if typed(node, XS, "decimal") && decimal(&value) {
        Some(AppearanceOperand::Number(value))
    } else if typed(node, XS, "boolean") {
        Some(AppearanceOperand::Boolean(boolean(&value)?))
    } else {
        None
    }
}

pub(super) fn settings(node: Node<'_, '_>) -> Option<Vec<AppearanceSetting>> {
    let mut seen = HashSet::new();
    list(node, DCS, |item| {
        if !only_type(item)
            || !typed(item, SETTINGS, "SettingsParameterValue")
            || !children(item, DCS, &["use", "parameter", "value"])
        {
            return None;
        }
        let name = text(item, DCS, "parameter")?;
        let parameter = AppearanceParameter::ALL.into_iter().find(|value| {
            let (ru, en) = value.names();
            name == ru || name == en
        })?;
        if !seen.insert(parameter) {
            return None;
        }
        let value = one(item, DCS, "value").ok()??;
        let value = match parameter.editor() {
            "color" if only_type(value) && typed(value, UI, "Color") => {
                AppearanceValue::Color(color(value, &scalar(value)?)?)
            }
            "font" if typed(value, UI, "Font") => AppearanceValue::Font(font(value)?),
            "localized" if only_type(value) && typed(value, CORE, "LocalStringType") => {
                AppearanceValue::Localized(localized(value)?)
            }
            "boolean" if only_type(value) && typed(value, XS, "boolean") => {
                AppearanceValue::Boolean(boolean(&scalar(value)?)?)
            }
            _ => return None,
        };
        Some(AppearanceSetting {
            parameter,
            enabled: enabled(item, DCS)?,
            value,
        })
    })
}

fn localized(node: Node<'_, '_>) -> Option<Vec<LocalizedText>> {
    if !children(node, CORE, &["item"]) {
        return None;
    }
    let mut seen = HashSet::new();
    node.children()
        .filter(Node::is_element)
        .map(|item| {
            if item.attributes().len() != 0 || !children(item, CORE, &["lang", "content"]) {
                return None;
            }
            let language = text(item, CORE, "lang")?;
            if !seen.insert(language.clone()) {
                return None;
            }
            Some(LocalizedText {
                language,
                content: text(item, CORE, "content")?,
            })
        })
        .collect()
}

fn font(node: Node<'_, '_>) -> Option<AppearanceFont> {
    if !children(node, UI, &[])
        || !node
            .attributes()
            .all(|attribute| match attribute.namespace() {
                Some(XSI) => attribute.name() == "type",
                None => [
                    "ref",
                    "kind",
                    "bold",
                    "italic",
                    "underline",
                    "strikeout",
                    "scale",
                ]
                .contains(&attribute.name()),
                _ => false,
            })
    {
        return None;
    }
    let reference = reference(node, node.attribute("ref")?, &["sys", "style"])?;
    match (node.attribute("kind")?, reference.split_once(':')?.0) {
        ("WindowsFont", "sys") | ("StyleItem", "style") => (),
        _ => return None,
    }
    let mut effects = Vec::new();
    for (effect, tag) in FontEffect::ALL {
        if let Some(flag) = node.attribute(tag)
            && boolean(flag)?
        {
            effects.push(effect);
        }
    }
    let scale = node
        .attribute("scale")
        .map(str::parse::<u16>)
        .transpose()
        .ok()?;
    Some(AppearanceFont {
        reference,
        effects,
        scale,
    })
}

fn reference(node: Node<'_, '_>, value: &str, allowed: &[&str]) -> Option<String> {
    let (prefix, name) = value.split_once(':')?;
    let uri = node.lookup_namespace_uri(Some(prefix))?;
    let (prefix, _) = PREFIXES
        .iter()
        .find(|(prefix, namespace)| allowed.contains(prefix) && *namespace == uri)?;
    valid_name(name).then(|| format!("{prefix}:{name}"))
}

fn color(node: Node<'_, '_>, value: &str) -> Option<String> {
    if raw_color(value) {
        Some(value.to_owned())
    } else {
        reference(node, value, &["style", "web", "win"])
    }
}

fn raw_color(value: &str) -> bool {
    value == "auto"
        || value
            .strip_prefix('#')
            .is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|c| c.is_ascii_hexdigit()))
}

fn valid_name(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn path(value: &str) -> bool {
    value.len() <= 2048 && value.split('.').all(valid_name)
}

fn decimal(value: &str) -> bool {
    let value = value.strip_prefix(['-', '+']).unwrap_or(value);
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().any(|c| c.is_ascii_digit())
        && value.split('.').count() <= 2
        && value.bytes().all(|c| c.is_ascii_digit() || c == b'.')
}

impl AppearanceOperator {
    const fn token(self) -> &'static str {
        match self {
            Self::Equal => "Equal",
            Self::NotEqual => "NotEqual",
            Self::Greater => "Greater",
            Self::GreaterOrEqual => "GreaterOrEqual",
            Self::Less => "Less",
            Self::LessOrEqual => "LessOrEqual",
            Self::Filled => "Filled",
            Self::NotFilled => "NotFilled",
        }
    }
}

fn fragment(scope: Node<'_, '_>, tag: &str, content: &str) -> String {
    let mut declarations = String::new();
    for &(prefix, namespace) in PREFIXES {
        // QName-valued attributes and scalar colors/fonts need bindings too.
        if scope.lookup_namespace_uri(Some(prefix)) != Some(namespace)
            && (tag.contains(&format!("{prefix}:")) || content.contains(&format!("{prefix}:")))
        {
            let _ = write!(declarations, " xmlns:{prefix}=\"{namespace}\"");
        }
    }
    format!("<{tag}{declarations}>{content}</{tag}>")
}

fn use_value(prefix: &str, enabled: bool) -> String {
    if enabled {
        String::new()
    } else {
        format!("<{prefix}:use>false</{prefix}:use>")
    }
}

pub(super) fn render_enabled(scope: Node<'_, '_>, enabled: bool) -> String {
    fragment(scope, "dcsset:use", if enabled { "true" } else { "false" })
}

pub(super) fn new_rule(
    scope: Node<'_, '_>,
    rule: &AppearanceRule,
) -> Result<String, FormEditError> {
    let fields = rule
        .fields
        .as_deref()
        .ok_or(invalid("conditionalAppearance", "missing_fields"))?;
    let format = rule
        .format
        .as_deref()
        .ok_or(invalid("conditionalAppearance", "missing_format"))?;
    if format.is_empty() {
        return Err(invalid("conditionalAppearance", "missing_format"));
    }
    let conditions = rule.conditions.as_deref().unwrap_or_default();
    // Child fragments already establish any namespaces missing from the form.
    let content = format!(
        "{}{}{}{}",
        if rule.enabled == Some(false) {
            render_enabled(scope, false)
        } else {
            String::new()
        },
        render_fields(scope, fields)?,
        render_conditions(scope, conditions)?,
        render_settings(scope, format)?
    );
    // Declare only the rule tag's prefix here; the child fragments manage their own bindings.
    if scope.lookup_namespace_uri(Some("dcsset")) == Some(SETTINGS) {
        Ok(format!("<dcsset:item>{content}</dcsset:item>"))
    } else {
        Ok(format!(
            "<dcsset:item xmlns:dcsset=\"{SETTINGS}\">{content}</dcsset:item>"
        ))
    }
}

pub(super) fn render_fields(
    scope: Node<'_, '_>,
    fields: &[AppearanceField],
) -> Result<String, FormEditError> {
    let mut content = String::new();
    for field in fields {
        let name = escape_xml(&field.name)?;
        let enabled = use_value("dcsset", field.enabled);
        let _ = write!(
            content,
            "<dcsset:item>{enabled}<dcsset:field>{name}</dcsset:field></dcsset:item>"
        );
    }
    Ok(fragment(scope, "dcsset:selection", &content))
}

pub(super) fn render_conditions(
    scope: Node<'_, '_>,
    conditions: &[AppearanceCondition],
) -> Result<String, FormEditError> {
    let mut content = String::new();
    let mut count = 0;
    for item in conditions {
        render_condition(&mut content, item, 0, &mut count)?;
    }
    Ok(fragment(scope, "dcsset:filter", &content))
}

fn render_condition(
    output: &mut String,
    condition: &AppearanceCondition,
    depth: usize,
    count: &mut usize,
) -> Result<(), FormEditError> {
    *count += 1;
    if depth > MAX_GROUP_DEPTH || *count > MAX_ROWS {
        return Err(invalid("conditionalAppearance", "too_many_conditions"));
    }
    match condition {
        AppearanceCondition::Comparison {
            enabled,
            field,
            operator,
            operand,
        } => {
            if !path(field) || operator.needs_operand() != operand.is_some() {
                return Err(invalid("conditionalAppearance", "invalid_comparison"));
            }
            let use_tag = use_value("dcsset", *enabled);
            let field = escape_xml(field)?;
            let operator = operator.token();
            let _ = write!(
                output,
                "<dcsset:item xsi:type=\"dcsset:FilterItemComparison\">{use_tag}<dcsset:left xsi:type=\"dcscor:Field\">{field}</dcsset:left><dcsset:comparisonType>{operator}</dcsset:comparisonType>"
            );
            if let Some(operand) = operand {
                let (kind, value) = match operand {
                    AppearanceOperand::Field(value) if path(value) => {
                        ("dcscor:Field", escape_xml(value)?)
                    }
                    AppearanceOperand::Reference(value) if path(value) => {
                        ("dcscor:DesignTimeValue", escape_xml(value)?)
                    }
                    AppearanceOperand::Number(value) if decimal(value) => {
                        ("xs:decimal", value.clone())
                    }
                    AppearanceOperand::String(value) => ("xs:string", escape_xml(value)?),
                    AppearanceOperand::Boolean(value) => ("xs:boolean", value.to_string()),
                    _ => return Err(invalid("conditionalAppearance", "invalid_operand")),
                };
                let _ = write!(
                    output,
                    "<dcsset:right xsi:type=\"{kind}\">{value}</dcsset:right>"
                );
            }
        }
        AppearanceCondition::Group {
            enabled,
            group,
            items,
        } => {
            if items.is_empty() {
                return Err(invalid("conditionalAppearance", "empty_group"));
            }
            let use_tag = use_value("dcsset", *enabled);
            let group = match group {
                AppearanceGroup::And => "AndGroup",
                AppearanceGroup::Or => "OrGroup",
                AppearanceGroup::Not => "NotGroup",
            };
            let _ = write!(
                output,
                "<dcsset:item xsi:type=\"dcsset:FilterItemGroup\">{use_tag}<dcsset:groupType>{group}</dcsset:groupType>"
            );
            for item in items {
                render_condition(output, item, depth + 1, count)?;
            }
        }
    }
    output.push_str("</dcsset:item>");
    Ok(())
}

fn is_reference(value: &str, allowed: &[&str]) -> bool {
    value
        .split_once(':')
        .is_some_and(|(prefix, name)| allowed.contains(&prefix) && valid_name(name))
}

pub(super) fn render_settings(
    scope: Node<'_, '_>,
    values: &[AppearanceSetting],
) -> Result<String, FormEditError> {
    let mut used = HashSet::new();
    let mut content = String::new();
    if values.len() > AppearanceParameter::ALL.len() {
        return Err(invalid("conditionalAppearance", "too_many_parameters"));
    }
    for setting in values {
        if !used.insert(setting.parameter) {
            return Err(invalid("conditionalAppearance", "duplicate_parameter"));
        }
        let (ru, en) = setting.parameter.names();
        let previous = scope
            .children()
            .find(|node| node.has_tag_name((SETTINGS, "appearance")))
            .and_then(|node| {
                node.children().filter(Node::is_element).find_map(|item| {
                    text(item, DCS, "parameter").filter(|name| name == ru || name == en)
                })
            });
        let name = previous.as_deref().unwrap_or(ru);
        let use_tag = use_value("dcscor", setting.enabled);
        let _ = write!(
            content,
            "<dcscor:item xsi:type=\"dcsset:SettingsParameterValue\">{use_tag}<dcscor:parameter>{name}</dcscor:parameter>"
        );
        render_value(&mut content, setting)?;
        content.push_str("</dcscor:item>");
    }
    Ok(fragment(scope, "dcsset:appearance", &content))
}

fn render_value(output: &mut String, setting: &AppearanceSetting) -> Result<(), FormEditError> {
    match (setting.parameter.editor(), &setting.value) {
        ("boolean", AppearanceValue::Boolean(value)) => {
            let _ = write!(
                output,
                "<dcscor:value xsi:type=\"xs:boolean\">{value}</dcscor:value>"
            );
        }
        ("color", AppearanceValue::Color(value))
            if raw_color(value) || is_reference(value, &["style", "web", "win"]) =>
        {
            let _ = write!(
                output,
                "<dcscor:value xsi:type=\"v8ui:Color\">{}</dcscor:value>",
                escape_xml(value)?
            );
        }
        ("localized", AppearanceValue::Localized(values)) => {
            let mut seen = HashSet::new();
            output.push_str("<dcscor:value xsi:type=\"v8:LocalStringType\">");
            for value in values {
                if value.language.is_empty() || !seen.insert(&value.language) {
                    return Err(invalid(
                        "conditionalAppearance",
                        "duplicate_or_empty_language",
                    ));
                }
                let language = escape_xml(&value.language)?;
                let value = escape_xml(&value.content)?;
                let _ = write!(
                    output,
                    "<v8:item><v8:lang>{language}</v8:lang><v8:content>{value}</v8:content></v8:item>"
                );
            }
            output.push_str("</dcscor:value>");
        }
        ("font", AppearanceValue::Font(font))
            if is_reference(&font.reference, &["style", "sys"]) && font.scale != Some(0) =>
        {
            if font.effects.iter().collect::<HashSet<_>>().len() != font.effects.len() {
                return Err(invalid("conditionalAppearance", "duplicate_font_effect"));
            }
            let kind = if font.reference.starts_with("sys:") {
                "WindowsFont"
            } else {
                "StyleItem"
            };
            let reference = escape_attribute(&font.reference)?;
            let _ = write!(
                output,
                "<dcscor:value xsi:type=\"v8ui:Font\" ref=\"{reference}\""
            );
            for (effect, tag) in FontEffect::ALL {
                let _ = write!(output, " {tag}=\"{}\"", font.effects.contains(&effect));
            }
            let _ = write!(output, " kind=\"{kind}\"");
            if let Some(scale) = font.scale {
                let _ = write!(output, " scale=\"{scale}\"");
            }
            output.push_str("/>");
        }
        _ => return Err(invalid("conditionalAppearance", "invalid_format_value")),
    }
    Ok(())
}
