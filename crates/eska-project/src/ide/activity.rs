//! Machine-local activity controls metadata ownership without changing CLI selection or manifests.

use super::{
    dto,
    envelope::{domain, error},
    errors, params,
    server::Server,
};
use eska_project_core::{
    application::ActivityProjection,
    config::global,
    project::{
        activity::{self, Snapshot, Store},
        designer_source::SourceError,
        discovery::{self, DiscoveryContext},
        metadata_model::{NodeId, ProjectScope},
        metadata_workspace::{MetadataWorkspace, WorkspaceError},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

pub(super) struct WorkspaceActivity {
    pub context: DiscoveryContext,
    pub store: Store,
    pub snapshot: Snapshot,
}

pub(super) fn is_method(method: &str) -> bool {
    matches!(method, "workspace/activitySet" | "workspace/activityFilter")
}

pub(super) fn open(
    start: &Path,
    names: &[String],
    all: bool,
    cached: bool,
) -> Result<
    (
        eska_project_core::project::metadata_workspace::ActivityWorkspace,
        Option<WorkspaceActivity>,
    ),
    Value,
> {
    let context = discovery::discover_context(start).map_err(|failure| {
        errors::opening(&WorkspaceError::Source(SourceError::Discovery(failure)))
    })?;
    let activity = match &context {
        DiscoveryContext::Standalone(_) => None,
        DiscoveryContext::Workspace { workspace, .. } => {
            let config = global::config_path().map_err(|_| failure("storage_unavailable"))?;
            let base = config
                .parent()
                .ok_or_else(|| failure("storage_unavailable"))?
                .join("state/workspaces");
            let store =
                Store::new(&base, workspace.root()).map_err(|error| storage_error(&error))?;
            let snapshot = store.load().map_err(|error| storage_error(&error))?;
            Some(WorkspaceActivity {
                context: context.clone(),
                store,
                snapshot,
            })
        }
    };
    let default = activity::State::default();
    let state = activity
        .as_ref()
        .map_or(&default, |activity| &activity.snapshot.state);
    let opened = MetadataWorkspace::open_activity(&context, names, all, cached, state)
        .map_err(|failure| errors::opening(&failure))?;
    Ok((opened, activity))
}

pub(super) fn projection(
    activity: Option<&WorkspaceActivity>,
) -> Result<Option<ActivityProjection>, Value> {
    let Some(activity) = activity else {
        return Ok(None);
    };
    let DiscoveryContext::Workspace { workspace, .. } = &activity.context else {
        return Err(error(-32603));
    };
    ActivityProjection::for_workspace(workspace, &activity.snapshot.state)
        .map(Some)
        .map_err(|_| failure("context_changed"))
}
fn details(projection: Option<&ActivityProjection>, scope: &ProjectScope) -> Result<Value, Value> {
    let Some(projection) = projection else {
        return Ok(json!({"enabled":true,"requiredBy":[]}));
    };
    let ProjectScope::Member(name) = scope else {
        return Err(error(-32603));
    };
    let project = projection
        .projects
        .get(name)
        .ok_or_else(|| failure("context_changed"))?;
    Ok(json!({"enabled":project.enabled,"requiredBy":project.required_by}))
}

impl Server {
    pub(super) fn activity(
        &mut self,
        method: &str,
        args: &Value,
        events: &mut Vec<Value>,
    ) -> Result<Value, Value> {
        #[derive(Deserialize)]
        struct Revision {
            revision: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Project {
            project_id: String,
            enabled: bool,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Filter {
            hide_disabled: bool,
        }
        self.check_session(args)?;
        let revision: Revision = params::decode(args)?;
        let session = self.session.as_mut().ok_or_else(|| error(-32603))?;
        let activity = session
            .activity
            .as_mut()
            .ok_or_else(|| failure("workspace_required"))?;
        if revision.revision != activity.snapshot.revision {
            return Err(failure("conflict"));
        }
        match method {
            "workspace/activityFilter" => {
                let input: Filter = params::decode(args)?;
                let mut next = activity.snapshot.state.clone();
                next.hide_disabled = input.hide_disabled;
                activity.snapshot = activity
                    .store
                    .save(&revision.revision, &next)
                    .map_err(|error| storage_error(&error))?;
            }
            "workspace/activitySet" => {
                let input: Project = params::decode(args)?;
                let state = session
                    .projects
                    .iter_mut()
                    .find(|state| state.id == input.project_id)
                    .ok_or_else(|| {
                        domain("unknown_project", json!({"projectId":input.project_id}))
                    })?;
                set_project(&mut session.workspace, state, activity, input.enabled)?;
            }
            _ => return Err(error(-32601)),
        }
        if let Some((_, _, plan)) = &self.analyzer_lease
            && let Some(projection) = projection(
                self.session
                    .as_ref()
                    .and_then(|session| session.activity.as_ref()),
            )?
            && plan.context.name().is_some_and(|name| {
                projection
                    .applications
                    .get(name)
                    .is_some_and(std::collections::BTreeSet::is_empty)
            })
        {
            self.analyzer_lease = None;
        }
        let info = self.info()?;
        events.push(json!({"jsonrpc":"2.0","method":"workspace/activityChanged","params":info}));
        Ok(info)
    }
}

fn set_project(
    workspace: &mut MetadataWorkspace,
    state: &mut super::server::ProjectState,
    activity: &mut WorkspaceActivity,
    enabled: bool,
) -> Result<(), Value> {
    if !matches!(state.scope, ProjectScope::Member(_)) {
        return Err(failure("workspace_required"));
    }
    let revision = &activity.snapshot.revision;
    let mut next = activity.snapshot.state.clone();
    if enabled == state.inactive.is_none() {
        activity
            .store
            .save(revision, &next)
            .map_err(|error| storage_error(&error))?;
        return Ok(());
    }
    let event = state
        .event
        .checked_add(1)
        .ok_or_else(|| domain("generation_exhausted", json!({})))?;
    if enabled {
        let inactive = state.inactive.take().ok_or_else(|| error(-32603))?;
        next.disabled
            .remove(&activity::path_key(inactive.project().root()));
        let prepared = match workspace.prepare_resume(inactive) {
            Ok(prepared) => prepared,
            Err(failed) => {
                state.inactive = Some(failed.project);
                return Err(errors::opening(&failed.error));
            }
        };
        match activity.store.save(revision, &next) {
            Ok(saved) => {
                activity.snapshot = saved;
                prepared.commit();
            }
            Err(error) => {
                state.inactive = Some(prepared.cancel());
                return Err(storage_error(&error));
            }
        }
    } else {
        let project = workspace
            .project(&state.scope)
            .map_err(|failure| errors::workspace(&failure))?;
        project
            .generation()
            .checked_add(1)
            .ok_or_else(|| domain("generation_exhausted", json!({})))?;
        next.disabled.insert(
            activity::path_key(project.project().root()),
            project.activity_summary(),
        );
        activity.snapshot = activity
            .store
            .save(revision, &next)
            .map_err(|error| storage_error(&error))?;
        state.inactive = Some(
            workspace
                .suspend(&state.scope)
                .map_err(|failure| errors::workspace(&failure))?,
        );
    }
    state.file = 0;
    state.event = event;
    state.last_progress = None;
    state.refresh = false;
    Ok(())
}

pub(super) fn project_info(
    session: &super::server::Session,
    state: &super::server::ProjectState,
    projection: Option<&ActivityProjection>,
) -> Result<Value, Value> {
    let scope = match &state.scope {
        ProjectScope::Standalone => json!({"kind":"standalone"}),
        ProjectScope::Member(name) => json!({"kind":"member","name":name.as_str()}),
    };
    let (project, root, generation, label) = if let Some(inactive) = &state.inactive {
        (
            inactive.project(),
            NodeId::Object(inactive.summary().id.clone()),
            inactive.generation(),
            inactive.summary().name.clone(),
        )
    } else {
        let active = session
            .workspace
            .project(&state.scope)
            .map_err(|failure| errors::workspace(&failure))?;
        (
            active.project(),
            active.root().clone(),
            active.generation(),
            active.activity_summary().name,
        )
    };
    Ok(
        json!({"projectId":state.id,"scope":scope,"type":project.configuration().project_type().as_str(),
        "rootPath":dto::path(project.root()),"sourcePath":dto::path(project.source()),"root":dto::node_id(&root),
        "generation":generation.to_string(),"eventSequence":state.event.to_string(),"requiresRefresh":state.refresh,"requiresReopen":state.reopen,
        "displayName":label,"activity":details(projection, &state.scope)?}),
    )
}

fn failure(reason: &str) -> Value {
    domain("workspace_activity_failed", json!({"reason":reason}))
}
fn storage_error(error: &activity::Error) -> Value {
    failure(match error {
        activity::Error::Conflict => "conflict",
        activity::Error::Busy => "busy",
        activity::Error::Invalid => "invalid_state",
        activity::Error::ResourceLimit => "resource_limit",
        activity::Error::UnsafePath => "unsafe_path",
        activity::Error::Io(_) => "io",
    })
}
