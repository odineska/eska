//! Insert a native child declaration while preserving every byte outside its insertion boundary.

mod render;

use roxmltree::{Document, Node};
use std::collections::BTreeSet;

use super::{CreationError, CreationKind, CreationReference, PreparedCreate, schema::MD};
use crate::project::{
    metadata_edit::{
        EditError, EditPlan, Replacement, escape_xml, snapshot, types::indentation,
        xml_replacements,
    },
    metadata_model::{MetadataKind, ObjectId, standard_name},
    metadata_parser::{self, PropertiesMode},
    metadata_rename::{NameError, same_name, validate_name},
};

/// Descriptor ancestry is supplied by the checked source resolver, never inferred from filesystem paths.
pub fn plan(
    input: &str,
    descriptor_parent: Option<ObjectId>,
    prepared: &PreparedCreate,
    references: &[CreationReference],
) -> Result<(ObjectId, EditPlan), CreationError> {
    let request = &prepared.request;
    if snapshot(input) != request.snapshot {
        return Err(EditError::Conflict.into());
    }
    validate_name(&request.name).map_err(CreationError::Name)?;
    validate_text(prepared)?;
    let parsed = metadata_parser::parse(input, descriptor_parent.clone(), PropertiesMode::Summary)
        .map_err(|_| EditError::InvalidXml)?;
    let owner = parsed
        .objects
        .iter()
        .find(|object| object.metadata.id() == &request.owner)
        .ok_or(EditError::Conflict)?;
    let document = Document::parse(input).map_err(|_| EditError::InvalidXml)?;
    let node = document
        .descendants()
        .find(|node| node.is_element() && node.range() == owner.range)
        .ok_or(EditError::Conflict)?;
    let object_id = ObjectId::from_parts(
        Some(&request.owner),
        request.kind.metadata_kind().as_str(),
        &request.name,
    );
    validate_owner(node, owner.metadata.kind(), prepared)?;
    validate_name_context(node, &request.name, &object_id)?;
    prepared.identity.validate(request.kind, &document)?;
    let type_keys = super::types::validate(request, references)?;
    let child = node
        .children()
        .find(|child| child.has_tag_name((MD, "ChildObjects")));
    let layout = render::Layout::for_owner(input, child.unwrap_or(node));
    let fragment = render::declaration(
        node,
        child.unwrap_or(node),
        prepared,
        &type_keys,
        &layout,
        child.is_none(),
    )?;
    let insertion = insert(
        input,
        child.unwrap_or(node),
        &fragment,
        child.is_none(),
        request.kind,
    )?;
    let result = xml_replacements(input, vec![insertion])?;
    let updated =
        metadata_parser::parse(result.output(), descriptor_parent, PropertiesMode::Summary)
            .map_err(|_| EditError::InvalidXml)?;
    let created = updated
        .objects
        .iter()
        .find(|object| object.metadata.id() == &object_id)
        .ok_or(EditError::InvalidXml)?;
    if created.metadata.uuid() != prepared.identity.uuid {
        return Err(EditError::InvalidXml.into());
    }
    Ok((object_id, result))
}

fn validate_owner(
    node: Node<'_, '_>,
    kind: MetadataKind,
    prepared: &PreparedCreate,
) -> Result<(), CreationError> {
    if !super::kinds_for_owner(kind).contains(&prepared.request.kind) {
        return Err(CreationError::UnsupportedOwner(kind));
    }
    if kind == MetadataKind::TabularSection {
        let parent = node
            .parent_element()
            .and_then(|node| node.parent_element())
            .and_then(|node| MetadataKind::from_xml_tag(node.tag_name().name()).ok());
        if !parent
            .is_some_and(|kind| super::kinds_for_owner(kind).contains(&CreationKind::TablePart))
        {
            return Err(CreationError::UnsupportedOwner(kind));
        }
    }
    let properties: Vec<_> = node
        .children()
        .filter(|child| child.has_tag_name((MD, "Properties")))
        .collect();
    if properties.len() != 1
        || node
            .children()
            .filter(|child| child.has_tag_name((MD, "ChildObjects")))
            .count()
            > 1
        || properties[0].children().any(|child| {
            child.has_tag_name((MD, "ObjectBelonging")) && child.text() == Some("Adopted")
        })
    {
        return Err(EditError::UnsupportedValue.into());
    }
    Ok(())
}

fn validate_text(prepared: &PreparedCreate) -> Result<(), CreationError> {
    let request = &prepared.request;
    if request.comment.len() > 1_048_576 || request.synonyms.len() > 128 {
        return Err(CreationError::Invalid {
            field: "comment",
            reason: "too_large",
        });
    }
    escape_xml(&request.comment)?;
    let mut languages = BTreeSet::new();
    for synonym in &request.synonyms {
        if synonym.language.is_empty()
            || synonym.language.len() > 32
            || !synonym
                .language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !languages.insert(synonym.language.to_ascii_lowercase())
            || synonym.content.len() > 1_048_576
        {
            return Err(CreationError::Invalid {
                field: "synonyms",
                reason: "invalid_language_or_duplicate",
            });
        }
        escape_xml(&synonym.content)?;
    }
    Ok(())
}

