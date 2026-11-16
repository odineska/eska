//! Generation-checked metadata queries and explicit refresh commit boundaries.

use super::{
    dto,
    envelope::{domain, error},
    errors, params,
    server::{ProjectState, Server},
};
use eska_project_core::project::{
    configurator::TreeOptions,
    metadata_model::{NodeId, ObjectId},
    metadata_workspace::{
        ProjectSession,
        search::{IndexFailure, SearchOptions},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Source mutations share write permission and late-cancellation policy across the transport.
pub(super) fn writes(method: &str) -> bool {
    matches!(
        method,
        "metadata/updateProperty"
            | "metadata/undoProperty"
            | "metadata/renameApply"
            | "metadata/create"
            | "metadata/applyObject"
            | "metadata/applyFormEdit"
    )
}

/// Recognize request-only methods before validating metadata parameters.
pub(super) fn is_method(method: &str) -> bool {
    matches!(
        method,
        "metadata/support"
            | "metadata/propertyEditing"
            | "metadata/propertyTypeChoices"
            | "metadata/propertyReferenceChoices"
            | "metadata/propertyValueChoices"
            | "metadata/previewProperty"
            | "metadata/updateProperty"
            | "metadata/undoProperty"
            | "metadata/renamePreview"
            | "metadata/renameApply"
            | "metadata/creationSchema"
            | "metadata/previewCreate"
            | "metadata/create"
            | "metadata/objectSchema"
            | "metadata/previewObject"
            | "metadata/applyObject"
            | "metadata/formEditSchema"
            | "metadata/previewFormEdit"
            | "metadata/applyFormEdit"
            | "metadata/supportFiles"
            | "metadata/supportObjects"
            | "metadata/root"
            | "metadata/children"
            | "metadata/get"
            | "metadata/properties"
            | "metadata/source"
            | "metadata/search"
            | "metadata/reveal"
            | "metadata/revealSource"
            | "metadata/refresh"
            | "metadata/index"
            | "metadata/indexErrors"
    )
}

impl Server {
    /// Resolve context, enforce recovery state, then expose only current-generation data.
    pub(super) fn metadata(
        &mut self,
        method: &str,
        args: &Value,
        events: &mut Vec<Value>,
    ) -> Result<Value, Value> {
        let input: params::Context = params::decode(args)?;
        check_client_version(self.client_minor, method, args)?;

        if self
            .session
            .as_ref()
            .is_none_or(|session| session.id != input.session_id)
        {
            return Err(domain(
                "unknown_session",
                json!({"sessionId": input.session_id}),
            ));
        }
        params::counter(&input.generation)?;
        let session = self.session.as_mut().ok_or_else(|| error(-32603))?;
        let state = session
            .projects
            .iter_mut()
            .find(|state| state.id == input.project_id)
            .ok_or_else(|| domain("unknown_project", json!({"projectId":input.project_id})))?;
        if state.inactive.is_some() {
            return Err(domain("project_inactive", json!({"projectId":state.id})));
        }
        let project = session
            .workspace
            .project_mut(&state.scope)
            .map_err(|failure| errors::workspace(&failure))?;
        let result = metadata_request(
            &self.labels,
            method,
            args,
            &session.id,
            state,
            project,
            events,
        );
        result
            .map(|mut result| {
                constrain_schema(
                    &mut result,
                    method,
                    self.client_minor,
                    self.property_editing,
                );
                result["sessionId"] = json!(session.id);
                result["projectId"] = json!(state.id);
                result["generation"] = json!(project.generation().to_string());
                result["eventSequence"] = json!(state.event.to_string());
                result
            })
            .map_err(|mut failure| {
                if let Some(data) = failure.get_mut("data") {
                    data["sessionId"] = json!(session.id);
                    data["projectId"] = json!(state.id);
                    data["generation"] = json!(project.generation().to_string());
                }
                failure
            })
    }
}

fn check_client_version(minor: u32, method: &str, args: &Value) -> Result<(), Value> {
    let required = match method {
        "metadata/objectSchema"
        | "metadata/previewObject"
        | "metadata/applyObject"
        | "metadata/formEditSchema"
        | "metadata/previewFormEdit"
        | "metadata/applyFormEdit" => 49,
        "metadata/creationSchema" | "metadata/previewCreate" | "metadata/create" => 48,
        "metadata/revealSource" => 24,
        "metadata/supportObjects" => 25,
        "metadata/renamePreview" | "metadata/renameApply" => 9,
        "metadata/propertyReferenceChoices" => 7,
        "metadata/propertyValueChoices" => 8,
        _ => 0,
    };
    if minor < 50
        && (args["request"]["operation"] == "create-module"
            || args["prepared"]["request"]["operation"] == "create-module")
    {
        return Err(error(-32602));
    }
    if minor < required {
        return Err(error(-32601));
    }
    if minor < 52
        && matches!(
            method,
            "metadata/previewFormEdit" | "metadata/applyFormEdit"
        )
    {
        let operations = if method == "metadata/previewFormEdit" {
            &args["request"]["operations"]
        } else {
            &args["prepared"]["request"]["operations"]
        };
        if operations.as_array().is_some_and(|items| {
            items.iter().any(|item| match item["kind"].as_str() {
                Some("setListConditionalAppearance") => true,
                Some("setConditionalAppearance") => {
                    minor < 51
                        || item["rules"].as_array().is_some_and(|rules| {
                            rules
                                .iter()
                                .any(|rule| extended_conditions(&rule["conditions"]))
                        })
                }
                _ => false,
            })
        }) {
            return Err(error(-32602));
        }
    }
    if minor < 49
        && matches!(
            method,
            "metadata/previewProperty" | "metadata/updateProperty"
        )
        && matches!(
            args["change"]["kind"].as_str(),
            Some("references" | "types")
        )
    {
        return Err(error(-32602));
    }
    Ok(())
}

fn extended_conditions(conditions: &Value) -> bool {
    conditions.as_array().is_some_and(|items| {
        items.iter().any(|item| {
            matches!(
                item["operator"].as_str(),
                Some("InList" | "NotInList" | "InListByHierarchy" | "NotInListByHierarchy")
            ) || matches!(item["operand"]["kind"].as_str(), Some("date" | "values"))
                || extended_conditions(&item["items"])
        })
    })
}

fn constrain_form_operations(result: &mut Value, minor: u32) {
    let Some(definitions) = result
        .get_mut("requestSchemas")
        .and_then(|value| value["form-edit"]["$defs"].as_object_mut())
    else {
        return;
    };
    if let Value::Object(legacy) = super::authoring::form_definitions(false) {
        *definitions = legacy;
    }
    if minor >= 51 {
        return;
    }
    if let Some(items) = definitions
        .get_mut("formOperation")
        .and_then(|value| value["oneOf"].as_array_mut())
    {
        items.retain(|item| item["properties"]["kind"]["const"] != "setConditionalAppearance");
    }
    for key in ["appearanceRule", "appearanceCondition", "appearanceValue"] {
        definitions.remove(key);
    }
}

fn constrain_form_appearance(result: &mut Value, minor: u32) {
    let Some(result) = result.as_object_mut() else {
        return;
    };
    result.remove("dynamicListAppearance");
    if minor < 51 {
        for key in [
            "conditionalAppearance",
            "propertyCaptions",
            "choiceCaptions",
        ] {
            result.remove(key);
        }
    } else if let Some(Value::Object(appearance)) = result.get_mut("conditionalAppearance") {
        if let Some(Value::Array(comparisons)) = appearance.get_mut("comparisons") {
            comparisons.retain(|item| item["usesList"] != true);
            for item in comparisons.iter_mut().filter_map(Value::as_object_mut) {
                item.remove("usesList");
            }
        }
        if let Some(Value::Array(rules)) = appearance.get_mut("rules") {
            for rule in rules {
                if extended_conditions(&rule["conditions"]) {
                    rule["conditions"] = Value::Null;
                }
            }
        }
    }
}

fn constrain_schema(result: &mut Value, method: &str, minor: u32, writable: bool) {
    if minor < 52 {
        if method == "metadata/formEditSchema" {
            constrain_form_appearance(result, minor);
        }
        if matches!(method, "metadata/formEditSchema" | "metadata/objectSchema") {
            constrain_form_operations(result, minor);
        }
    }
    if method == "metadata/objectSchema"
        && (minor < 50 || !writable)
        && let Some(result) = result.as_object_mut()
    {
        result.remove("moduleOptions");
        result.remove("moduleRoles");
        if let Some(schemas) = result
            .get_mut("requestSchemas")
            .and_then(Value::as_object_mut)
        {
            schemas.remove("create-module");
        }
    }
    if !writable {
        match method {
            "metadata/creationSchema" | "metadata/formEditSchema" => {
                result["writable"] = json!(false);
            }
            "metadata/objectSchema" => result["operations"] = json!([]),
            _ => (),
        }
    }
    let schema = if method == "metadata/propertyEditing" {
        Some(result)
    } else {
        result.get_mut("editing")
    };
    if let Some(schema) = schema {
        if minor < 9
            && let Some(schema) = schema.as_object_mut()
        {
            for key in ["renameAvailable", "undoRename", "redoRename"] {
                schema.remove(key);
            }
        }
        if minor < 49 {
            super::editing::retain_legacy_fields(schema, minor);
        }
    }
}

/// Recovery refresh skips stale-generation checks only for the exact root node.
fn metadata_request(
    labels: &dto::Labels,
    method: &str,
    args: &Value,
    session: &str,
    state: &mut ProjectState,
    project: &mut ProjectSession,
    events: &mut Vec<Value>,
) -> Result<Value, Value> {
    check_metadata_request(project, state, method, args)?;
    match method {
        "metadata/propertyEditing"
        | "metadata/propertyTypeChoices"
        | "metadata/propertyReferenceChoices"
        | "metadata/propertyValueChoices"
        | "metadata/previewProperty" => super::editing::query(labels, project, method, args),
        "metadata/updateProperty" | "metadata/undoProperty" => {
            update_property(labels, session, state, project, method, args, events)
        }
        "metadata/renamePreview" => super::renaming::preview(project, args),
        "metadata/renameApply" => {
            super::renaming::apply(labels, session, state, project, args, events)
        }
        "metadata/creationSchema" | "metadata/previewCreate" => {
            super::creation::query(labels, project, method, args)
        }
        "metadata/create" => super::creation::apply(session, state, project, args, events),
        "metadata/objectSchema"
        | "metadata/previewObject"
        | "metadata/formEditSchema"
        | "metadata/previewFormEdit" => super::authoring::query(labels, project, method, args),
        "metadata/applyObject" | "metadata/applyFormEdit" => {
            super::authoring::apply(session, state, project, method, args, events)
        }
        _ => metadata_read(labels, session, state, project, method, args, events),
    }
}

/// Every metadata action observes the same generation and recovery boundary before reading inputs.
fn check_metadata_request(
    project: &ProjectSession,
    state: &ProjectState,
    method: &str,
    args: &Value,
) -> Result<(), Value> {
    let generation = params::counter(args["generation"].as_str().ok_or_else(|| error(-32602))?)?;
    if state.reopen {
        return Err(domain(
            "reopen_required",
            json!({"reason":"manifest_changed"}),
        ));
    }
    let root_refresh =
        method == "metadata/refresh" && params::node(&args["node"])? == *project.root();
    if state.refresh && !root_refresh && !(method == "metadata/index" && args["action"] == "status")
    {
        return Err(domain("resync_required", json!({"reason":"file_events"})));
    }
    if !(state.refresh && root_refresh) {
        project
            .check_generation(generation)
            .map_err(|failure| errors::workspace(&failure))?;
    }
    Ok(())
}

fn metadata_read(
    labels: &dto::Labels,
    session: &str,
    state: &mut ProjectState,
    project: &mut ProjectSession,
    method: &str,
    args: &Value,
    events: &mut Vec<Value>,
) -> Result<Value, Value> {
    match method {
        "metadata/supportFiles" => support_files(project, args),
        "metadata/supportObjects" => support_objects(project, args),
        "metadata/support" => {
            let offset = match args.get("offset") {
                None => 0,
                Some(value) => value
                    .as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or_else(|| error(-32602))?,
            };
            let snapshot = project.support_page(offset).ok_or_else(|| error(-32602))?;
            Ok(
                json!({"objects":snapshot.objects,"files":snapshot.files.iter().map(|file| json!({"path":dto::path(&file.path),"objects":file.objects,"readOnly":file.read_only,"reason":file.reason,"mixed":file.mixed,"unknown":file.unknown})).collect::<Vec<_>>(),"diagnostics":snapshot.diagnostics,"suppliers":snapshot.suppliers.iter().map(|supplier|json!({"id":supplier.id,"configurationUuid":supplier.configuration_uuid,"name":supplier.name,"vendor":supplier.vendor,"version":supplier.version,"locked":supplier.locked})).collect::<Vec<_>>(),"nextOffset":snapshot.next_offset,"freshness":"current"}),
            )
        }
        "metadata/root" => Ok(
            json!({"node":labels.node(project.node(project.root()).map_err(|failure| errors::workspace(&failure))?)}),
        ),
        "metadata/children" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Options {
                hide_empty_root_sections: Option<bool>,
            }
            let options: Options = params::decode(args)?;
            let id = params::node(&args["node"])?;
            let nodes = project
                .children(
                    &id,
                    TreeOptions {
                        hide_empty_root_sections: options.hide_empty_root_sections.unwrap_or(true),
                    },
                )
                .map_err(|failure| errors::workspace(&failure))?;
            Ok(json!({"nodes":nodes.into_iter().map(|node|labels.node(node)).collect::<Vec<_>>()}))
        }
        "metadata/get" => {
            let id: ObjectId = params::decode(&args["objectId"])?;
            Ok(
                json!({"object":dto::object(project.object(&id).map_err(|failure|errors::workspace(&failure))?)}),
            )
        }
        "metadata/properties" => properties(labels, project, args),
        "metadata/source" => {
            let id = params::node(&args["node"])?;
            let sources = project
                .source(&id)
                .map_err(|failure| errors::workspace(&failure))?;
            if sources.is_empty() {
                return Err(domain(
                    "source_missing",
                    json!({"node":dto::node_id(&id),"reason":"not_file"}),
                ));
            }
            Ok(json!({"sources":sources.iter().map(dto::source).collect::<Vec<_>>()}))
        }
        "metadata/search" => search(project, args),
        "metadata/revealSource" => reveal_source(labels, project, args),
        "metadata/reveal" => {
            let id: ObjectId = params::decode(&args["objectId"])?;
            let ancestry = reveal_ancestry(project, &id)?;
            Ok(json!({"ancestry":ancestry.iter().map(dto::node_id).collect::<Vec<_>>()}))
        }
        "metadata/refresh" => refresh(session, state, project, args, events),
        "metadata/index" => index(session, state, project, args, events),
        "metadata/indexErrors" => index_errors(project, args),
        _ => Err(error(-32601)),
    }
}

/// Return only the selected source's ancestry, avoiding transport and client retention of every sibling.
fn reveal_source(
    labels: &dto::Labels,
    project: &mut ProjectSession,
    args: &Value,
) -> Result<Value, Value> {
    let path: params::Path = params::decode(&args["path"])?;
    let Some(navigation) = project
        .reveal_source(&path.native()?)
        .map_err(|failure| errors::workspace(&failure))?
    else {
        return Ok(json!({"source":null,"nodes":[]}));
    };
    let nodes = navigation
        .ancestry
        .iter()
        .map(|id| {
            project
                .node(id)
                .map(|node| labels.node(node))
                .map_err(|failure| errors::workspace(&failure))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"source":dto::source(&navigation.source),"nodes":nodes}))
}

