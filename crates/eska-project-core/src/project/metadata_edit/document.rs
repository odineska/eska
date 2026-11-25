//! Bind logical field addresses to one freshly parsed descriptor snapshot.

use crate::project::metadata_edit::EditingProfile;
use roxmltree::Node;
use serde::{Deserialize, Serialize};

use super::{EditError, EditPlan, ScalarSchema, patch, schema, snapshot};
use crate::project::{
    metadata_model::{ObjectId, PropertyKey},
    metadata_parser::{self, ParsedDescriptor, PropertiesMode},
};

/// Repeated keys are addressed by occurrence in their parent, never by client-supplied offsets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldStep {
    pub key: PropertyKey,
    pub occurrence: usize,
}

/// A typed request supplies semantic values, never serialized XML or filesystem coordinates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PropertyChange {
    Text {
        value: String,
    },
    DataType {
        key: PropertyKey,
    },
    Value {
        key: Option<PropertyKey>,
        value: String,
    },
    References {
        values: Vec<String>,
    },
    Types {
        keys: Vec<PropertyKey>,
    },
}

/// An existing scalar and its backend-owned editing domain.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableField {
    pub path: Vec<FieldStep>,
    pub value: String,
    pub schema: ScalarSchema,
    /// Localized strings retain their configuration language independently of UI locale.
    pub language: Option<String>,
}

/// Exact-file version and editable leaves; fields absent here remain read-only.
#[derive(Clone, Debug, Serialize)]
pub struct PropertyEditing {
    pub snapshot: String,
    pub profile: &'static str,
    pub read_only_properties: Vec<ReadOnlyProperty>,
    pub fields: Vec<EditableField>,
}

/// A property without a verified writer remains explicitly outside the mutation contract.
#[derive(Clone, Debug, Serialize)]
pub struct ReadOnlyProperty {
    pub key: PropertyKey,
    pub reason: &'static str,
}

/// Descriptor context is supplied by the checked Designer resolver, not by the IDE client.
pub struct EditingDocument<'a> {
    pub input: &'a str,
    pub id: &'a ObjectId,
    pub parent: Option<ObjectId>,
    pub predefined: bool,
    pub profile: EditingProfile,
}

