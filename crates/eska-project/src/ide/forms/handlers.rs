//! A handler click resolves its module before accepting the editor's current BSL snapshot.

use super::{Entry, error, failure, limit, preview_error, requests::Context};
use crate::ide::{dto, params, query};
use eska_project_core::project::{forms, metadata_edit::snapshot};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Target {
    revision: String,
    index: usize,
    input: Option<Input>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    source: String,
    version: String,
    text: Option<String>,
    buffer_id: Option<String>,
}

pub(super) fn navigate(
    context: &Context<'_>,
    entry: &Entry,
    queries: &query::State,
    locate: bool,
    args: &Value,
) -> Result<Value, Value> {
    if entry.last_error.is_some() {
        return Err(failure("stale_form_model"));
    }
    let input: Target = params::decode(args)?;
    if params::counter(&input.revision)? != entry.revision {
        return Err(failure("stale_form_revision"));
    }
    if locate != input.input.is_some() {
        return Err(error(-32602));
    }
    let module = context
        .project
        .form_handler_source(
            &entry.node,
            entry.application.as_deref(),
            &entry.preview,
            input.index,
            context.activity,
        )
        .map_err(preview_error)?
        .ok_or_else(|| failure("handler_unavailable"))?;
    let mut result = json!({"source":module.id,"root":dto::path(&module.root),"path":dto::path(&module.path),"revision":entry.revision.to_string()});
    if let Some(input) = input.input {
        if input.source != module.id || input.version.is_empty() {
            return Err(failure("foreign_handler_input"));
        }
        let text = queries
            .input_text(
                &entry.key,
                input.text.as_deref(),
                input.buffer_id.as_deref(),
            )?
            .ok_or_else(|| error(-32602))?;
        let range = forms::handler_range(text, &module.name)
            .map_err(|error| match error {
                forms::HandlerLookupError::TooLarge => limit(),
                forms::HandlerLookupError::InvalidModule => failure("handler_module_invalid"),
                forms::HandlerLookupError::Ambiguous => failure("handler_ambiguous"),
            })?
            .ok_or_else(|| failure("handler_not_found"))?;
        result["snapshot"] = json!(snapshot(text));
        result["range"] = json!(range);
        result["version"] = json!(input.version);
    }
    Ok(result)
}
