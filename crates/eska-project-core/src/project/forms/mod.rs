//! Read-only managed-form documents with source-qualified identities and lossless source ranges.

mod appearance;
mod based_on;
mod borrowing;
mod catalog;
mod command_types;
mod commands;
mod composition;
mod context;
mod handlers;
mod layer_order;
mod palette;
mod parsing;
mod pictures;
mod preparation;
mod presentation;
mod references;
mod resources;
mod root_commands;
mod style_data;
mod table_commands;

pub use appearance::{AppearanceDiagnostic, ResolvedColor, ResolvedFont};
pub use catalog::{ElementFamily, ElementKind};
pub use commands::AutoCommandItem;
pub use composition::{
    ComposedForm, CompositionIssue, EffectiveDefinition, EffectiveElement,
    FormCompositionDiagnostic, PropertySource, SourcedProperty, SourcedQualifier, compose,
};
pub use context::{
    FormContextError, FormDependency, FormDependencyRole, FormOverlay, FormPreview, load,
};
pub use handlers::{
    FormHandler, FormHandlerOwner, FormHandlerSource, HandlerLookupError, handler_range,
    handler_source, handlers,
};
pub use palette::{
    PaletteCaption, PaletteCategory, PaletteEditor, PaletteRow, PaletteValue,
    PlatformPropertyValue, PropertyPalette,
};
pub use parsing::parse;
pub use pictures::{ButtonRepresentation, FormPicture};
pub use preparation::{FormInput, FormInputSource, FormPreparation, prepare};
pub use presentation::{
    ElementPresentation, FieldTitleLocation, FieldType, FieldValueKind, FormGrouping,
    FormPresentation, FormScriptVariant, NumberSign, PresentationText, TableColumnAppearance,
    TabularTotalCaption, present,
};
pub use references::{FormReference, ReferenceSlot};
pub use resources::FormResource;
pub use style_data::{FormStyleData, StyleValue};

use std::ops::Range;

use serde::Serialize;

use super::{
    metadata_model::{MetadataProperty, ObjectId, PropertyKey},
    metadata_parser::LocatedProperty,
};

pub const FORM_NAMESPACE: &str = "http://v8.1c.ru/8.3/xcf/logform";
const DEFAULT_GROUP_UNITED: bool = true;
pub const MAX_FORM_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FORM_ELEMENTS: usize = 16_384;
pub const MAX_FORM_CONTEXT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FORM_DEPENDENCIES: usize = 1024;

fn is_form_property(key: &PropertyKey, name: &str) -> bool {
    key.name == name && key.namespace.as_deref() == Some(FORM_NAMESPACE)
}

/// Root identity cannot collide with any user-provided XML id, including negative helper ids.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ElementKey {
    Root,
    Xml(i64),
}

/// The caller supplies the immutable source identity, including its application/extension context.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementIdentity {
    pub source: String,
    pub part: FormPart,
    pub key: ElementKey,
}

/// A stored borrowing baseline is distinct from the editable/current XML tree in the same file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormPart {
    Current,
    StoredBase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FormIssue {
    UnknownElement,
    InvalidIdentity,
    DuplicateIdentity,
    MissingName,
    RepeatedSection,
    UnknownDefinition,
    UnsupportedProperty,
    UnsupportedStructure,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDiagnostic {
    pub issue: FormIssue,
    pub range: Range<usize>,
    pub key: PropertyKey,
}

/// Known and unknown elements retain their original attributes and properties in declaration order.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormElement {
    pub identity: Option<ElementIdentity>,
    pub kind: Option<ElementKind>,
    pub xml_name: PropertyKey,
    pub name: Option<String>,
    pub range: Range<usize>,
    pub qualifiers: Vec<LocatedQualifier>,
    pub properties: Vec<LocatedProperty>,
    pub children: Vec<Self>,
    /// Menus, tooltips and table additions are not ordinary layout children.
    pub auxiliary: Vec<Self>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocatedQualifier {
    pub key: PropertyKey,
    pub text: String,
    pub range: Range<usize>,
}

impl FormElement {
    /// Qualified lookup prevents foreign XML properties from impersonating platform properties.
    #[must_use]
    pub fn property(&self, name: &str) -> Option<&MetadataProperty> {
        self.properties
            .iter()
            .find(|value| is_form_property(&value.property.key, name))
            .map(|value| &value.property)
    }

    fn find(&self, identity: &ElementIdentity) -> Option<&Self> {
        if self.identity.as_ref() == Some(identity) {
            return Some(self);
        }
        self.children
            .iter()
            .chain(&self.auxiliary)
            .find_map(|child| child.find(identity))
    }
}

/// Definitions have independent id spaces; an attribute id is not an element id.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDefinition {
    pub xml_name: PropertyKey,
    pub name: Option<String>,
    pub range: Range<usize>,
    pub qualifiers: Vec<(PropertyKey, String)>,
    pub properties: Vec<LocatedProperty>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDocument {
    pub source: String,
    pub version: Option<String>,
    pub revision: String,
    pub root: FormElement,
    pub attributes: Vec<FormDefinition>,
    /// The native format can carry conditional appearance alongside attribute definitions.
    pub attribute_properties: Vec<LocatedProperty>,
    pub commands: Vec<FormDefinition>,
    pub parameters: Vec<FormDefinition>,
    pub diagnostics: Vec<FormDiagnostic>,
    pub base: Option<FormSnapshot>,
    pub references: Vec<FormReference>,
    #[serde(skip)]
    resource_ids: Vec<ObjectId>,
    /// Keep exact bytes for source navigation and future checked edits; never send them with the model.
    #[serde(skip)]
    input: String,
}

/// The native extension format embeds the original borrowed form inside `BaseForm`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormSnapshot {
    pub root: FormElement,
    pub attributes: Vec<FormDefinition>,
    pub attribute_properties: Vec<LocatedProperty>,
    pub commands: Vec<FormDefinition>,
    pub parameters: Vec<FormDefinition>,
    pub diagnostics: Vec<FormDiagnostic>,
}

impl FormDocument {
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    #[must_use]
    pub fn element(&self, identity: &ElementIdentity) -> Option<&FormElement> {
        (identity.source == self.source)
            .then(|| self.root.find(identity))
            .flatten()
    }

    #[must_use]
    pub const fn has_issues(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum FormParseError {
    InvalidSource,
    TooLarge,
    TooDeep,
    InvalidXml,
    UnsupportedRoot,
}

#[cfg(test)]
mod tests;