impl EditingDocument<'_> {
    /// Inspect known existing leaves without inventing missing XML fields or collection items.
    pub fn inspect(&self) -> Result<PropertyEditing, EditError> {
        let parsed = self.parsed(self.input)?;
        let object = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == self.id)
            .ok_or(EditError::UnsupportedValue)?;
        let document = roxmltree::Document::parse(self.input).map_err(|_| EditError::InvalidXml)?;
        let node = document
            .descendants()
            .find(|node| node.is_element() && node.range() == object.range)
            .ok_or(EditError::UnsupportedValue)?;
        let properties = node
            .children()
            .find(|node| node.has_tag_name((schema::MD, "Properties")))
            .ok_or(EditError::UnsupportedValue)?;
        let class = schema::object_class(self.id);
        let type_rules: Vec<_> = super::type_sets::rules(self.id).collect();
        let mut fields = Vec::new();
        let mut read_only_properties = Vec::new();
        for (node, step) in children(properties) {
            let count = fields.len();
            if let Some(rule) = type_rules
                .iter()
                .copied()
                .find(|rule| rule.property == node.tag_name().name())
                .filter(|_| node.tag_name().namespace() == Some(schema::MD))
                && let Ok(schema) = super::type_sets::schema(rule, Some(node))
            {
                fields.push(EditableField {
                    path: vec![step.clone()],
                    value: String::new(),
                    schema,
                    language: None,
                });
            }
            if let Some(rule) = super::reference_lists::rule(&class, node.tag_name().name())
                .filter(|_| node.tag_name().namespace() == Some(schema::MD))
                && let Ok(schema) = super::reference_lists::schema(rule, Some(node))
            {
                fields.push(EditableField {
                    path: vec![step],
                    value: String::new(),
                    schema,
                    language: None,
                });
                continue;
            }
            let kind = schema::field_type(&class, node, true, self.profile);
            self.visit(node, kind, &mut vec![step.clone()], &mut fields);
            if fields.len() == count {
                read_only_properties.push(ReadOnlyProperty {
                    key: step.key,
                    reason: if schema::protected(node.tag_name().name()) {
                        "structural_operation"
                    } else if super::lengths::inherited_from_numerator(node) {
                        "numerator_inherited"
                    } else if let Some(reason) = super::hierarchy::read_only_reason(node) {
                        reason
                    } else if let Some(reason) = super::ext_dimensions::read_only_reason(node) {
                        reason
                    } else {
                        "unsupported_domain_or_shape"
                    },
                });
            }
        }
        for rule in super::reference_lists::rules(&class).filter(|rule| {
            !properties
                .children()
                .any(|node| node.has_tag_name((schema::MD, rule.property)))
        }) {
            fields.push(container_field(
                rule.property,
                super::reference_lists::schema(rule, None)?,
            ));
        }
        for rule in type_rules.into_iter().filter(|rule| {
            !properties
                .children()
                .any(|node| node.has_tag_name((schema::MD, rule.property)))
        }) {
            fields.push(container_field(
                rule.property,
                super::type_sets::schema(rule, None)?,
            ));
        }
        Ok(PropertyEditing {
            snapshot: snapshot(self.input),
            profile: self.profile.name(),
            read_only_properties,
            fields,
        })
    }

    /// A stale source or unadvertised address is rejected before constructing any replacement.
    pub fn update(
        &self,
        expected: &str,
        path: &[FieldStep],
        change: &PropertyChange,
    ) -> Result<EditPlan, EditError> {
        if expected != snapshot(self.input) {
            return Err(EditError::Conflict);
        }
        if path.is_empty() || path.len() > 64 {
            return Err(EditError::UnsupportedValue);
        }
        let editing = self.inspect()?;
        let field = editing
            .fields
            .iter()
            .find(|field| field.path == path)
            .ok_or(EditError::UnsupportedValue)?;
        validate_change(field, change)?;
        let parsed = self.parsed(self.input)?;
        let object = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == self.id)
            .ok_or(EditError::UnsupportedValue)?;
        let document = roxmltree::Document::parse(self.input).map_err(|_| EditError::InvalidXml)?;
        let node = document
            .descendants()
            .find(|node| node.is_element() && node.range() == object.range)
            .ok_or(EditError::UnsupportedValue)?;
        let mut node = node
            .children()
            .find(|node| node.has_tag_name((schema::MD, "Properties")))
            .ok_or(EditError::UnsupportedValue)?;
        if let Some(plan) = self.insert_missing(node, path, change)? {
            return Ok(plan);
        }
        for step in path {
            node = children(node)
                .into_iter()
                .find(|(_, candidate)| candidate == step)
                .map(|(node, _)| node)
                .ok_or(EditError::UnsupportedValue)?;
        }
        let plan = match change {
            PropertyChange::Types { keys } => {
                let ScalarSchema::Types { domain, .. } = &field.schema else {
                    return Err(EditError::InvalidValue);
                };
                super::type_sets::replace(self.input, node, *domain, keys)?
            }
            PropertyChange::References { values } => {
                let ScalarSchema::References { domain, .. } = &field.schema else {
                    return Err(EditError::InvalidValue);
                };
                super::reference_lists::replace(self.input, node, domain, values)?
            }
            PropertyChange::Text { value }
                if matches!(field.schema, ScalarSchema::Decimal { nullable: true }) =>
            {
                super::values::replace_bound(self.input, node, value)?
            }
            PropertyChange::Text { value } if super::type_links::is_link(node) => {
                super::type_links::replace(self.input, node, value)?
            }
            PropertyChange::Text { value } if super::choice_parameters::is_item(node) => {
                super::choice_parameters::rename(self.input, node, value)?
            }
            PropertyChange::Text { value } => patch::text_plan(self.input, node.range(), value)?,
            PropertyChange::DataType { key } => super::types::replace(self.input, node, key)?,
            PropertyChange::Value { key, value } => {
                super::values::replace_value(self.input, node, key.as_ref(), value)?
            }
        };
        let after = self.parsed(plan.output())?;
        if !plan.is_empty() {
            validate_dependencies(node, plan.output())?;
        }
        if parsed
            .objects
            .iter()
            .map(|object| &object.metadata)
            .ne(after.objects.iter().map(|object| &object.metadata))
        {
            return Err(EditError::UnsupportedValue);
        }
        Ok(plan)
    }

    fn insert_missing(
        &self,
        properties: Node<'_, '_>,
        path: &[FieldStep],
        change: &PropertyChange,
    ) -> Result<Option<EditPlan>, EditError> {
        let Some(step) = path.first().filter(|_| path.len() == 1) else {
            return Ok(None);
        };
        if properties
            .children()
            .any(|child| child.has_tag_name((schema::MD, step.key.name.as_str())))
        {
            return Ok(None);
        }
        let plan = match change {
            PropertyChange::References { values } => {
                let class = schema::object_class(self.id);
                let rule = super::reference_lists::rule(&class, &step.key.name)
                    .ok_or(EditError::UnsupportedValue)?;
                super::reference_lists::insert(self.input, properties, rule, values)?
            }
            PropertyChange::Types { keys } => {
                let rule = super::type_sets::rule(self.id, &step.key.name)
                    .ok_or(EditError::UnsupportedValue)?;
                super::type_sets::insert(self.input, properties, rule, keys)?
            }
            _ => return Ok(None),
        };
        Ok(Some(plan))
    }

    /// Core parsing keeps object identity and inline descriptor handling identical to reading.
    pub(super) fn parsed(&self, input: &str) -> Result<ParsedDescriptor, EditError> {
        let result = if self.predefined {
            metadata_parser::parse_predefined(
                input,
                self.parent.as_ref().ok_or(EditError::UnsupportedValue)?,
                PropertiesMode::All,
            )
        } else {
            metadata_parser::parse(input, self.parent.clone(), PropertiesMode::All)
        };
        result.map_err(|_| EditError::InvalidXml)
    }

    /// Traverse only known writer contexts; an unknown record is not a generic XML editor.
    fn visit(
        &self,
        node: Node<'_, '_>,
        model_type: Option<&str>,
        path: &mut Vec<FieldStep>,
        fields: &mut Vec<EditableField>,
    ) {
        if path.len() > 64
            || (schema::protected(node.tag_name().name()) && !super::choice_links::is_name(node))
        {
            return;
        }
        if model_type == Some("TypeLink") {
            fields.extend(super::type_links::fields(node, path));
            return;
        }
        if matches!(model_type, Some("TypeItem" | "ReferenceTypeItem"))
            && node.has_tag_name((schema::CORE, "Type"))
        {
            if let Some(key) = super::types::key(node)
                .filter(super::types::supported)
                .filter(|_| {
                    node.parent()
                        .is_some_and(|parent| super::types::validate_shape(parent).is_ok())
                })
            {
                fields.push(EditableField {
                    path: path.clone(),
                    value: node.text().unwrap_or_default().to_owned(),
                    schema: ScalarSchema::DataType {
                        key,
                        reference_only: model_type == Some("ReferenceTypeItem"),
                    },
                    language: None,
                });
            }
            return;
        }
        if model_type == Some("ChoiceParameter") && super::choice_parameters::is_item(node) {
            fields.push(EditableField {
                path: path.clone(),
                value: node.attribute("name").unwrap_or_default().to_owned(),
                schema: ScalarSchema::Text {
                    domain: Some(super::TextDomain::ChoiceParameterName),
                },
                language: None,
            });
        }
        if let Some(schema) = schema::scalar(node, model_type, self.profile) {
            let value: String = node
                .children()
                .filter(Node::is_text)
                .filter_map(|node| node.text())
                .collect();
            if matches!(schema, ScalarSchema::Value { .. })
                || super::choice_fields::is_path(node)
                || schema.validate(&value).is_ok()
            {
                let language = node
                    .parent()
                    .filter(|parent| parent.has_tag_name((schema::CORE, "item")))
                    .and_then(|parent| {
                        parent
                            .children()
                            .find(|child| child.has_tag_name((schema::CORE, "lang")))
                    })
                    .and_then(|language| language.text())
                    .map(str::to_owned);
                fields.push(EditableField {
                    path: path.clone(),
                    value,
                    schema,
                    language,
                });
            }
            return;
        }
        let Some(class) = model_type else {
            return;
        };
        for (child, step) in children(node) {
            let kind = child_type(class, node, child, &path[0].key.name, self.profile);
            path.push(step);
            self.visit(child, kind, path, fields);
            path.pop();
        }
    }
}

