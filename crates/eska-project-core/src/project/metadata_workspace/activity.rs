//! Disabled projects retain only presentation and explicit edit ownership, never metadata indexes.

use super::{MetadataWorkspace, ProjectSession, WorkspaceError};
use crate::project::{
    Project,
    activity::{RootSummary, State, path_key},
    designer_source::{self, SourceError},
    discovery::DiscoveryContext,
    metadata_model::{ObjectId, ProjectScope},
    selection::{SelectionIntent, select_projects},
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct SuspendedProject {
    project: Project,
    scope: ProjectScope,
    summary: RootSummary,
    generation: u64,
    cached: bool,
    history: BTreeMap<ObjectId, super::editing_history::PropertyHistory>,
    protections: super::source_protection::SourceProtections,
}

#[derive(Debug)]
pub enum ActivityProject {
    Active(ProjectScope),
    Inactive(Box<SuspendedProject>),
}

pub struct ActivityWorkspace {
    pub workspace: MetadataWorkspace,
    pub projects: Vec<ActivityProject>,
}

/// A failed preparation returns the complete inactive state to its caller.
#[derive(Debug)]
pub struct ResumeFailure {
    pub error: Box<WorkspaceError>,
    pub project: Box<SuspendedProject>,
}

/// Exclusive borrowing keeps the preflight valid while the activity preference is committed.
#[derive(Debug)]
pub struct PreparedResume<'workspace> {
    workspace: &'workspace mut MetadataWorkspace,
    previous: Box<SuspendedProject>,
    active: ProjectSession,
}

impl SuspendedProject {
    #[must_use]
    pub const fn project(&self) -> &Project {
        &self.project
    }
    #[must_use]
    pub const fn scope(&self) -> &ProjectScope {
        &self.scope
    }
    #[must_use]
    pub const fn summary(&self) -> &RootSummary {
        &self.summary
    }
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    fn declared(project: Project, scope: ProjectScope, summary: RootSummary, cached: bool) -> Self {
        Self {
            project,
            scope,
            summary,
            generation: 0,
            cached,
            history: BTreeMap::new(),
            protections: super::source_protection::SourceProtections::default(),
        }
    }

    /// Explicit source protection reads may borrow a transient session without starting an index.
    ///
    /// # Errors
    /// Preserves checked source/settings errors; the inactive state is never modified.
    pub fn protection_session(&self) -> Result<ProjectSession, WorkspaceError> {
        let mut session = self.open(self.generation)?;
        session.source_protections = self.protections.clone();
        Ok(session)
    }

    fn open(&self, generation: u64) -> Result<ProjectSession, WorkspaceError> {
        let mut sources =
            designer_source::open_projects_cached(self.project.root(), &[], false, self.cached)
                .map_err(WorkspaceError::Source)?;
        let source = sources
            .pop()
            .ok_or_else(|| WorkspaceError::SourceChanged(self.project.root().into()))?;
        if !sources.is_empty() || source.project() != &self.project || source.scope() != &self.scope
        {
            return Err(WorkspaceError::Source(SourceError::InvalidMetadata {
                path: self.project.root().join("eska.toml"),
                reason: "project settings changed",
            }));
        }
        let mut session = ProjectSession::open(source)?;
        session.generation = generation;
        Ok(session)
    }
}

impl ProjectSession {
    /// Capture only the stable root label and identity for an inactive IDE row.
    #[must_use]
    pub fn activity_summary(&self) -> RootSummary {
        RootSummary {
            id: self.source.root().id().clone(),
            name: self.source.root().name().into(),
        }
    }
}

impl MetadataWorkspace {
    /// Open only active root descriptors; cached disabled summaries need no XML or disk cache.
    ///
    /// # Errors
    /// Returns existing selection/source errors without publishing a partial workspace.
    pub fn open_activity(
        context: &DiscoveryContext,
        names: &[String],
        entire_workspace: bool,
        cached: bool,
        activity: &State,
    ) -> Result<ActivityWorkspace, WorkspaceError> {
        let selection =
            select_projects(context, names, entire_workspace, SelectionIntent::ReadOnly)
                .map_err(|error| WorkspaceError::Source(SourceError::Selection(error)))?;
        let mut workspace = Self {
            projects: Vec::new(),
        };
        let mut projects = Vec::new();
        for selected in selection.projects() {
            let project = selected.project().clone();
            let scope = selected.name().map_or(ProjectScope::Standalone, |name| {
                ProjectScope::Member(name.clone())
            });
            if matches!(scope, ProjectScope::Member(_))
                && let Some(summary) = activity.disabled.get(&path_key(project.root()))
            {
                projects.push(ActivityProject::Inactive(Box::new(
                    SuspendedProject::declared(project, scope, summary.clone(), cached),
                )));
            } else {
                let source = designer_source::open_project(project, scope.clone(), cached)
                    .map_err(WorkspaceError::Source)?;
                workspace.projects.push(ProjectSession::open(source)?);
                projects.push(ActivityProject::Active(scope));
            }
        }
        Ok(ActivityWorkspace {
            workspace,
            projects,
        })
    }

    /// Drop the source reader, metadata caches and background search while preserving explicit edits.
    ///
    /// # Errors
    /// Returns unknown projects or generation exhaustion before removing any session.
    pub fn suspend(
        &mut self,
        scope: &ProjectScope,
    ) -> Result<Box<SuspendedProject>, WorkspaceError> {
        let index = self
            .projects
            .iter()
            .position(|project| project.scope() == scope)
            .ok_or_else(|| WorkspaceError::UnknownProject(scope.clone()))?;
        let previous = &self.projects[index];
        let generation = previous
            .generation
            .checked_add(1)
            .ok_or(WorkspaceError::GenerationExhausted)?;
        let summary = previous.activity_summary();
        let project = previous.project().clone();
        let cached = previous.source.disk_cache.is_some();
        let previous = self.projects.remove(index);
        Ok(Box::new(SuspendedProject {
            project,
            scope: scope.clone(),
            summary,
            generation,
            cached,
            history: previous.property_history,
            protections: previous.source_protections,
        }))
    }

    /// Validate activation before persisting the preference; a failure retains the inactive owner.
    ///
    /// # Errors
    /// Returns checked source/generation failures together with the unchanged inactive state.
    pub fn prepare_resume(
        &mut self,
        project: Box<SuspendedProject>,
    ) -> Result<PreparedResume<'_>, ResumeFailure> {
        let generation = project
            .generation
            .checked_add(1)
            .ok_or(WorkspaceError::GenerationExhausted);
        let active = generation.and_then(|generation| {
            if self
                .projects
                .iter()
                .any(|active| active.scope() == project.scope())
            {
                return Err(WorkspaceError::SourceChanged(project.project.root().into()));
            }
            project.open(generation)
        });
        match active {
            Ok(active) => Ok(PreparedResume {
                workspace: self,
                previous: project,
                active,
            }),
            Err(error) => Err(ResumeFailure {
                error: Box::new(error),
                project,
            }),
        }
    }
}

impl PreparedResume<'_> {
    /// A failed preference write can return ownership without losing query restrictions or history.
    #[must_use]
    pub fn cancel(self) -> Box<SuspendedProject> {
        self.previous
    }

    /// Publish the prepared active session only after the caller has saved its preference.
    pub fn commit(self) {
        let mut active = self.active;
        active.property_history = self.previous.history;
        active.source_protections = self.previous.protections;
        self.workspace.projects.push(active);
    }
}

#[cfg(test)]
mod tests;
