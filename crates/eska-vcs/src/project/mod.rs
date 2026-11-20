//! Git-aware project operations over the shared Designer XML model.

pub use eska_project_core::project::*;

use std::path::Path;

use crate::vcs::repository::{self, Repository};
use discovery::{ContextDiscoveryError, DiscoveryContext};
use selection::SelectedProject;

pub mod clone;
pub mod debug_sources;
pub mod diff;
pub mod finish;
pub mod history;
pub mod patch;
pub mod save;
pub mod semantic;
pub mod start;
pub mod status;
pub mod switch;

pub(crate) enum RepositoryContextError {
    Repository(repository::Error),
    Project(ContextDiscoveryError),
}

/// Resolve the optional ESKA context inside the worktree that owns this directory.
pub(crate) fn repository_context(
    start: &Path,
) -> Result<(Repository, Option<DiscoveryContext>), RepositoryContextError> {
    let repository = Repository::discover(start).map_err(RepositoryContextError::Repository)?;
    let context = discovery::discover_context_within(start, repository.work_dir())
        .map_err(RepositoryContextError::Project)?;
    Ok((repository, context))
}

/// A selection from another workspace member may have its own nested worktree.
pub(crate) fn selected_repository(
    repository: Repository,
    context: &DiscoveryContext,
    selected: SelectedProject<'_>,
) -> Result<Repository, repository::Error> {
    if let DiscoveryContext::Workspace { current_member, .. } = context
        && selected.name() != current_member.as_ref()
    {
        return Repository::discover(selected.project().root());
    }
    Ok(repository)
}