fn container_field(name: &str, schema: ScalarSchema) -> EditableField {
    EditableField {
        path: vec![FieldStep {
            key: PropertyKey {
                namespace: Some(schema::MD.to_owned()),
                name: name.to_owned(),
            },
            occurrence: 0,
        }],
        value: String::new(),
        schema,
        language: None,
    }
}

/// Resolve a child only in the reviewed writer context of its parent record.
fn child_type<'a>(
    class: &'a str,
    parent: Node<'_, '_>,
    child: Node<'_, '_>,
    root: &str,
    profile: EditingProfile,
) -> Option<&'a str> {
    if (super::references::target(class).is_some()
        && child.has_tag_name((schema::READABLE, "Item")))
        || (class == "LocalStringMapEntry" && child.has_tag_name((schema::CORE, "item")))
        || (class == "ChoiceParameterLink" && super::choice_links::is_link(child))
        || (class == "ChoiceParameter" && super::choice_parameters::is_item(child))
    {
        Some(class)
    } else if matches!(class, "ChoiceParameter" | "Value")
        && super::choice_parameters::is_value(child)
    {
        Some("Value")
    } else if class == "ChoiceParameterLink" {
        if super::choice_fields::is_path(child) {
            return Some(super::choice_fields::DOMAIN);
        }
        super::choice_links::is_link(parent)
            .then(|| schema::field_type(class, child, false, profile))
            .flatten()
    } else if class == "LocalStringMapEntry" && child.has_tag_name((schema::CORE, "content")) {
        Some("EString")
    } else if matches!(
        class,
        "TypeDescription" | "SourceReferenceTypeDescription" | "MdObjectReferenceTypeDescription"
    ) && child.tag_name().namespace() == Some(schema::CORE)
    {
        match child.tag_name().name() {
            "StringQualifiers" | "NumberQualifiers" | "DateQualifiers" | "BinaryDataQualifiers" => {
                Some(class)
            }
            "Type" => Some(if class == "TypeDescription" {
                "TypeItem"
            } else {
                "ReferenceTypeItem"
            }),
            _ => schema::nested_type(root, parent, child),
        }
    } else if (matches!(class, "UsedFunctionality" | "RequiredPermission")
        && child.tag_name().namespace() == Some(schema::APP)
        && matches!(child.tag_name().name(), "functionality" | "permission")
        && child.children().any(|node| node.is_element()))
        || (child.has_tag_name(("http://v8.1c.ru/8.3/xcf/readable", "StandardAttribute"))
            && class == "StandardAttribute")
    {
        Some(class)
    } else {
        schema::field_type(class, child, false, profile)
            .or_else(|| schema::nested_type(root, parent, child))
    }
}

