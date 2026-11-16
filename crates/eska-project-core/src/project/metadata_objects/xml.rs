//! XML identity discovery is namespace-aware; only IDs owned by a copied subtree are allocated.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use roxmltree::{Document, Node};

use super::{ObjectError, invalid};
use crate::project::{
    metadata_edit::{
        EditError, EditPlan, Replacement, escape_xml,
        types::{indentation, newline},
    },
    metadata_rename::{ReferenceRename, XmlAnalysis},
};

pub const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
pub const XR: &str = "http://v8.1c.ru/8.3/xcf/readable";
const PREDEFINED: &str = "http://v8.1c.ru/8.3/xcf/predef";
const SCHEME: &str = "http://v8.1c.ru/8.3/xcf/scheme";

pub fn document(input: &str) -> Result<Document<'_>, ObjectError> {
    Document::parse(input).map_err(|_| EditError::InvalidXml.into())
}

pub fn properties<'a>(node: Node<'a, 'a>) -> Result<Node<'a, 'a>, ObjectError> {
    node.children()
        .find(|child| child.has_tag_name((MD, "Properties")))
        .ok_or_else(|| EditError::InvalidXml.into())
}

pub fn name<'a>(node: Node<'a, 'a>) -> Result<Node<'a, 'a>, ObjectError> {
    properties(node)?
        .children()
        .find(|child| child.has_tag_name((MD, "Name")))
        .ok_or_else(|| EditError::InvalidXml.into())
}

pub fn generated(input: &str) -> Result<Vec<String>, ObjectError> {
    Ok(document(input)?
        .descendants()
        .filter(|node| node.has_tag_name((XR, "GeneratedType")))
        .filter_map(|node| node.attribute("name").map(str::to_owned))
        .collect())
}

pub fn owned_ids(
    input: &str,
    selection: Option<&Range<usize>>,
) -> Result<BTreeSet<String>, ObjectError> {
    let parsed = document(input)?;
    let mut result = BTreeSet::new();
    for node in parsed
        .descendants()
        .filter(Node::is_element)
        .filter(|node| {
            selection.is_none_or(|range| {
                range.start <= node.range().start && node.range().end <= range.end
            })
        })
    {
        let value = if node.tag_name().namespace() == Some(MD) {
            node.attribute("uuid")
        } else if node.has_tag_name((PREDEFINED, "Item")) {
            node.attribute("id")
        } else if node.tag_name().namespace() == Some(SCHEME) {
            node.attribute("uuid")
        } else if node.has_tag_name((XR, "ThisNode"))
            || (node.tag_name().namespace() == Some(XR)
                && matches!(node.tag_name().name(), "TypeId" | "ValueId")
                && node
                    .parent_element()
                    .is_some_and(|parent| parent.has_tag_name((XR, "GeneratedType"))))
        {
            node.text()
        } else {
            None
        };
        if let Some(value) = value {
            if !valid_uuid(value) {
                return Err(invalid("identity", "invalid_source_uuid"));
            }
            if value != "00000000-0000-0000-0000-000000000000" {
                result.insert(value.to_ascii_lowercase());
            }
        }
    }
    Ok(result)
}

