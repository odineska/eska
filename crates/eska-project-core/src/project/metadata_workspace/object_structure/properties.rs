//! Structural overrides reuse existing scalar schemas; they cannot inject XML or change identity.

use super::{ProjectSession, WorkspaceError};
use crate::project::{
    metadata_edit::{
        EditError, EditPlan, EditableField, EditingDocument, PropertyChange, Replacement,
        ScalarSchema, escape_xml, snapshot,
    },
    metadata_model::{MetadataKind, NodeId, ObjectId},
    metadata_objects::{
        ObjectError, ObjectPlan, ObjectRequest, invalid,
        inventory::{Inputs, text},
        xml,
    },
    metadata_parser::{self, PropertiesMode},
};

struct Target<'a> {
    id: &'a ObjectId,
    parent: Option<ObjectId>,
    modern: bool,
}
impl Target<'_> {
    fn editing<'a>(&'a self, input: &'a str) -> EditingDocument<'a> {
        EditingDocument {
            input,
            id: self.id,
            parent: self.parent.clone(),
            predefined: false,
            modern: self.modern,
        }
    }
}

impl ProjectSession {
    pub(super) fn object_properties(
        &mut self,
        plan: &mut ObjectPlan,
        inputs: &mut Inputs,
    ) -> Result<(), ObjectError> {
        let (values, lists, types) = match &plan.prepared.request {
            ObjectRequest::Create {
                property_values,
                property_references,
                property_types,
                ..
            }
            | ObjectRequest::Copy {
                property_values,
                property_references,
                property_types,
                ..
            } => (property_values, property_references, property_types),
            ObjectRequest::Delete { .. } | ObjectRequest::CreateModule { .. } => return Ok(()),
        };
        let id = plan.object_id.as_ref().ok_or(EditError::Conflict)?;
        let (owner, paths) = self
            .source
            .descriptor_candidates(id)
            .map_err(WorkspaceError::Source)?;
        let file = plan
            .files
            .iter_mut()
            .find(|file| paths.contains(&file.path))
            .ok_or(EditError::Conflict)?;
        let mut input = text(file.after.as_deref().ok_or(EditError::Conflict)?)?.to_owned();
        let target = Target {
            id,
            parent: owner.parent(),
            modern: self
                .project()
                .configuration()
                .build_settings()
                .platform_version()
                .is_some_and(|version| version.as_str().starts_with("8.5.1.")),
        };
        for (key, keys) in types {
            input = self.override_types(&input, &target, key, keys, inputs)?;
        }
        for (key, value) in values {
            input = self.override_scalar(&input, &target, key, value, inputs)?;
        }
        for (key, values) in lists {
            input = self.override_list(&input, &target, key, values, inputs)?;
        }
        let parsed = metadata_parser::parse(&input, owner.parent(), PropertiesMode::Summary)
            .map_err(|_| EditError::InvalidXml)?;
        let object = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == id)
            .ok_or(EditError::Conflict)?;
        if matches!(
            plan.prepared.request,
            ObjectRequest::Create { template: None, .. }
        ) {
            validate_required(&input, object.metadata.kind(), object.range.clone())?;
        }
        self.unique_properties(
            id,
            object.metadata.kind(),
            &input,
            object.range.clone(),
            inputs,
        )?;
        file.after = Some(input.into_bytes());
        Ok(())
    }

    fn override_scalar(
        &mut self,
        input: &str,
        target: &Target<'_>,
        key: &str,
        value: &str,
        inputs: &mut Inputs,
    ) -> Result<String, ObjectError> {
        if matches!(key, "Name" | "uuid" | "InternalInfo" | "ObjectBelonging")
            || crate::project::metadata_rename::validate_name(key).is_err()
        {
            return Err(invalid("propertyValues", "structural_property"));
        }
        let input = insert_scalar(input, target, key, value)?;
        let editing = target.editing(&input);
        let field = property_field(&editing, key, "propertyValues")?;
        if matches!(field.schema, ScalarSchema::Reference { .. }) && !value.is_empty() {
            self.override_reference(value, inputs)?;
        }
        if matches!(
            field.schema,
            ScalarSchema::Text {
                domain: Some(crate::project::metadata_edit::TextDomain::CommandGroup)
            }
        ) && !crate::project::metadata_edit::references::COMMAND_GROUPS.contains(&value)
        {
            self.override_reference(value, inputs)?;
        }
        Ok(editing
            .update(
                &snapshot(&input),
                &field.path,
                &PropertyChange::Text {
                    value: value.to_owned(),
                },
            )?
            .output()
            .to_owned())
    }

    fn override_list(
        &mut self,
        input: &str,
        target: &Target<'_>,
        key: &str,
        values: &[String],
        inputs: &mut Inputs,
    ) -> Result<String, ObjectError> {
        let editing = target.editing(input);
        let field = property_field(&editing, key, "propertyReferences")?;
        let ScalarSchema::References { domain, .. } = &field.schema else {
            return Err(invalid("propertyReferences", "unsupported_property"));
        };
        crate::project::metadata_edit::reference_lists::validate(domain, values)?;
        for value in values {
            self.override_reference(value, inputs)?;
        }
        Ok(editing
            .update(
                &snapshot(input),
                &field.path,
                &PropertyChange::References {
                    values: values.to_vec(),
                },
            )?
            .output()
            .to_owned())
    }

    fn override_reference(&mut self, value: &str, inputs: &mut Inputs) -> Result<(), ObjectError> {
        let parts =
            crate::project::metadata_edit::reference_parts(value).ok_or(EditError::InvalidValue)?;
        let target = self.property_reference(&parts)?;
        if let Some(location) = self
            .source
            .object_descriptor(&target.id)
            .map_err(WorkspaceError::Source)?
        {
            inputs.required(self.project().source(), &location.path)?;
        }
        Ok(())
    }

    fn override_types(
        &mut self,
        input: &str,
        target: &Target<'_>,
        key: &str,
        keys: &[crate::project::metadata_model::PropertyKey],
        inputs: &mut Inputs,
    ) -> Result<String, ObjectError> {
        let editing = target.editing(input);
        let field = property_field(&editing, key, "propertyTypes")?;
        let ScalarSchema::Types { domain, .. } = field.schema else {
            return Err(invalid("propertyTypes", "unsupported_property"));
        };
        crate::project::metadata_edit::type_sets::validate(domain, keys)?;
        for key in keys {
            if let Some(parts) = crate::project::metadata_edit::type_sets::target(domain, key) {
                let object = self.property_reference(&parts)?;
                if let Some(location) = self
                    .source
                    .object_descriptor(&object.id)
                    .map_err(WorkspaceError::Source)?
                {
                    inputs.required(self.project().source(), &location.path)?;
                }
            }
        }
        Ok(editing
            .update(
                &snapshot(input),
                &field.path,
                &PropertyChange::Types {
                    keys: keys.to_vec(),
                },
            )?
            .output()
            .to_owned())
    }

    fn unique_properties(
        &mut self,
        id: &ObjectId,
        kind: MetadataKind,
        input: &str,
        range: std::ops::Range<usize>,
        inputs: &mut Inputs,
    ) -> Result<(), ObjectError> {
        let Some(key) = crate::project::metadata_objects::templates::unique_property(kind) else {
            return Ok(());
        };
        let document = xml::document(input)?;
        let node = document
            .descendants()
            .find(|node| node.is_element() && node.range() == range)
            .ok_or(EditError::Conflict)?;
        let value = unique_value(node, kind, key)?;
        if value.is_empty() && kind != MetadataKind::URLTemplate {
            return Err(invalid("propertyValues", "required_property"));
        }
        let parent = id.parent();
        if let Some(parent) = &parent {
            self.children(
                &NodeId::Object(parent.clone()),
                crate::project::configurator::TreeOptions::default(),
            )?;
        }
        let candidates: Vec<_> = self
            .objects
            .values()
            .filter(|object| object.kind == kind && object.parent == parent && object.id != *id)
            .map(|object| object.id.clone())
            .collect();
        for candidate in candidates {
            let context = self.object_context(&candidate, inputs)?;
            let document = xml::document(&context.input)?;
            let node = document
                .descendants()
                .find(|node| node.is_element() && node.range() == context.range)
                .ok_or(EditError::Conflict)?;
            let other = unique_value(node, kind, key)?;
            let equal = if kind == MetadataKind::URLTemplate {
                other == value
            } else {
                crate::project::metadata_rename::same_name(other, value)
            };
            if equal {
                return Err(ObjectError::PropertyConflict {
                    property: key.to_owned(),
                    value: value.to_owned(),
                    object: candidate,
                });
            }
        }
        Ok(())
    }
}

