//! Data paths for the inspector are resolved against the same composed form as the renderer.

use std::collections::HashMap;

use super::{
    EffectiveDefinition, FieldType, FieldValueKind, MetadataKind, MetadataProperty, MetadataValue,
    ObjectId, PresentationText, PropertySource, appearance, definition_type, effective_metadata,
    field_type, metadata_field, metadata_property, record, references, resolve_binding,
    single_type_target, standard_attribute, standard_caption, standard_fields, standard_type,
};
use crate::project::forms::{FormPreview, MAX_FORM_ELEMENTS};

/// Resolve only the explicitly requested branch. A self-reference consumes a path segment;
/// neither loading nor enumeration recursively walks the metadata reference graph.
fn targets(
    form: &super::ComposedForm,
    resources: &[super::FormResource],
    path: &[String],
) -> (Vec<ObjectId>, Vec<ObjectId>) {
    let mut missing = Vec::new();
    let Some((name, remaining)) = path.split_first() else {
        return (Vec::new(), missing);
    };
    let Some(attribute) = form
        .attributes
        .iter()
        .find(|item| item.name.as_ref() == Some(name))
    else {
        return (Vec::new(), missing);
    };
    let remaining: Vec<_> = remaining.iter().map(String::as_str).collect();
    let (value_type, consumed) = super::column_binding(attribute, &remaining).map_or_else(
        || (definition_type(attribute), 0),
        |(binding, count)| (binding.value_type, count),
    );
    let mut current: Vec<_> = value_type
        .types
        .iter()
        .filter_map(references::metadata_type_id)
        .collect();
    for part in &remaining[consumed..] {
        let mut next = Vec::new();
        for target in current {
            let Some((_, owner)) = effective_metadata(&target, resources) else {
                missing.push(target);
                continue;
            };
            if let Some((_, child)) = super::metadata_child(owner, part, resources) {
                if child.metadata.kind() == MetadataKind::TabularSection {
                    next.push(child.metadata.id().clone());
                } else if let Some(field) = metadata_property(child, "Type", resources) {
                    next.extend(
                        field_type(&field.value)
                            .types
                            .iter()
                            .filter_map(references::metadata_type_id),
                    );
                }
            } else if standard_fields(owner, resources).contains(part) {
                next.extend(
                    standard_type(part, owner, resources)
                        .types
                        .iter()
                        .filter_map(references::metadata_type_id),
                );
            }
        }
        next.sort();
        next.dedup();
        current = next;
    }
    for target in &current {
        if effective_metadata(target, resources).is_none() {
            missing.push(target.clone());
        }
    }
    (current, missing)
}

pub(in crate::project::forms) fn inspector_resource_requests(
    form: &super::ComposedForm,
    resources: &[super::FormResource],
    paths: &[Vec<String>],
) -> Vec<ObjectId> {
    paths
        .iter()
        .flat_map(|path| targets(form, resources, path).1)
        .collect()
}

#[derive(Debug)]
pub struct InspectorDataField<'a> {
    pub path: Vec<String>,
    pub caption: Option<PresentationText>,
    pub value_type: FieldType,
    pub type_property: Option<&'a MetadataProperty>,
    pub origin: Option<PropertySource>,
}

impl EffectiveDefinition {
    /// Reuse the binding type decoder, including resolved `QName` namespaces.
    #[must_use]
    pub fn inspector_type(&self) -> Option<FieldType> {
        self.property("Type")
            .map(|field| field_type(&field.property.value))
    }
}

impl FormPreview {
    /// Enumerate declared columns, fields of the main attribute, and reference paths used on this form.
    #[must_use]
    pub fn inspector_data(&self) -> Vec<InspectorDataField<'_>> {
        let mut data = DataFields {
            preview: self,
            fields: Vec::new(),
            paths: HashMap::new(),
        };
        let attributes: HashMap<_, _> = self
            .form
            .attributes
            .iter()
            .filter_map(|attribute| Some((attribute.name.as_deref()?, attribute)))
            .collect();
        for attribute in &self.form.attributes {
            let Some(name) = attribute.name.as_deref() else {
                continue;
            };
            let mut path = vec![name.to_owned()];
            if let Some(columns) = attribute.property("Columns") {
                data.columns(&columns.property.value, &columns.origin, &mut path);
            }
            let main = attribute
                .property("MainAttribute")
                .and_then(|field| appearance::text(&field.property.value))
                .is_some_and(|value| matches!(value, "true" | "1"));
            if main && let Some(target) = single_type_target(&definition_type(attribute)) {
                data.metadata(&target, &mut path);
            }
        }
        let mut nodes = vec![&self.form.root];
        while let Some(node) = nodes.pop() {
            nodes.extend(node.auxiliary.iter().rev());
            nodes.extend(node.children.iter().rev());
            let Some(binding) = node.scalar("DataPath") else {
                continue;
            };
            let parts: Vec<_> = binding.split('.').collect();
            if parts.len() > 64 || parts.iter().any(|part| part.is_empty()) {
                continue;
            }
            let Some(attribute) = attributes.get(parts[0]) else {
                continue;
            };
            let mut path = vec![parts[0].to_owned()];
            for (index, part) in parts.iter().enumerate().skip(1) {
                path.push((*part).to_owned());
                if !data.paths.contains_key(&path)
                    && let Ok(binding) =
                        resolve_binding(attribute, &parts[1..=index], &self.resources)
                {
                    data.bound(&path, binding);
                }
            }
        }
        for path in &self.expanded_data {
            for target in targets(&self.form, &self.resources, path).0 {
                data.metadata(&target, &mut path.clone());
            }
        }
        data.fields
    }

    /// A data path is a semantic form address; the client never supplies a file or range.
    #[must_use]
    pub fn inspector_data_origin(&self, path: &[String]) -> Option<PropertySource> {
        let (name, rest) = path.split_first()?;
        if !rest.is_empty() {
            return self
                .inspector_data()
                .into_iter()
                .find(|field| field.path == path)?
                .origin;
        }
        let attribute = self
            .form
            .attributes
            .iter()
            .find(|value| value.name.as_ref() == Some(name))?;
        let remaining: Vec<_> = rest.iter().map(String::as_str).collect();
        resolve_binding(attribute, &remaining, &self.resources)
            .ok()?
            .source
    }
}