/// Revalidate sibling constraints in the candidate, keeping byte positions relative to unchanged ancestors.
pub(super) fn validate_dependencies(node: Node<'_, '_>, output: &str) -> Result<(), EditError> {
    let candidate = roxmltree::Document::parse(output).map_err(|_| EditError::InvalidXml)?;
    super::standard::validate_dependents(node, &candidate)?;
    super::lengths::validate_dependents(node, &candidate)?;
    super::hierarchy::validate_dependents(node, &candidate)?;
    super::ext_dimensions::validate_dependents(node, &candidate)?;
    super::choice_links::validate_dependents(node, &candidate)?;
    super::choice_parameters::validate_dependents(node, &candidate)?;
    super::choice_fields::validate_dependents(node, &candidate)?;
    validate_common_module_global(node, &candidate)?;
    if let Some(description) = node
        .ancestors()
        .find(|ancestor| ancestor.has_tag_name((schema::MD, "Type")))
    {
        let description = candidate
            .descendants()
            .find(|candidate| {
                candidate.has_tag_name((schema::MD, "Type"))
                    && candidate.range().start == description.range().start
            })
            .ok_or(EditError::InvalidXml)?;
        super::value_schema::validate_dependents(description)?;
    }
    if let Some(parent) = node
        .parent()
        .filter(|parent| parent.has_tag_name((schema::CORE, "NumberQualifiers")))
    {
        let group = candidate
            .descendants()
            .find(|node| node.is_element() && node.range().start == parent.range().start)
            .ok_or(EditError::InvalidXml)?;
        super::types::validate_numbers(group)?;
    }
    Ok(())
}