/// Reuse visible ancestry before asking the lazy index to reveal an unloaded object.
fn reveal_ancestry(project: &mut ProjectSession, id: &ObjectId) -> Result<Vec<NodeId>, Value> {
    if let Ok(ancestry) = project.ancestry(&NodeId::Object(id.clone())) {
        return Ok(ancestry);
    }
    project
        .reveal_indexed_object(id)
        .map_err(|failure| errors::workspace(&failure))
}

/// Mutations publish an ordered invalidation once; clients must never replay them after a lost reply.
fn update_property(
    labels: &dto::Labels,
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
    let id: ObjectId = params::decode(&args["objectId"])?;
    let generation = project.generation();
    let result = if method == "metadata/updateProperty" {
        let request: super::editing::ChangeRequest = params::decode(args)?;
        project
            .update_property(
                &id,
                &request.snapshot,
                request.context_snapshot.as_deref(),
                &request.path,
                &request.change,
            )
            .map(
                |refresh| eska_project_core::project::metadata_workspace::PropertyReplay {
                    object_id: id.clone(),
                    refresh,
                },
            )
    } else {
        let undo = match args["direction"].as_str() {
            Some("undo") => true,
            Some("redo") => false,
            _ => return Err(error(-32602)),
        };
        project.undo_property(
            &id,
            args["snapshot"].as_str().ok_or_else(|| error(-32602))?,
            args.get("contextSnapshot")
                .map(|value| value.as_str().ok_or_else(|| error(-32602)))
                .transpose()?,
            undo,
        )
    };
    if project.generation() != generation {
        state.refresh = result.is_err();
        let affected = result
            .as_ref()
            .map_or(Value::Null, |report| json!(report.refresh.affected));
        changed(session, state, project, affected, events)?;
        if let Ok(replay) = &result {
            super::renaming::annotate_event(events, &id, &replay.object_id);
        }
        Server::progress_event(session, state, project, events);
    }
    let replay = result.map_err(super::editing::failure)?;
    let mut result = edited_properties(labels, project, &replay.object_id)?;
    if id != replay.object_id {
        result["renamed"] = super::renaming::transition(&id, &replay.object_id);
    }
    Ok(result)
}