fn validate_name_context(
    owner: Node<'_, '_>,
    name: &str,
    id: &ObjectId,
) -> Result<(), CreationError> {
    if owner
        .children()
        .filter(|child| child.has_tag_name((MD, "ChildObjects")))
        .flat_map(|children| children.children())
        .filter(|child| {
            child.tag_name().namespace() == Some(MD)
                && matches!(child.tag_name().name(), "Attribute" | "TabularSection")
        })
        .filter_map(|child| {
            child
                .children()
                .find(|node| node.has_tag_name((MD, "Properties")))
        })
        .filter_map(|properties| {
            properties
                .children()
                .find(|node| node.has_tag_name((MD, "Name")))
        })
        .filter_map(|node| node.text())
        .any(|other| same_name(other, name))
    {
        return Err(CreationError::Collision(id.clone()));
    }
    // Native name reservation survives disabling code, hierarchy or owner fields.
    let mut standard = crate::project::metadata_edit::choice_fields::standard::fields_for(
        owner.tag_name().name(),
        "DontUse",
        &|property| match property {
            "CodeLength" | "DescriptionLength" | "NumberLength" => Some("1"),
            "Hierarchical" => Some("true"),
            "HierarchyType" => Some("HierarchyFoldersAndItems"),
            _ => None,
        },
        true,
    );
    if !owner.has_tag_name((MD, "TabularSection")) {
        standard.push("ThisObject");
    }
    if standard.iter().any(|english| {
        same_name(english, name)
            || standard_name(english).is_some_and(|russian| same_name(russian, name))
    }) {
        return Err(CreationError::Name(NameError::ReservedProperty));
    }
    Ok(())
}

/// Insert adjacent to existing metadata peers; opaque nodes and whitespace retain their exact bytes.
fn insert(
    input: &str,
    container: Node<'_, '_>,
    fragment: &str,
    missing_group: bool,
    kind: CreationKind,
) -> Result<Replacement, EditError> {
    if container
        .children()
        .any(|child| child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(EditError::UnsupportedValue);
    }
    let tag = kind.metadata_kind().designer_tag();
    let last = (!missing_group)
        .then(|| {
            container
                .children()
                .rfind(|child| child.has_tag_name((MD, tag)))
        })
        .flatten();
    let anchor = last.or_else(|| {
        let kind_position = match kind {
            CreationKind::Attribute => 0,
            CreationKind::TablePart => 1,
        };
        (!missing_group)
            .then(|| {
                container.children().find(|child| {
                    child.tag_name().namespace() == Some(MD)
                        && match child.tag_name().name() {
                            "TabularSection" => kind_position < 1,
                            "Form" | "Template" | "Command" => true,
                            _ => false,
                        }
                })
            })
            .flatten()
    });
    if let Some(anchor) = anchor {
        let after = last.is_some();
        let at = if after {
            anchor.range().end
        } else {
            anchor.range().start
        };
        let separator = indentation(input, anchor.range().start)
            .map(|indent| {
                format!(
                    "{}{indent}",
                    crate::project::metadata_edit::types::newline(input)
                )
            })
            .unwrap_or_default();
        return Ok(Replacement {
            range: at..at,
            text: if after {
                format!("{separator}{fragment}")
            } else {
                format!("{fragment}{separator}")
            },
        });
    }
    append(input, container, fragment)
}

fn append(input: &str, container: Node<'_, '_>, fragment: &str) -> Result<Replacement, EditError> {
    let range = container.range();
    let raw = &input[range.clone()];
    let layout = render::Layout::for_owner(input, container);
    let separator = layout.child_separator();
    let trailing = layout.parent_separator();
    if let Some(open) = raw.strip_suffix("/>") {
        let name = open
            .strip_prefix('<')
            .and_then(|open| open.split_whitespace().next())
            .ok_or(EditError::InvalidXml)?;
        return Ok(Replacement {
            range: range.end - 2..range.end,
            text: format!(">{separator}{fragment}{trailing}</{name}>"),
        });
    }
    let closing = raw.rfind("</").ok_or(EditError::InvalidXml)? + range.start;
    let mut at = closing;
    if let Some(indent) = indentation(input, closing) {
        at -= indent.len();
        if input[..at].ends_with("\r\n") {
            at -= 2;
        } else if input[..at].ends_with(['\r', '\n']) {
            at -= 1;
        }
    }
    let trailing = if at < closing {
        String::new()
    } else {
        trailing
    };
    Ok(Replacement {
        range: at..at,
        text: format!("{separator}{fragment}{trailing}"),
    })
}
