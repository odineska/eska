//! IDE mutations retain the same commit, read-only and refresh boundaries as property edits.

use eska_project_core::project::{
    metadata_form_edit::{FormEditRequest, PreparedFormEdit, inspect_draft},
    metadata_model::ObjectId,
    metadata_objects::{ObjectRequest, PreparedObject},
    metadata_workspace::ProjectSession,
};
use serde_json::{Value, json};

use crate::ide::{
    dto::Labels,
    envelope::{domain, error},
    metadata, params,
    server::{ProjectState, Server},
};

pub(in crate::ide) fn query(
    labels: &Labels,
    project: &mut ProjectSession,
    method: &str,
    args: &Value,
) -> Result<Value, Value> {
    match method {
        "metadata/objectSchema" => {
            let parent: Option<ObjectId> = args
                .get("parent")
                .map(params::decode)
                .transpose()?
                .flatten();
            let kind = args.get("kind").map(params::decode).transpose()?.flatten();
            super::object_schema_for(labels, project, parent.as_ref(), kind)
        }
        "metadata/previewObject" => {
            let request: ObjectRequest = params::decode(&args["request"])?;
            project
                .preview_object(&request)
                .map(|plan| super::object_plan(&plan))
                .map_err(super::object_failure)
        }
        "metadata/formEditSchema" => {
            let form: ObjectId = params::decode(&args["form"])?;
            let include_types = args
                .get("includeTypes")
                .map(params::decode::<bool>)
                .transpose()?
                .unwrap_or(false);
            project
                .form_edit_schema(&form)
                .map(|schema| {
                    let mut result = super::form_schema(labels, &schema);
                    if include_types {
                        result["typeOptions"] = super::form_type_options(labels, project);
                    }
                    result
                })
                .map_err(super::form_failure)
        }
        "metadata/previewFormEdit" => {
            let request: FormEditRequest = params::decode(&args["request"])?;
            let include_draft = args
                .get("includeDraft")
                .map(params::decode::<bool>)
                .transpose()?
                .unwrap_or(false);
            let plan = project
                .preview_form_edit(&request)
                .map_err(super::form_failure)?;
            let mut result = super::form_plan(&plan);
            if include_draft {
                result["draft"] =
                    json!(inspect_draft(plan.output(), true).map_err(super::form_failure)?);
            }
            Ok(result)
        }
        "metadata/formEditHistory" => {
            let form: ObjectId = params::decode(&args["form"])?;
            let expected: String = params::decode(&args["snapshot"])?;
            Ok(json!(project.form_edit_history(&form, &expected)))
        }
        _ => Err(error(-32601)),
    }
}

pub(in crate::ide) fn apply(
    session: &str,
    state: &mut ProjectState,
    project: &mut ProjectSession,
    method: &str,
    args: &Value,
    events: &mut Vec<Value>,
) -> Result<Value, Value> {
    if state.event == u64::MAX {
        return Err(domain("generation_exhausted", json!({})));
    }
    let generation = project.generation();
    let result = match method {
        "metadata/applyObject" => {
            let prepared: PreparedObject = params::decode(&args["prepared"])?;
            project
                .apply_object(&prepared)
                .map(|result| {
                    (
                        super::object_result(&result),
                        json!(result.refresh.affected),
                    )
                })
                .map_err(super::object_failure)
        }
        "metadata/applyFormEdit" | "metadata/replayFormEdit" => {
            let result = if method == "metadata/applyFormEdit" {
                let prepared: PreparedFormEdit = params::decode(&args["prepared"])?;
                let steps = args
                    .get("steps")
                    .map(params::decode::<Vec<usize>>)
                    .transpose()?
                    .unwrap_or_default();
                project.apply_form_edit_steps(&prepared, &steps)
            } else {
                let form: ObjectId = params::decode(&args["form"])?;
                let snapshot: String = params::decode(&args["snapshot"])?;
                let undo: bool = params::decode(&args["undo"])?;
                project.replay_form_edit(&form, &snapshot, undo)
            };
            result
                .map(|result| {
                    (
                        super::form_result(&result),
                        result
                            .refresh
                            .map_or(Value::Null, |refresh| json!(refresh.affected)),
                    )
                })
                .map_err(super::form_failure)
        }
        _ => Err(error(-32601)),
    };
    if project.generation() != generation {
        state.refresh = result.is_err();
        let affected = result
            .as_ref()
            .map_or(Value::Null, |(_, affected)| affected.clone());
        metadata::changed(session, state, project, affected, events)?;
        Server::progress_event(session, state, project, events);
    }
    result.map(|(value, _)| value)
}