/// A committed mutation cannot be reported as a retryable failure when rereading the sheet fails.
pub(super) fn edited_properties(
    labels: &dto::Labels,
    project: &mut ProjectSession,
    id: &ObjectId,
) -> Result<Value, Value> {
    let committed = |failure: Value| {
        domain(
            "property_committed_refresh_required",
            json!({"cause":failure["data"]}),
        )
    };
    let mut result = properties(labels, project, &json!({"objectId":id})).map_err(committed)?;
    result["editing"] = super::editing::describe(labels, project, id).map_err(committed)?;
    Ok(result)
}

/// Preview data is optional and never turns a readable property sheet into an error.
fn properties(
    labels: &dto::Labels,
    project: &mut ProjectSession,
    args: &Value,
) -> Result<Value, Value> {
    let id: ObjectId = params::decode(&args["objectId"])?;
    let owner = project
        .object(&id)
        .map_err(|failure| errors::workspace(&failure))?
        .kind;
    let properties = project
        .properties(&id)
        .map_err(|failure| errors::workspace(&failure))?;
    let mut presentation = super::property_presentation::Presenter::new(labels, project, owner);
    let values: Vec<_> = properties
        .iter()
        .map(|property| {
            let mut value = dto::property(labels, owner, property);
            presentation.annotate(&property.property, &mut value);
            value
        })
        .collect();
    let mut result = json!({"properties":values});
    if let Some(picture) = project
        .picture_preview(&id)
        .map_err(|failure| errors::workspace(&failure))?
    {
        result["picture"] = dto::picture(&picture);
    }
    Ok(result)
}

