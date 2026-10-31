//! Resolve only the read-only configuration belonging to a selected project.

use std::path::PathBuf;

use crate::project::{Project, ProjectName, Workspace};

use super::{
    Application, ConfigurationReference, ContextError, ContextSource, SettingsError,
    context::{library_source_error, overlaps, read_configuration_reference},
};

/// Retain the declaration's coordinates without loading unrelated application sources.
#[derive(Debug)]
pub struct ReferenceError {
    pub path: PathBuf,
    pub field: &'static str,
    pub application: Option<ProjectName>,
    pub source: Box<ContextError>,
}

/// Resolve the selected member's own application, independent of the workspace default.
///
/// # Errors
/// Rejects ambiguous ownership, invalid references and overlap with writable members.
/// A project without a reference does not require a global configuration or root XML.
pub fn resolve_project_reference(
    project: &Project,
    workspace: Option<&Workspace>,
) -> Result<Option<ContextSource>, ReferenceError> {
    let (root, field, application, declaration) = if let Some(workspace) = workspace {
        let owner = owner(project, workspace).map_err(|source| ReferenceError {
            path: workspace.root().join("eska.toml"),
            field: "analysis.default_application",
            application: None,
            source: Box::new(source),
        })?;
        match owner {
            Some((name, application)) => (
                workspace.root(),
                "configuration",
                Some(name.clone()),
                application.configuration_reference(),
            ),
            None => (
                project.root(),
                "analysis.configuration",
                None,
                project
                    .configuration()
                    .analysis_settings()
                    .options()
                    .configuration(),
            ),
        }
    } else {
        (
            project.root(),
            "analysis.configuration",
            None,
            project
                .configuration()
                .analysis_settings()
                .options()
                .configuration(),
        )
    };
    let Some(declaration) = declaration else {
        return Ok(None);
    };
    resolve_declared(project, workspace, root, declaration)
        .map(Some)
        .map_err(|source| ReferenceError {
            path: root.join("eska.toml"),
            field,
            application,
            source: Box::new(source),
        })
}

/// A shared member needs a default that actually owns it, not an unrelated global choice.
pub fn owner<'a>(
    project: &Project,
    workspace: &'a Workspace,
) -> Result<Option<(&'a ProjectName, &'a Application)>, ContextError> {
    let Some(member) = workspace
        .members()
        .iter()
        .find(|member| member.root() == project.root())
    else {
        return Ok(None);
    };
    let settings = workspace.analysis_settings();
    let owners: Vec<_> = settings
        .applications()
        .iter()
        .filter(|(_, application)| {
            application.configuration() == Some(member.name())
                || application.extensions().contains(member.name())
                || application.external_objects().contains(member.name())
        })
        .collect();
    match owners.as_slice() {
        [] => Ok(None),
        [owner] => Ok(Some(*owner)),
        _ => settings
            .options()
            .default_application()
            .and_then(|default| owners.iter().find(|(name, _)| *name == default).copied())
            .map(Some)
            .ok_or_else(|| ContextError::AmbiguousApplication {
                available: owners.iter().map(|(name, _)| (*name).clone()).collect(),
            }),
    }
}

fn resolve_declared(
    project: &Project,
    workspace: Option<&Workspace>,
    root: &std::path::Path,
    declaration: &ConfigurationReference,
) -> Result<ContextSource, ContextError> {
    let reference = read_configuration_reference(root, declaration)?;
    if overlaps(reference.project().source(), project.root())
        || workspace.is_some_and(|workspace| {
            workspace
                .members()
                .iter()
                .any(|member| overlaps(reference.project().source(), member.root()))
        })
    {
        return Err(library_source_error(
            &reference,
            ContextError::Settings(Box::new(SettingsError::ConfigurationReferenceScope)),
        ));
    }
    Ok(reference)
}
