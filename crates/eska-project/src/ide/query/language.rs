//! Pure syntax reads share bounded uploads without opening files or acquiring write ownership.

use super::{INLINE_BYTES, params::Key};
use crate::ide::{
    envelope::{domain, error},
    params,
    server::Server,
};
use eska_project_core::{
    formatting::protocol::source_sha256,
    project::query::{MAX_MULTILINE_CONTINUATIONS, QueryError, multiline_strings},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

pub(in crate::ide) fn capability() -> Value {
    json!({"schemaVersion":1,"offsetEncoding":"utf-8","writesOwner":false,
        "maxTextBytes":MAX_TEXT_BYTES,"maxInlineBytes":INLINE_BYTES,
        "maxContinuations":MAX_MULTILINE_CONTINUATIONS})
}

pub(in crate::ide) fn is_method(method: &str) -> bool {
    matches!(
        method,
        "language/multilineStrings"
            | "language/bufferBegin"
            | "language/bufferAppend"
            | "language/bufferClose"
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    version: String,
    text: Option<String>,
    buffer_id: Option<String>,
}

impl Server {
    pub(in crate::ide) fn language(
        &mut self,
        method: &str,
        args: &Value,
        cancelled: &AtomicBool,
        closed: &AtomicBool,
    ) -> Result<Value, Value> {
        let key = Key {
            session: "@language".into(),
            project: "@language".into(),
            generation: "1".into(),
        };
        match method {
            "language/bufferBegin" => {
                if args["bytes"]
                    .as_u64()
                    .is_some_and(|bytes| bytes > MAX_TEXT_BYTES as u64)
                {
                    return Err(failure(QueryError::TooLarge));
                }
                return self.queries.begin_buffer(&key, args);
            }
            "language/bufferAppend" => return self.queries.append_buffer(&key, args),
            "language/bufferClose" => return self.queries.close("query/bufferClose", &key, args),
            _ => {}
        }
        params::optional_fields(args, &["text", "bufferId"])?;
        let input: Input = params::decode(args)?;
        if params::counter(&input.version)? == 0 {
            return Err(error(-32602));
        }
        let text = self
            .queries
            .input_text(&key, input.text.as_deref(), input.buffer_id.as_deref())?
            .ok_or_else(|| error(-32602))?;
        if text.len() > MAX_TEXT_BYTES {
            return Err(failure(QueryError::TooLarge));
        }
        check_cancelled(cancelled, closed)?;
        let strings = multiline_strings(text).map_err(failure)?;
        check_cancelled(cancelled, closed)?;
        let strings: Vec<_> = strings
            .into_iter()
            .map(|string| {
                json!({
                    "range":{"start":string.source.start,"end":string.source.end},
                    "continuations":string.continuations,
                })
            })
            .collect();
        Ok(
            json!({"schemaVersion":1,"version":input.version,"sourceSha256":source_sha256(text),"strings":strings}),
        )
    }
}

fn check_cancelled(cancelled: &AtomicBool, closed: &AtomicBool) -> Result<(), Value> {
    if cancelled.load(Ordering::Acquire) || closed.load(Ordering::Acquire) {
        Err(error(-32800))
    } else {
        Ok(())
    }
}

fn failure(error: QueryError) -> Value {
    domain(
        "language_syntax_failed",
        json!({"reason":match error {
            QueryError::TooLarge=>"text_too_large",
            _=>"invalid_literal",
        }}),
    )
}

#[cfg(test)]
mod tests;