/// Literal search never schedules indexing implicitly.
fn search(project: &ProjectSession, args: &Value) -> Result<Value, Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Search {
        text: String,
        limit: Option<usize>,
        synonym_language: Option<String>,
    }
    let input: Search = params::decode(args)?;
    let limit = limit(input.limit)?;
    let result = project.search(
        &input.text,
        &SearchOptions {
            limit,
            synonym_language: input.synonym_language,
        },
    );
    Ok(
        json!({"progress":dto::progress(&result.progress),"hits":result.hits.iter().map(dto::hit).collect::<Vec<_>>(),"truncated":result.truncated}),
    )
}

/// Refresh publishes changed state even when rereading the root fails.
fn refresh(
    session: &str,
    state: &mut ProjectState,
    project: &mut ProjectSession,
    args: &Value,
    events: &mut Vec<Value>,
) -> Result<Value, Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Options {
        reset_file_sequence: Option<bool>,
    }
    let options: Options = params::decode(args)?;
    let node = params::node(&args["node"])?;
    let root = &node == project.root();
    if options.reset_file_sequence.unwrap_or(false) && !root {
        return Err(error(-32602));
    }
    // Validate identity before claiming that a source mutation has occurred.
    project
        .node(&node)
        .map_err(|failure| errors::workspace(&failure))?;
    let result = project
        .refresh(&node)
        .map_err(|failure| errors::workspace(&failure));
    if root {
        state.refresh = result.is_err();
    }
    if result.is_ok() && options.reset_file_sequence.unwrap_or(false) {
        state.file = 0;
    }
    let affected = result
        .as_ref()
        .map_or(Value::Null, |report| json!(report.affected));
    changed(session, state, project, affected, events)?;
    Server::progress_event(session, state, project, events);
    result.map(|report| json!({"affected":report.affected}))
}