struct DataFields<'a> {
    preview: &'a FormPreview,
    fields: Vec<InspectorDataField<'a>>,
    paths: HashMap<Vec<String>, usize>,
}

impl<'a> DataFields<'a> {
    fn push(
        &mut self,
        path: &[String],
        caption: Option<PresentationText>,
        value_type: FieldType,
        type_property: Option<&'a MetadataProperty>,
        origin: Option<PropertySource>,
    ) {
        if let Some(index) = self.paths.get(path) {
            let field = &mut self.fields[*index];
            // Composite references can expose the same name with different types.
            // Such a row has no single XML origin or property record to navigate to.
            let same_origin = field
                .origin
                .as_ref()
                .zip(origin.as_ref())
                .is_some_and(|(a, b)| a.source == b.source && a.range == b.range);
            if !same_origin {
                field.origin = None;
                field.type_property = None;
            }
            for key in value_type.types {
                if !field.value_type.types.contains(&key) {
                    field.value_type.types.push(key);
                }
            }
            if field.value_type.kind != value_type.kind || field.value_type.types.len() > 1 {
                field.value_type.kind = FieldValueKind::Composite;
            }
            if field.value_type.length != value_type.length {
                field.value_type.length = None;
            }
            if field.value_type.digits != value_type.digits {
                field.value_type.digits = None;
            }
            if field.value_type.fraction != value_type.fraction {
                field.value_type.fraction = None;
            }
            if field.value_type.date_parts != value_type.date_parts {
                field.value_type.date_parts = None;
            }
            if field.value_type.allowed_sign != value_type.allowed_sign {
                field.value_type.allowed_sign = None;
            }
        } else if self.fields.len() < MAX_FORM_ELEMENTS {
            self.paths.insert(path.to_vec(), self.fields.len());
            self.fields.push(InspectorDataField {
                path: path.to_vec(),
                caption,
                value_type,
                type_property,
                origin,
            });
        }
    }

    fn columns(
        &mut self,
        value: &'a MetadataValue,
        origin: &PropertySource,
        path: &mut Vec<String>,
    ) {
        let MetadataValue::Record(columns) = value else {
            return;
        };
        for column in columns {
            let Some(name) = appearance::qualifier(column, "name") else {
                continue;
            };
            if name.is_empty() || name.contains('.') {
                continue;
            }
            path.push(name.to_owned());
            let property = record(&column.value, "Type");
            let value_type = property
                .map(|field| field_type(&field.value))
                .unwrap_or_default();
            self.push(path, None, value_type, property, Some(origin.clone()));
            if let Some(children) = record(&column.value, "Columns") {
                self.columns(&children.value, origin, path);
            }
            path.pop();
        }
    }

    fn metadata(&mut self, id: &ObjectId, path: &mut Vec<String>) {
        let resources = &self.preview.resources;
        let Some((_, owner)) = effective_metadata(id, resources) else {
            return;
        };
        for name in standard_fields(owner, resources) {
            let value_type = standard_type(name, owner, resources);
            if matches!(name, "Code" | "Description" | "Number")
                && value_type.length.or(value_type.digits).unwrap_or(0) == 0
            {
                continue;
            }
            path.push(name.to_owned());
            self.push(
                path,
                Some(standard_caption(name, owner, resources)),
                value_type,
                None,
                None,
            );
            path.pop();
        }
        for child in resources
            .iter()
            .flat_map(|resource| &resource.descriptor.objects)
        {
            if child.metadata.parent() != Some(id)
                || !matches!(
                    child.metadata.kind(),
                    MetadataKind::Attribute
                        | MetadataKind::TabularSection
                        | MetadataKind::Dimension
                        | MetadataKind::Resource
                        | MetadataKind::AddressingAttribute
                        | MetadataKind::AccountingFlag
                        | MetadataKind::ExtDimensionAccountingFlag
                )
            {
                continue;
            }
            path.push(child.metadata.name().to_owned());
            if let Some((resource, child)) = effective_metadata(child.metadata.id(), resources) {
                let binding = metadata_field(resource, child, resources);
                self.push(
                    path,
                    None,
                    binding.value_type,
                    metadata_property(child, "Type", resources),
                    binding.source,
                );
                if child.metadata.kind() == MetadataKind::TabularSection {
                    self.metadata(child.metadata.id(), path);
                }
            }
            path.pop();
        }
    }

    fn bound(&mut self, path: &[String], binding: super::FieldBinding) {
        let resources = &self.preview.resources;
        let origin = binding.source;
        let object = origin.as_ref().and_then(|origin| {
            resources
                .iter()
                .find(|resource| resource.source == origin.source)?
                .descriptor
                .objects
                .iter()
                .find(|object| object.range == origin.range)
        });
        let name = path.last().map(String::as_str).unwrap_or_default();
        let mut value_type = binding.value_type;
        let mut type_property = None;
        let mut caption = None;
        if let Some(object) = object {
            if object.metadata.name() == name {
                type_property = metadata_property(object, "Type", resources);
            } else if standard_attribute(name) {
                value_type = standard_type(name, object, resources);
                caption = binding.caption;
            }
        }
        self.push(path, caption, value_type, type_property, origin);
    }
}
