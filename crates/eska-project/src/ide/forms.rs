//! Read-only view sessions share source ownership and uploads with the existing IDE protocol.

mod dto;
mod navigation;
mod requests;

use super::{
    envelope::{domain, error},
    errors, params, query,
    server::Server,
};
use eska_project_core::{
    application::ContextError,
    project::{
        forms::{FormContextError, FormPreview},
        metadata_model::NodeId,
        metadata_workspace::FormPreviewError,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_SESSIONS: usize = 16;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;
const MAX_MODEL_BYTES: usize = 24 * 1024 * 1024;

#[derive(Default)]
pub(super) struct State {
    entries: BTreeMap<String, Entry>,
    serial: u64,
}
struct Entry {
    key: query::Key,
    node: NodeId,
    application: Option<String>,
    revision: u64,
    preview: FormPreview,
    model: Value,
    bytes: usize,
    last_error: Option<Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Open {
    node: Value,
    application: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Id {
    form_session_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Update {
    revision: String,
    overlays: Vec<Overlay>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Overlay {
    source: String,
    version: String,
    text: Option<String>,
    buffer_id: Option<String>,
}

pub(super) fn capability() -> Value {
    json!({"schemaVersion":1,"profile":"taxi-8.3.27","readOnly":true,"executesBsl":false,"maxSessions":MAX_SESSIONS,"maxRetainedBytes":MAX_RETAINED_BYTES,
        "maxFormBytes":eska_project_core::project::forms::MAX_FORM_BYTES,"maxOverlayBytes":8*1024*1024,"maxInlineBytes":128*1024,"uploads":"queryBuffers"})
}
pub(super) fn is_method(method: &str) -> bool {
    matches!(
        method,
        "form/open" | "form/get" | "form/update" | "form/close" | "form/locate"
    )
}
impl State {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
    pub(super) fn retain_projects(&mut self, projects: &[super::server::ProjectState]) {
        self.entries.retain(|_, entry| {
            projects
                .iter()
                .any(|project| project.id == entry.key.project && project.inactive.is_none())
        });
    }
    fn admit(&self, old: usize, bytes: usize) -> Result<(), Value> {
        let total: usize = self.entries.values().map(|entry| entry.bytes).sum();
        if total.saturating_sub(old).saturating_add(bytes) > MAX_RETAINED_BYTES {
            return Err(limit());
        }
        Ok(())
    }
}
impl Server {
    pub(super) fn forms(&mut self, method: &str, args: &Value) -> Result<Value, Value> {
        self.check_session(args)?;
        let key = query::Key::decode(args)?;
        if method == "form/close" {
            let input: Id = params::decode(args)?;
            if self
                .forms
                .entries
                .get(&input.form_session_id)
                .is_some_and(|entry| !entry.key.same_owner(&key))
            {
                return Err(failure("unknown_form_session"));
            }
            let released = self.forms.entries.remove(&input.form_session_id).is_some();
            return self.form_envelope(&key.project, json!({"released":released}));
        }
        let session = self.session.as_mut().ok_or_else(|| error(-32603))?;
        let state = session
            .projects
            .iter()
            .find(|project| project.id == key.project)
            .ok_or_else(|| domain("unknown_project", json!({"projectId":key.project})))?;
        if state.inactive.is_some() {
            return Err(domain("project_inactive", json!({"projectId":key.project})));
        }
        let project = session
            .workspace
            .project_mut(&state.scope)
            .map_err(|error| errors::workspace(&error))?;
        if project.generation().to_string() != key.generation {
            return Err(domain(
                "stale_generation",
                json!({"generation":project.generation().to_string()}),
            ));
        }
        let context = requests::Context {
            labels: &self.labels,
            project,
            activity: session
                .activity
                .as_ref()
                .map(|activity| &activity.snapshot.state),
        };
        let value = if method == "form/open" {
            self.forms.open(&context, &key, args)?
        } else {
            let input: Id = params::decode(args)?;
            let entry = self.forms.entry(&key, &input.form_session_id)?;
            match method {
                "form/get" => header(&input.form_session_id, entry),
                "form/update" => self.forms.update(
                    &context,
                    &self.queries,
                    &key,
                    &input.form_session_id,
                    args,
                )?,
                "form/locate" => navigation::locate(entry, args)?,
                _ => return Err(error(-32601)),
            }
        };
        self.form_envelope(&key.project, value)
    }
    fn form_envelope(&self, project: &str, mut value: Value) -> Result<Value, Value> {
        let session = self.session.as_ref().ok_or_else(|| error(-32603))?;
        let state = session
            .projects
            .iter()
            .find(|state| state.id == project)
            .ok_or_else(|| error(-32603))?;
        value["sessionId"] = json!(session.id);
        value["projectId"] = json!(project);
        let generation = if let Some(inactive) = state.inactive.as_ref() {
            inactive.generation()
        } else {
            session
                .workspace
                .project(&state.scope)
                .map_err(|error| errors::workspace(&error))?
                .generation()
        };
        value["generation"] = json!(generation.to_string());
        value["eventSequence"] = json!(state.event.to_string());
        Ok(value)
    }
}
fn header(id: &str, entry: &Entry) -> Value {
    json!({"formSessionId":id,"revision":entry.revision.to_string(),"contextFingerprint":entry.preview.context_fingerprint,"application":entry.preview.application,
        "limitedContext":entry.preview.limited_context,"status":if entry.last_error.is_some(){"stale"}else{"ready"},"lastError":entry.last_error,"modelComplete":entry.preview.form.diagnostics.is_empty() && entry.preview.presentation.diagnostics.is_empty(),
        "profile":"taxi-8.3.27","dependencies":entry.preview.dependencies.iter().map(dto::dependency).collect::<Vec<_>>(),"model":entry.model})
}
fn limit() -> Value {
    domain(
        "resource_limit",
        json!({"resource":"form_preview","maxSessions":MAX_SESSIONS,"maxRetainedBytes":MAX_RETAINED_BYTES}),
    )
}
fn failure(reason: &str) -> Value {
    domain("form_preview_failed", json!({"reason":reason}))
}
fn preview_error(error: FormPreviewError) -> Value {
    match error {
        FormPreviewError::Workspace(error) => errors::workspace(&error),
        FormPreviewError::Context(context) => match *context {
            ContextError::AmbiguousApplication { available }
            | ContextError::UnknownApplication { available, .. } => domain(
                "form_preview_failed",
                json!({"reason":"application_required","available":available.iter().map(eska_project_core::project::ProjectName::as_str).collect::<Vec<_>>()}),
            ),
            _ => failure("context_unavailable"),
        },
        FormPreviewError::Form(error) => match error {
            FormContextError::Parse { source, error } => domain(
                "form_preview_failed",
                json!({"reason":"invalid_xml","source":source,"parse":error}),
            ),
            FormContextError::InvalidMetadata { source } => domain(
                "form_preview_failed",
                json!({"reason":"invalid_metadata","source":source}),
            ),
            FormContextError::MemberMismatch => failure("context_member_mismatch"),
            FormContextError::InvalidOverlay => failure("foreign_form_input"),
            FormContextError::TooManyResources | FormContextError::InputTooLarge => limit(),
            FormContextError::Source(_) | FormContextError::MissingForm => {
                failure("source_unavailable")
            }
        },
        FormPreviewError::NotManagedForm => failure("not_managed_form"),
    }
}
