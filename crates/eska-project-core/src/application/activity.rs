//! IDE activity projects ownership through the existing application roles and dependency graph.

use super::{SettingsError, graph};
use crate::project::{ProjectName, Workspace, activity::State};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// An active consumer is scoped to its declared application, including shared physical projects.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Consumer {
    pub application: ProjectName,
    pub project: ProjectName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectActivity {
    pub enabled: bool,
    pub required_by: BTreeSet<Consumer>,
}

/// Metadata ownership is per project; semantic resources belong to the effective application sources.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActivityProjection {
    pub projects: BTreeMap<ProjectName, ProjectActivity>,
    pub applications: BTreeMap<ProjectName, BTreeSet<ProjectName>>,
}

impl ActivityProjection {
    /// Resolve only checked manifests: no source file, metadata index or analyzer is read or started.
    ///
    /// # Errors
    /// Rejects unknown members or malformed extension edges using the existing application errors.
    pub fn for_workspace(workspace: &Workspace, state: &State) -> Result<Self, SettingsError> {
        let mut projection = Self {
            projects: workspace
                .members()
                .iter()
                .map(|member| {
                    (
                        member.name().clone(),
                        ProjectActivity {
                            enabled: state.enabled(member.root()),
                            required_by: BTreeSet::new(),
                        },
                    )
                })
                .collect(),
            applications: BTreeMap::new(),
        };
        for (name, application) in workspace.analysis_settings().applications() {
            let extensions = graph::ordered_extensions(name, application)?;
            let members: Vec<_> = application
                .configuration()
                .into_iter()
                .chain(application.extensions())
                .chain(application.external_objects())
                .collect();
            let mut effective = BTreeSet::new();
            for member in members {
                let active = projection.projects.get(member).ok_or_else(|| {
                    SettingsError::UnknownMember {
                        application: name.clone(),
                        member: member.clone(),
                    }
                })?;
                if !active.enabled {
                    continue;
                }
                let mut needed = BTreeSet::from([member.clone()]);
                if application.configuration() != Some(member) {
                    if let Some(base) = application.configuration() {
                        needed.insert(base.clone());
                    }
                    if application.external_objects().contains(member) {
                        // An external object uses the application's complete runtime extension context.
                        needed.extend(extensions.iter().cloned());
                    } else {
                        let mut pending = vec![member];
                        while let Some(current) = pending.pop() {
                            for dependency in application
                                .extension_dependencies()
                                .get(current)
                                .into_iter()
                                .flatten()
                            {
                                if needed.insert(dependency.clone()) {
                                    pending.push(dependency);
                                }
                            }
                        }
                    }
                }
                let consumer = Consumer {
                    application: name.clone(),
                    project: member.clone(),
                };
                for dependency in needed {
                    let activity = projection.projects.get_mut(&dependency).ok_or_else(|| {
                        SettingsError::UnknownMember {
                            application: name.clone(),
                            member: dependency.clone(),
                        }
                    })?;
                    if !activity.enabled {
                        activity.required_by.insert(consumer.clone());
                    }
                    effective.insert(dependency);
                }
            }
            projection.applications.insert(name.clone(), effective);
        }
        Ok(projection)
    }
}