pub fn identities(
    owned: &BTreeSet<String>,
    saved: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, ObjectError> {
    if let Some(saved) = saved {
        let keys: BTreeSet<_> = saved.keys().cloned().collect();
        let values: BTreeSet<_> = saved
            .values()
            .map(|value| value.to_ascii_lowercase())
            .collect();
        if keys != *owned
            || values.len() != saved.len()
            || values
                .iter()
                .any(|value| !valid_new_uuid(value) || owned.contains(value))
        {
            return Err(invalid("identities", "invalid_mapping"));
        }
        return Ok(saved.clone());
    }
    owned
        .iter()
        .map(|old| {
            Ok((
                old.clone(),
                crate::project::metadata_create::allocate_uuid().map_err(ObjectError::Identity)?,
            ))
        })
        .collect()
}

pub fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub fn valid_new_uuid(value: &str) -> bool {
    valid_uuid(value)
        && value.as_bytes()[14] == b'4'
        && matches!(value.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
}

/// Metadata paths/types are resolved by the same engine used by rename; free strings remain untouched.
pub fn copy_xml(
    input: &str,
    rename: &ReferenceRename,
    declarations: &[Range<usize>],
    identities: &BTreeMap<String, String>,
) -> Result<(String, XmlAnalysis), ObjectError> {
    let analysis = rename.analyze_xml(input, declarations)?;
    let mut changes: Vec<_> = analysis
        .replacements
        .iter()
        .map(|change| Replacement {
            range: change.range.clone(),
            text: change.after.clone(),
        })
        .collect();
    let parsed = document(input)?;
    for node in parsed.descendants().filter(Node::is_element) {
        for attribute in node.attributes() {
            if let Some(value) = identities.get(&attribute.value().to_ascii_lowercase()) {
                changes.push(Replacement {
                    range: attribute.range_value(),
                    text: value.clone(),
                });
            }
        }
        if node.children().any(|child| child.is_element())
            || matches!(
                node.tag_name().name(),
                "Comment" | "content" | "Title" | "ToolTip"
            )
        {
            continue;
        }
        for text in node.children().filter(Node::is_text) {
            if let Some(value) = text
                .text()
                .and_then(|value| identities.get(&value.to_ascii_lowercase()))
            {
                changes.push(Replacement {
                    range: text.range(),
                    text: value.clone(),
                });
            }
        }
    }
    let result = EditPlan::from_replacements(input, changes)?
        .output()
        .to_owned();
    document(&result)?;
    Ok((result, analysis))
}

/// Parent declarations are small semantic references; copying does not rewrite unrelated root settings.
pub fn add_reference(
    input: &str,
    parent_range: Range<usize>,
    tag: &str,
    name: &str,
) -> Result<String, ObjectError> {
    let parsed = document(input)?;
    let parent = parsed
        .descendants()
        .find(|node| node.is_element() && node.range() == parent_range)
        .ok_or(EditError::Conflict)?;
    let children = parent
        .children()
        .find(|node| node.has_tag_name((MD, "ChildObjects")));
    let scope = children.unwrap_or(parent);
    let prefix = if scope.default_namespace() == Some(MD) {
        String::new()
    } else {
        scope
            .lookup_prefix(MD)
            .map(|prefix| format!("{prefix}:"))
            .ok_or(EditError::InvalidXml)?
    };
    let fragment = format!("<{prefix}{tag}>{}</{prefix}{tag}>", escape_xml(name)?);
    insert_child(input, parent_range, "ChildObjects", &fragment)
}

pub fn insert_child(
    input: &str,
    parent_range: Range<usize>,
    group: &str,
    fragment: &str,
) -> Result<String, ObjectError> {
    let parsed = document(input)?;
    let parent = parsed
        .descendants()
        .find(|node| node.is_element() && node.range() == parent_range)
        .ok_or(EditError::Conflict)?;
    let children = parent
        .children()
        .find(|node| node.has_tag_name((MD, group)));
    let prefix = if parent.default_namespace() == Some(MD) {
        String::new()
    } else {
        parent
            .lookup_prefix(MD)
            .map(|prefix| format!("{prefix}:"))
            .ok_or(EditError::InvalidXml)?
    };
    let (range, text) = if let Some(children) = children {
        let source = &input[children.range()];
        let qualified = source
            .strip_prefix('<')
            .and_then(|source| source.split([' ', '\t', '\n', '\r', '/', '>']).next())
            .ok_or(EditError::InvalidXml)?;
        let indent = indentation(input, children.range().start);
        if source.trim_end().ends_with("/>") {
            let offset = source.rfind("/>").ok_or(EditError::InvalidXml)?;
            let middle = indent.map_or_else(
                || fragment.to_owned(),
                |indent| {
                    format!(
                        "{}{}\t{fragment}{}{}",
                        newline(input),
                        indent,
                        newline(input),
                        indent
                    )
                },
            );
            (
                children.range(),
                format!("{}>{middle}</{qualified}>", &source[..offset]),
            )
        } else {
            let offset =
                children.range().start + source.rfind("</").ok_or(EditError::InvalidXml)?;
            insert_before_close(input, offset, fragment, indent)
        }
    } else {
        let source = &input[parent.range()];
        let offset = parent.range().start + source.rfind("</").ok_or(EditError::InvalidXml)?;
        let group = format!("<{prefix}{group}>{fragment}</{prefix}{group}>");
        insert_before_close(
            input,
            offset,
            &group,
            indentation(input, parent.range().start),
        )
    };
    let output = EditPlan::from_replacements(input, vec![Replacement { range, text }])?
        .output()
        .to_owned();
    document(&output)?;
    Ok(output)
}

fn insert_before_close(
    input: &str,
    offset: usize,
    fragment: &str,
    indent: Option<&str>,
) -> (Range<usize>, String) {
    if let Some(indent) = indent {
        let start = input[..offset].rfind('\n').map_or(0, |index| index + 1);
        if input[start..offset]
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
        {
            return (
                start..offset,
                format!("{indent}\t{fragment}{}{indent}", newline(input)),
            );
        }
    }
    (offset..offset, fragment.to_owned())
}

pub fn remove(input: &str, range: Range<usize>) -> Result<String, ObjectError> {
    Ok(EditPlan::from_replacements(
        input,
        vec![Replacement {
            range,
            text: String::new(),
        }],
    )?
    .output()
    .to_owned())
}
