//! A successful complete parse replaces the view atomically; a failed refresh retains a marked stale model.

use super::{
    Entry, MAX_SESSIONS, Open, State, Update, dto, error, failure, header, limit, preview_error,
};
use crate::ide::{dto::Labels, params, query};
use eska_project_core::project::{forms::FormOverlay, metadata_workspace::ProjectSession};
use serde_json::Value;

pub(super) struct Context<'a> {
    pub labels: &'a Labels,
    pub project: &'a ProjectSession,
    pub pictures: bool,
    pub activity: Option<&'a eska_project_core::project::activity::State>,
}

impl State {
    pub(super) fn entry(&self, key: &query::Key, id: &str) -> Result<&Entry, Value> {
        self.entries
            .get(id)
            .filter(|entry| entry.key.same_owner(key))
            .ok_or_else(|| failure("unknown_form_session"))
    }
    pub(super) fn open(
        &mut self,
        context: &Context<'_>,
        key: &query::Key,
        args: &Value,
    ) -> Result<Value, Value> {
        params::optional_fields(args, &["application"])?;
        let input: Open = params::decode(args)?;
        if input.application.as_ref().is_some_and(String::is_empty) {
            return Err(error(-32602));
        }
        if self.entries.len() >= MAX_SESSIONS {
            return Err(limit());
        }
        let node = params::node(&input.node)?;
        let preview = context
            .project
            .form_preview(
                &node,
                input.application.as_deref(),
                &[],
                &[],
                context.activity,
            )
            .map_err(preview_error)?;
        let (model, bytes) = dto::model(context.labels, &preview)?;
        self.admit(0, bytes)?;
        self.serial = self.serial.checked_add(1).ok_or_else(limit)?;
        let id = format!("f{}", self.serial);
        let entry = Entry {
            key: key.clone(),
            node,
            application: input.application,
            pictures: context.pictures,
            revision: 1,
            preview,
            model,
            bytes,
            last_error: None,
        };
        let value = header(&id, &entry);
        self.entries.insert(id, entry);
        Ok(value)
    }
    pub(super) fn update(
        &mut self,
        context: &Context<'_>,
        queries: &query::State,
        key: &query::Key,
        id: &str,
        args: &Value,
    ) -> Result<Value, Value> {
        let entry = self.entry(key, id)?;
        let update: Update = params::decode(args)?;
        if params::counter(&update.revision)? != entry.revision {
            return Err(failure("stale_form_revision"));
        }
        if let Some(values) = args["overlays"].as_array() {
            for value in values {
                params::optional_fields(value, &["text", "bufferId"])?;
            }
        }
        let mut sources = std::collections::BTreeSet::new();
        let mut input_bytes = 0usize;
        let overlays = update
            .overlays
            .into_iter()
            .map(|input| {
                if !entry.preview.dependencies.iter().any(|dependency| {
                    dependency.id == input.source
                        && dependency.role
                            != eska_project_core::project::forms::FormDependencyRole::Picture
                }) || input.version.is_empty()
                    || !sources.insert(input.source.clone())
                {
                    return Err(failure("foreign_form_input"));
                }
                let text = queries
                    .input_text(key, input.text.as_deref(), input.buffer_id.as_deref())?
                    .ok_or_else(|| error(-32602))?;
                // Count even retired metadata, before copying shared upload buffers.
                input_bytes = input_bytes.saturating_add(text.len());
                if input_bytes > eska_project_core::project::forms::MAX_FORM_CONTEXT_BYTES {
                    return Err(limit());
                }
                Ok(FormOverlay {
                    source: input.source,
                    version: input.version,
                    text: text.into(),
                })
            })
            .collect::<Result<Vec<_>, Value>>()?;
        let refreshed = context
            .project
            .form_preview(
                &entry.node,
                entry.application.as_deref(),
                &overlays,
                &entry.preview.dependencies,
                context.activity,
            )
            .map_err(preview_error)
            .and_then(|preview| {
                let (model, bytes) = dto::model(context.labels, &preview)?;
                self.admit(entry.bytes, bytes)?;
                Ok((preview, model, bytes))
            });
        let (preview, model, bytes) = match refreshed {
            Ok(result) => result,
            Err(error) => {
                if let Some(entry) = self.entries.get_mut(id) {
                    entry.last_error = Some(error.clone());
                }
                return Err(error);
            }
        };
        let revision = entry.revision.checked_add(1).ok_or_else(limit)?;
        let entry = self.entries.get_mut(id).ok_or_else(|| error(-32603))?;
        entry.preview = preview;
        entry.model = model;
        entry.bytes = bytes;
        entry.revision = revision;
        entry.last_error = None;
        Ok(header(id, entry))
    }
}