/// Start and resume return immediately; the worker schedules bounded descriptor steps later.
fn index(
    session: &str,
    state: &mut ProjectState,
    project: &mut ProjectSession,
    args: &Value,
    events: &mut Vec<Value>,
) -> Result<Value, Value> {
    match args["action"].as_str() {
        Some("start") => project.start_search_index(),
        Some("cancel") => project.cancel_search_index(),
        Some("resume") => project.resume_search_index(),
        Some("status") => (),
        _ => return Err(error(-32602)),
    }
    Server::progress_event(session, state, project, events);
    Ok(json!({"progress":dto::progress(&project.search_progress())}))
}

/// Paginate the current stable `ObjectId` ordering without retaining stale page snapshots.
fn index_errors(project: &ProjectSession, args: &Value) -> Result<Value, Value> {
    #[derive(Deserialize)]
    struct Options {
        offset: Option<usize>,
        limit: Option<usize>,
    }
    let input: Options = params::decode(args)?;
    let count = limit(input.limit)?;
    let offset = input.offset.unwrap_or(0);
    let mut entries = project.search_failures().skip(offset);
    let errors: Vec<_> = entries.by_ref().take(count).map(|(id, failure)| {
        let (code, details) = match failure {
            IndexFailure::Read(value) => { let failure = errors::workspace(value); (failure["data"]["kind"].clone(), failure["data"]["details"].clone()) }
            IndexFailure::Unsupported(diagnostics) => (json!("unsupported_metadata"), json!({"diagnostics":diagnostics.iter().map(dto::diagnostic).collect::<Vec<_>>()})),
        };
        json!({"objectId":id.as_str(),"code":code,"details":details})
    }).collect();
    let next = entries
        .next()
        .and_then(|_| offset.checked_add(errors.len()));
    Ok(json!({"errors":errors,"nextOffset":next}))
}

