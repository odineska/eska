//! Explicit application composition and read-only analysis contexts for Designer XML.

mod activity;
mod context;
mod graph;
mod policy;
mod reference;
mod settings;
mod snapshot;

pub use activity::{ActivityProjection, Consumer, ProjectActivity};
pub use context::{
    AnalysisContext, ContextError, ContextSource, LibrarySelection, SourceRole, resolve,
    resolve_active,
};
pub(crate) use policy::RawTools;
pub use policy::{AnalyzerPolicy, CheckPolicy, ToolSettings};
pub(crate) use reference::owner;
pub use reference::{ReferenceError, resolve_project_reference};
pub use settings::{
    AnalysisOptions, AnalysisSettings, Application, ConfigurationReference, ExtensionScope,
    SettingsError, TargetPlatformVersion,
};
pub(crate) use settings::{RawAnalysis, RawApplication, RawConfiguration};
pub use snapshot::{ContextSnapshot, resolve_snapshot};

#[cfg(test)]
mod tests;
