//! Structural metadata operations preserve owned resources and distinguish identity from references.

pub(crate) mod inventory;
pub(crate) mod templates;
pub(crate) mod xml;

use std::{collections::BTreeMap, ops::Range, path::PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    metadata_edit::EditError,
    metadata_model::{MetadataKind, ObjectId, PropertyKey},
    metadata_rename::NameError,
    metadata_workspace::{RefreshReport, WorkspaceError},
    metadata_write::{DirectoryChange, FileChange},
};

/// Semantic input contains metadata identities; callers never select arbitrary destination paths.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ObjectRequest {
    Copy {
        source: ObjectId,
        name: String,
        #[serde(default)]
        parent: Option<ObjectId>,
        #[serde(default)]
        property_values: BTreeMap<String, String>,
        #[serde(default)]
        property_references: BTreeMap<String, Vec<String>>,
        #[serde(default)]
        property_types: BTreeMap<String, Vec<PropertyKey>>,
    },
    Create {
        kind: MetadataKind,
        name: String,
        #[serde(default)]
        parent: Option<ObjectId>,
        #[serde(default)]
        template: Option<ObjectId>,
        #[serde(default)]
        property_values: BTreeMap<String, String>,
        #[serde(default)]
        property_references: BTreeMap<String, Vec<String>>,
        #[serde(default)]
        property_types: BTreeMap<String, Vec<PropertyKey>>,
    },
    Delete {
        object: ObjectId,
        #[serde(default)]
        force: bool,
    },
}

/// A portable plan can be replayed only against the exact discovery inputs and reviewed result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedObject {
    pub request: ObjectRequest,
    pub source_snapshot: String,
    pub identities: BTreeMap<String, String>,
    pub expected_output_snapshot: String,
}

/// Proven semantic references are separate from source text that cannot safely be resolved.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectReference {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub reason: &'static str,
    pub confirmed: bool,
}

/// Actual file contents stay backend-owned; deserializing replacement bytes cannot authorize writes.
#[derive(Debug)]
pub struct ObjectPlan {
    pub prepared: PreparedObject,
    pub object_id: Option<ObjectId>,
    pub files: Vec<FileChange>,
    pub directories: Vec<DirectoryChange>,
    pub references: Vec<ObjectReference>,
    pub inputs: Vec<PathBuf>,
    pub(crate) input_checks: inventory::InputChecks,
}

/// Publication succeeded even if later client refresh needs a new session.
#[derive(Debug)]
pub struct ObjectResult {
    pub object_id: Option<ObjectId>,
    pub snapshot: String,
    pub refresh: RefreshReport,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectSchema {
    pub schema_version: u32,
    pub kinds: Vec<MetadataKind>,
    pub parent: Option<ObjectId>,
    pub operations: Vec<&'static str>,
    pub prepared_plans: bool,
    pub required_properties: BTreeMap<String, Vec<String>>,
    pub reference_properties: BTreeMap<String, Vec<String>>,
    pub type_properties: BTreeMap<String, Vec<String>>,
    pub unique_properties: BTreeMap<String, Vec<String>>,
}

/// Initial selectors reuse the same domains and object choices as existing property editors.
pub struct ObjectInitialField {
    pub property: String,
    pub choices: ObjectInitialChoices,
}

pub enum ObjectInitialChoices {
    Types(Vec<super::metadata_workspace::PropertyTypeChoice>),
    References {
        multiple: bool,
        choices: Vec<super::metadata_workspace::PropertyReferenceChoice>,
    },
    Values {
        domain: &'static str,
        values: Vec<String>,
    },
}

/// Structured failures preserve domain boundaries and never contain localized presentation text.
#[derive(Debug)]
pub enum ObjectError {
    Workspace(Box<WorkspaceError>),
    Edit(EditError),
    Name(NameError),
    Collision(ObjectId),
    Unsupported(MetadataKind),
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
    ReviewRequired(Vec<ObjectReference>),
    Committed(Box<WorkspaceError>),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Identity(getrandom::Error),
    PropertyConflict {
        property: String,
        value: String,
        object: ObjectId,
    },
}

impl From<WorkspaceError> for ObjectError {
    fn from(value: WorkspaceError) -> Self {
        Self::Workspace(Box::new(value))
    }
}

impl From<EditError> for ObjectError {
    fn from(value: EditError) -> Self {
        Self::Edit(value)
    }
}

pub(crate) const fn invalid(field: &'static str, reason: &'static str) -> ObjectError {
    ObjectError::Invalid { field, reason }
}