/// Reject out-of-contract limits rather than silently clamping client input.
fn limit(value: Option<usize>) -> Result<usize, Value> {
    let value = value.unwrap_or(50);
    if (1..=500).contains(&value) {
        Ok(value)
    } else {
        Err(error(-32602))
    }
}

/// Each invalidation has its own counter, including failed changes with unchanged generation.
pub(super) fn changed(
    session: &str,
    state: &mut ProjectState,
    project: &ProjectSession,
    affected: Value,
    events: &mut Vec<Value>,
) -> Result<(), Value> {
    state.event = state
        .event
        .checked_add(1)
        .ok_or_else(|| domain("generation_exhausted", json!({})))?;
    let mut notification = json!({"jsonrpc":"2.0","method":"metadata/changed","params":{"sessionId":session,"projectId":state.id,"generation":project.generation().to_string(),"eventSequence":state.event.to_string(),"affected":null,"requiresRefresh":state.refresh,"requiresReopen":state.reopen}});
    notification["params"]["affected"] = affected;
    notification["params"]["supportUnchanged"] =
        json!(!state.refresh && !state.reopen && project.support_is_current());
    if serde_json::to_vec(&notification)
        .map_or(true, |body| body.len() > super::framing::MAX_RESPONSE)
    {
        notification["params"]["affected"] = Value::Null;
    }
    events.push(notification);
    Ok(())
}

/// Return display policy in bounded batches without resolving source paths in every IDE adapter.
fn support_objects(project: &mut ProjectSession, args: &Value) -> Result<Value, Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Objects {
        object_ids: Vec<ObjectId>,
    }
    let input: Objects = params::decode(args)?;
    if input.object_ids.is_empty() || input.object_ids.len() > 128 {
        return Err(error(-32602));
    }
    let snapshot = project.support_objects(&input.object_ids);
    Ok(json!({"objects":snapshot.objects,"diagnostics":snapshot.diagnostics,"freshness":"current"}))
}

/// Classify a bounded set of source-relative paths without starting a support inventory.
fn support_files(project: &mut ProjectSession, args: &Value) -> Result<Value, Value> {
    #[derive(Deserialize)]
    struct Files {
        paths: Vec<params::Path>,
    }
    let input: Files = params::decode(args)?;
    if input.paths.is_empty() || input.paths.len() > 128 {
        return Err(error(-32602));
    }
    let paths = input
        .paths
        .iter()
        .map(params::Path::native)
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot = project
        .support_files(&paths)
        .map_err(|failure| errors::workspace(&failure))?;
    Ok(
        json!({"objects":snapshot.objects,"files":snapshot.files.iter().map(|file| json!({"path":dto::path(&file.path),"objects":file.objects,"readOnly":file.read_only,"reason":file.reason,"mixed":file.mixed,"unknown":file.unknown})).collect::<Vec<_>>(),"diagnostics":snapshot.diagnostics,"freshness":"current"}),
    )
}
