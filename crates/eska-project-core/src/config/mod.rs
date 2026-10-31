//! Strict project/workspace manifests and shared machine-local configuration.

pub(crate) mod composition;
pub mod global;
mod manifest;
mod paths;
mod project;
mod schema;
mod workflow;
mod workspace;

pub use manifest::{ManifestConfig, ManifestConfigError};
pub use project::{InvalidSourceReason, ProjectConfig, ProjectConfigError};
pub use schema::parse_project_type;
pub use workspace::{InvalidMemberPathReason, WorkspaceConfig, WorkspaceConfigError};

/// Name shared by project discovery and manifest writers.
pub const FILE_NAME: &str = "eska.toml";
