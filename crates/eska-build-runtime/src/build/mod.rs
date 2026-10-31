//! Build settings, artifact planning and execution for Designer XML projects.

mod base_configuration;
mod execute;
mod infobase;
mod manifest;
mod plan;
mod shared_library;
mod source_snapshot;
pub use source_snapshot::Error as SourceSnapshotError;
mod tool;

pub use eska_project_core::project::build::{
    BuildSettings, BuildSettingsError, InvalidArtifactsDirectoryReason, PlatformVersion,
};
pub use execute::{
    BuildError, BuildResult, BuildStage, execute, execute_streaming,
    execute_streaming_with_manifest, preflight, preflight_manifest,
};
pub use infobase::{
    CleanOutcome as InfobaseCleanOutcome, ManagedInfobaseError, clean as clean_infobase,
};
pub use manifest::{GitProvenance, ManifestError, path_for_artifact as manifest_path};
pub use plan::{
    ArtifactType, BaseConfiguration, BuildPlan, PlanError, managed_infobase_root,
    validate_unique_outputs,
};
pub use tool::{
    Ibcmd, InstalledPlatform, ProcessStream, RunError, RunnerPreference, ToolError, ToolOptions,
    ToolSource,
};
