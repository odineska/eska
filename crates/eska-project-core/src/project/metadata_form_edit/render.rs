//! Typed values use namespace identities; source prefixes are never trusted as semantic data.

use std::{collections::BTreeMap, fmt::Write};

use roxmltree::Node;

use super::{FormEditError, invalid};
use crate::project::{
    metadata_edit::{escape_attribute, escape_xml},
    metadata_model::{MetadataProperty, MetadataValue, PropertyKey},
};

const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

pub(super) fn property(
    value: &MetadataProperty,
    scope: Node<'_, '_>,
) -> Result<String, FormEditError> {
    render(value, &Names::scope(scope), 0)
}

fn render(
    property: &MetadataProperty,
    scope: &BTreeMap<String, String>,
    depth: usize,
) -> Result<String, FormEditError> {
    if depth > 48 {
        return Err(invalid("property", "too_deep"));
    }
    let mut names = Names::new(scope);
    let tag = names.name(&property.key, false)?;
    let mut attributes = String::new();
    let mut keys = Vec::new();
    for (key, value) in &property.qualifiers {
        if keys.contains(key)
            || key.name == "xmlns"
            || key.namespace.as_deref() == Some("http://www.w3.org/2000/xmlns/")
        {
            return Err(invalid("qualifiers", "invalid_namespace_or_duplicate"));
        }
        keys.push(key.clone());
        // TypedText carries the namespace-aware type and owns its lexical rendering.
        if matches!(property.value, MetadataValue::TypedText { .. })
            && key.namespace.as_deref() == Some(XSI)
            && key.name == "type"
        {
            continue;
        }
        let name = names.name(key, true)?;
        let _ = write!(attributes, " {name}=\"{}\"", escape_attribute(value)?);
    }
    let content = match &property.value {
        MetadataValue::Text(text) => escape_xml(text)?,
        MetadataValue::QualifiedText { key, .. } => names.name(key, true)?,
        MetadataValue::TypedText { text, key } => {
            let attribute = names.name(
                &PropertyKey {
                    namespace: Some(XSI.to_owned()),
                    name: "type".to_owned(),
                },
                true,
            )?;
            let typename = names.name(key, true)?;
            let _ = write!(attributes, " {attribute}=\"{typename}\"");
            escape_xml(text)?
        }
        MetadataValue::Record(children) => children
            .iter()
            .map(|child| render(child, &names.bindings, depth + 1))
            .collect::<Result<String, _>>()?,
        MetadataValue::Localized(values) => {
            let mut languages = std::collections::HashSet::new();
            let mut content = String::new();
            for value in values {
                if value.language.trim().is_empty() || !languages.insert(&value.language) {
                    return Err(invalid("localized", "duplicate_or_empty_language"));
                }
                let item = MetadataProperty {
                    key: key(CORE, "item"),
                    qualifiers: Vec::new(),
                    value: MetadataValue::Record(vec![
                        MetadataProperty {
                            key: key(CORE, "lang"),
                            qualifiers: Vec::new(),
                            value: MetadataValue::Text(value.language.clone()),
                        },
                        MetadataProperty {
                            key: key(CORE, "content"),
                            qualifiers: Vec::new(),
                            value: MetadataValue::Text(value.content.clone()),
                        },
                    ]),
                };
                content.push_str(&render(&item, &names.bindings, depth + 1)?);
            }
            content
        }
        MetadataValue::Unsupported(_) => return Err(invalid("property", "unsupported_value")),
    };
    let declarations = names.declarations()?;
    Ok(if content.is_empty() {
        format!("<{tag}{declarations}{attributes}/>")
    } else {
        format!("<{tag}{declarations}{attributes}>{content}</{tag}>")
    })
}

pub(super) fn key(namespace: &str, name: &str) -> PropertyKey {
    PropertyKey {
        namespace: Some(namespace.to_owned()),
        name: name.to_owned(),
    }
}

pub(super) fn tag(scope: Node<'_, '_>, name: &str) -> String {
    scope
        .lookup_prefix(crate::project::forms::FORM_NAMESPACE)
        .filter(|prefix| !prefix.is_empty())
        .map_or_else(|| name.to_owned(), |prefix| format!("{prefix}:{name}"))
}

struct Names {
    bindings: BTreeMap<String, String>,
    added: BTreeMap<String, String>,
}

impl Names {
    fn new(scope: &BTreeMap<String, String>) -> Self {
        Self {
            bindings: scope.clone(),
            added: BTreeMap::new(),
        }
    }

    fn scope(scope: Node<'_, '_>) -> BTreeMap<String, String> {
        scope
            .namespaces()
            .map(|namespace| {
                (
                    namespace.name().unwrap_or_default().to_owned(),
                    namespace.uri().to_owned(),
                )
            })
            .collect()
    }

    fn declare(&mut self, prefix: String, namespace: String) {
        self.bindings.insert(prefix.clone(), namespace.clone());
        self.added.insert(prefix, namespace);
    }

    fn name(&mut self, key: &PropertyKey, qualified: bool) -> Result<String, FormEditError> {
        if !ncname(&key.name) {
            return Err(invalid("property", "invalid_xml_name"));
        }
        let Some(namespace) = key
            .namespace
            .as_deref()
            .filter(|namespace| !namespace.is_empty())
        else {
            if !qualified && self.bindings.get("").is_some_and(|value| !value.is_empty()) {
                self.declare(String::new(), String::new());
            }
            return Ok(key.name.clone());
        };
        if namespace == "http://www.w3.org/2000/xmlns/" {
            return Err(invalid("property", "invalid_namespace"));
        }
        if let Some((prefix, _)) = self
            .bindings
            .iter()
            .find(|(prefix, uri)| uri.as_str() == namespace && (!qualified || !prefix.is_empty()))
        {
            return Ok(if prefix.is_empty() {
                key.name.clone()
            } else {
                format!("{prefix}:{}", key.name)
            });
        }
        let preferred = match namespace {
            crate::project::forms::FORM_NAMESPACE => "f",
            CORE => "v8",
            XSI => "xsi",
            "http://www.w3.org/2001/XMLSchema" => "xs",
            "http://v8.1c.ru/8.1/data/enterprise/current-config" => "cfg",
            _ => "ns",
        };
        let mut prefix = preferred.to_owned();
        let mut suffix = 0;
        while self.bindings.contains_key(&prefix) {
            suffix += 1;
            prefix = format!("{preferred}{suffix}");
        }
        self.declare(prefix.clone(), namespace.to_owned());
        Ok(format!("{prefix}:{}", key.name))
    }

    fn declarations(&self) -> Result<String, FormEditError> {
        self.added
            .iter()
            .map(|(prefix, namespace)| {
                let suffix = if prefix.is_empty() {
                    String::new()
                } else {
                    format!(":{prefix}")
                };
                Ok(format!(
                    " xmlns{suffix}=\"{}\"",
                    escape_attribute(namespace)?
                ))
            })
            .collect()
    }
}

fn ncname(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}
