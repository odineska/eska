//! Semantic managed-form changes over the existing lossless form/property models.

mod appearance;
mod document;
mod operations;
mod render;
mod schema;
mod types;

#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::{
    forms::FormParseError,
    metadata_edit::{EditError, EditPlan, Replacement},
    metadata_model::{MetadataProperty, ObjectId, PropertyKey},
    metadata_rename::NameError,
    metadata_workspace::{RefreshReport, WorkspaceError},
};

pub use appearance::AppearanceRule;
pub use document::{empty_form, plan};
pub(crate) use document::{plan_complete, removed_targets};
pub use schema::{
    FormDefinitionSchema, FormEditNode, FormEditProperty, FormEditSchema, FormElementKindSchema,
    child_kinds, inspect,
};
pub use types::FormTypeOption;
pub(crate) use types::{type_options, type_reference};

/// Integer identities belong to elements; definition names remain stable CLI selectors.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FormTarget {
    Root {},
    Element { id: i64 },
    Attribute { name: String },
    Command { name: String },
    Parameter { name: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormDefinitionKind {
    Attribute,
    Command,
    Parameter,
}

impl FormDefinitionKind {
    pub(super) const fn section(self) -> &'static str {
        match self {
            Self::Attribute => "Attributes",
            Self::Command => "Commands",
            Self::Parameter => "Parameters",
        }
    }

    pub(super) const fn tag(self) -> &'static str {
        match self {
            Self::Attribute => "Attribute",
            Self::Command => "Command",
            Self::Parameter => "Parameter",
        }
    }
}

/// New subtrees allocate identities themselves; callers cannot submit arbitrary local IDs.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormElementSpec {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub properties: Vec<MetadataProperty>,
    #[serde(default)]
    pub children: Vec<Self>,
    #[serde(default)]
    pub auxiliary: Vec<Self>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FormOperation {
    SetConditionalAppearance {
        rules: Vec<AppearanceRule>,
    },
    SetListConditionalAppearance {
        attribute: String,
        rules: Vec<AppearanceRule>,
    },
    SetProperty {
        target: FormTarget,
        property: MetadataProperty,
    },
    RemoveProperty {
        target: FormTarget,
        key: PropertyKey,
    },
    SetEvent {
        target: FormTarget,
        event: String,
        handler: Option<String>,
    },
    AddElement {
        parent: FormTarget,
        #[serde(default)]
        before: Option<i64>,
        element: FormElementSpec,
    },
    CopyElement {
        target: FormTarget,
        parent: FormTarget,
        #[serde(default)]
        before: Option<i64>,
        name: String,
    },
    MoveElement {
        target: FormTarget,
        parent: FormTarget,
        #[serde(default)]
        before: Option<i64>,
    },
    AddDefinition {
        section: FormDefinitionKind,
        #[serde(default)]
        parent: Option<FormTarget>,
        name: String,
        #[serde(default)]
        properties: Vec<MetadataProperty>,
    },
    Remove {
        target: FormTarget,
        #[serde(default)]
        force: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormEditRequest {
    pub form: ObjectId,
    pub snapshot: String,
    pub operations: Vec<FormOperation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedFormEdit {
    pub request: FormEditRequest,
    pub source_snapshot: String,
    pub expected_output_snapshot: String,
}

/// An acknowledgement reports allocated identities and never implies a database update.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignedFormId {
    pub operation: usize,
    pub target: FormTarget,
    pub id: Option<i64>,
}

#[derive(Debug)]
pub struct FormEditPlan {
    pub path: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub prepared: PreparedFormEdit,
    pub assigned_ids: Vec<AssignedFormId>,
    pub(crate) edit: EditPlan,
    pub(crate) input_snapshots: BTreeMap<PathBuf, String>,
}

pub(crate) struct RemovedFormTarget {
    pub target: FormTarget,
    pub names: Vec<String>,
    pub force: bool,
}

pub(crate) struct FormChanges {
    pub edit: EditPlan,
    pub assigned_ids: Vec<AssignedFormId>,
    pub removed: Vec<RemovedFormTarget>,
}

impl FormEditPlan {
    #[must_use]
    pub fn output(&self) -> &str {
        self.edit.output()
    }

    #[must_use]
    pub fn original(&self) -> &str {
        self.edit.original()
    }

    #[must_use]
    pub fn replacements(&self) -> &[Replacement] {
        self.edit.replacements()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.edit.is_empty()
    }
}

#[derive(Debug)]
pub struct FormEditResult {
    pub form: ObjectId,
    pub snapshot: String,
    pub assigned_ids: Vec<AssignedFormId>,
    pub refresh: Option<RefreshReport>,
}

#[derive(Debug, Default, Serialize)]
pub struct FormEditHistory {
    pub undo: bool,
    pub redo: bool,
}

#[derive(Debug)]
pub enum FormEditError {
    Edit(EditError),
    Workspace(Box<WorkspaceError>),
    Parse(FormParseError),
    Name(NameError),
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
    MissingTarget(FormTarget),
    Collision(String),
    References {
        target: FormTarget,
        paths: Vec<String>,
    },
    Committed(Box<WorkspaceError>),
}

impl From<EditError> for FormEditError {
    fn from(value: EditError) -> Self {
        Self::Edit(value)
    }
}

impl From<WorkspaceError> for FormEditError {
    fn from(value: WorkspaceError) -> Self {
        Self::Workspace(Box::new(value))
    }
}

impl From<FormParseError> for FormEditError {
    fn from(value: FormParseError) -> Self {
        Self::Parse(value)
    }
}

pub(super) const fn invalid(field: &'static str, reason: &'static str) -> FormEditError {
    FormEditError::Invalid { field, reason }
}
