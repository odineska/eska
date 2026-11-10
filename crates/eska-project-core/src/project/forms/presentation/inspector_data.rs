//! Data paths for the inspector are resolved against the same composed form as the renderer.

use std::collections::{HashMap, HashSet};

use super::{
    EffectiveDefinition, FieldType, FieldValueKind, MetadataKind, MetadataProperty, MetadataValue,
    ObjectId, PresentationText, PropertyKey, PropertySource, appearance, definition_type,
    effective_metadata, field_type, metadata_field, metadata_property, record, references,
    resolve_binding, single_type_target, standard_attribute, standard_caption, standard_type,
};
use crate::project::forms::{FormPreview, MAX_FORM_ELEMENTS};

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
            paths: HashSet::new(),
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
                if !data.paths.contains(&path)
                    && let Ok(binding) =
                        resolve_binding(attribute, &parts[1..=index], &self.resources)
                {
                    data.bound(&path, binding);
                }
            }
        }
        data.fields
    }

    /// A data path is a semantic form address; the client never supplies a file or range.
    #[must_use]
    pub fn inspector_data_origin(&self, path: &[String]) -> Option<PropertySource> {
        let (name, rest) = path.split_first()?;
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
    paths: HashSet<Vec<String>>,
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
        if self.fields.len() < MAX_FORM_ELEMENTS && self.paths.insert(path.to_vec()) {
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
        // These standard members are available on the corresponding managed object.
        // Other standard fields are added when a binding resolves them.
        let standard: &[&str] = match owner.metadata.kind() {
            MetadataKind::Document => &["Ref", "DeletionMark", "Date", "Number", "Posted"],
            MetadataKind::Catalog => &["Ref", "DeletionMark", "Code", "Description"],
            MetadataKind::TabularSection => &["LineNumber"],
            _ => &[],
        };
        for &name in standard {
            let value_type = inspector_standard_type(name, owner, resources);
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
                )
            {
                continue;
            }
            path.push(child.metadata.name().to_owned());
            if !self.paths.contains(path)
                && let Some((resource, child)) = effective_metadata(child.metadata.id(), resources)
            {
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
                value_type = inspector_standard_type(name, object, resources);
                caption = binding.caption;
            }
        }
        self.push(path, caption, value_type, type_property, origin);
    }
}

fn inspector_standard_type(
    name: &str,
    owner: &super::ParsedObject,
    resources: &[super::FormResource],
) -> FieldType {
    let mut result = standard_type(name, owner, resources);
    if matches!(name, "Number" | "Code")
        && metadata_property(owner, &format!("{name}Type"), resources)
            .and_then(|field| appearance::text(&field.value))
            == Some("Number")
    {
        result.kind = FieldValueKind::Number;
        result.digits = result.length.take();
        result.fraction = Some(0);
    }
    if result.kind == FieldValueKind::Date {
        result.date_parts = Some("DateTime".into());
    }
    if matches!(name, "Ref" | "Parent") {
        result.types.push(PropertyKey {
            namespace: Some(references::CONFIG.into()),
            name: format!(
                "{}Ref.{}",
                owner.metadata.kind().designer_tag(),
                owner.metadata.name()
            ),
        });
    }
    result
}