/// Omitted HTTP fields use platform defaults; an explicit empty URL template stays empty.
fn unique_value<'a>(
    node: roxmltree::Node<'a, 'a>,
    kind: MetadataKind,
    key: &str,
) -> Result<&'a str, ObjectError> {
    let default = match kind {
        MetadataKind::URLTemplate => "/*",
        MetadataKind::Method => "GET",
        _ => "",
    };
    Ok(xml::properties(node)?
        .children()
        .find(|node| node.has_tag_name((xml::MD, key)))
        .map_or(default, |node| node.text().unwrap_or_default()))
}

fn property_field(
    editing: &EditingDocument<'_>,
    key: &str,
    input_field: &'static str,
) -> Result<EditableField, ObjectError> {
    editing
        .inspect()?
        .fields
        .into_iter()
        .find(|field| {
            field.path.len() == 1
                && field.path[0].key.namespace.as_deref() == Some(xml::MD)
                && field.path[0].key.name == key
        })
        .ok_or_else(|| invalid(input_field, "unsupported_property"))
}

fn insert_scalar(
    input: &str,
    target: &Target<'_>,
    key: &str,
    value: &str,
) -> Result<String, ObjectError> {
    let parsed = metadata_parser::parse(input, target.parent.clone(), PropertiesMode::Summary)
        .map_err(|_| EditError::InvalidXml)?;
    let range = &parsed
        .objects
        .iter()
        .find(|object| object.metadata.id() == target.id)
        .ok_or(EditError::Conflict)?
        .range;
    let document = xml::document(input)?;
    let node = document
        .descendants()
        .find(|node| node.is_element() && node.range() == *range)
        .ok_or(EditError::Conflict)?;
    let properties = xml::properties(node)?;
    if properties
        .children()
        .any(|node| node.has_tag_name((xml::MD, key)))
    {
        return Ok(input.to_owned());
    }
    let prefix = if properties.default_namespace() == Some(xml::MD) {
        String::new()
    } else {
        properties
            .lookup_prefix(xml::MD)
            .map(|prefix| format!("{prefix}:"))
            .ok_or(EditError::InvalidXml)?
    };
    let at = properties.range().start
        + input[properties.range()]
            .rfind("</")
            .ok_or(EditError::InvalidXml)?;
    let fragment = format!("<{prefix}{key}>{}</{prefix}{key}>", escape_xml(value)?);
    Ok(EditPlan::from_replacements(
        input,
        vec![Replacement {
            range: at..at,
            text: fragment,
        }],
    )?
    .output()
    .to_owned())
}

fn validate_required(
    input: &str,
    kind: MetadataKind,
    range: std::ops::Range<usize>,
) -> Result<(), ObjectError> {
    let document = xml::document(input)?;
    let node = document
        .descendants()
        .find(|node| node.is_element() && node.range() == range)
        .ok_or(EditError::InvalidXml)?;
    let properties = xml::properties(node)?;
    for key in crate::project::metadata_objects::templates::required_properties(kind) {
        if !properties.children().any(|node| {
            node.has_tag_name((xml::MD, *key))
                && (node.children().any(|node| node.is_element())
                    || node.text().is_some_and(|text| !text.is_empty()))
        }) {
            return Err(invalid(key, "required_property"));
        }
    }
    Ok(())
}