/// A global module may use a reserved property name until its Global flag is disabled.
fn validate_common_module_global(
    node: Node<'_, '_>,
    candidate: &roxmltree::Document<'_>,
) -> Result<(), EditError> {
    if !node.has_tag_name((schema::MD, "Global"))
        || !node
            .parent()
            .and_then(|properties| properties.parent())
            .is_some_and(|owner| owner.has_tag_name((schema::MD, "CommonModule")))
    {
        return Ok(());
    }
    let global = candidate
        .descendants()
        .find(|current| {
            current.has_tag_name((schema::MD, "Global"))
                && current.range().start == node.range().start
        })
        .ok_or(EditError::InvalidXml)?;
    if !matches!(global.text(), Some("false" | "0")) {
        return Ok(());
    }
    let name = global
        .parent()
        .and_then(|properties| {
            properties
                .children()
                .find(|property| property.has_tag_name((schema::MD, "Name")))
        })
        .and_then(|name| name.text())
        .ok_or(EditError::UnsupportedValue)?;
    if crate::project::metadata_rename::validate_common_module_name(name).is_err() {
        return Err(EditError::IncompatibleProperty(PropertyKey {
            namespace: None,
            name: "Name".into(),
        }));
    }
    Ok(())
}

/// Each step includes the expanded XML name, including repeated siblings in collection values.
fn children<'a, 'input>(node: Node<'a, 'input>) -> Vec<(Node<'a, 'input>, FieldStep)> {
    let mut result: Vec<(Node<'_, '_>, FieldStep)> = Vec::new();
    for child in node.children().filter(Node::is_element) {
        let key = PropertyKey {
            namespace: child.tag_name().namespace().map(str::to_owned),
            name: child.tag_name().name().to_owned(),
        };
        let occurrence = result.iter().filter(|(_, step)| step.key == key).count();
        result.push((child, FieldStep { key, occurrence }));
    }
    result
}

fn validate_change(field: &EditableField, change: &PropertyChange) -> Result<(), EditError> {
    match (&field.schema, change) {
        (ScalarSchema::Types { domain, .. }, PropertyChange::Types { keys }) => {
            super::type_sets::validate(*domain, keys)?;
        }
        (ScalarSchema::References { domain, .. }, PropertyChange::References { values }) => {
            super::reference_lists::validate(domain, values)?;
        }
        (ScalarSchema::Value { domain, types, .. }, PropertyChange::Value { key, value }) => {
            super::value_schema::validate_change(*domain, types, key.as_ref(), value)?;
        }
        (
            ScalarSchema::Value { .. }
            | ScalarSchema::References { .. }
            | ScalarSchema::Types { .. },
            _,
        )
        | (
            _,
            PropertyChange::Value { .. }
            | PropertyChange::References { .. }
            | PropertyChange::Types { .. },
        ) => {
            return Err(EditError::InvalidValue);
        }
        (ScalarSchema::DataType { reference_only, .. }, PropertyChange::DataType { key }) => {
            if *reference_only && key.namespace.as_deref() != Some(super::types::CFG) {
                return Err(EditError::InvalidValue);
            }
        }
        (ScalarSchema::DataType { .. }, PropertyChange::Text { .. })
        | (_, PropertyChange::DataType { .. }) => return Err(EditError::InvalidValue),
        (schema, PropertyChange::Text { value }) => schema.validate(value)?,
    }
    Ok(())
}
