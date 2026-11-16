//! Human and machine clients share semantic creation plans and platform captions.

use super::{
    dto::{self, Labels},
    editing,
    envelope::{domain, error},
    metadata, params,
    server::{ProjectState, Server},
};
use eska_project_core::project::{
    metadata_create::{
        CreateRequest, CreationCapabilities, CreationError, CreationKind, CreationPlan,
        CreationResult, CreationSchema, PreparedCreate,
    },
    metadata_model::{MetadataKind, ObjectId, PropertyKey},
    metadata_workspace::{ProjectSession, PropertyEditError},
};
use serde_json::{Value, json};

/// Captions enrich the existing machine vocabulary without making it depend on a locale.
pub fn capabilities(labels: &Labels, value: &CreationCapabilities) -> Value {
    let mut result = json!(value);
    result["kindOptions"] = kind_options(labels, &value.kinds);
    result["ownerOptions"] = json!(
        value
            .owner_kinds
            .iter()
            .map(|kind| { json!({"value":kind,"caption":kind_caption(labels, *kind)}) })
            .collect::<Vec<_>>()
    );
    let properties = [
        ("name", "md", "Name"),
        ("synonym", "md", "Synonym"),
        ("comment", "md", "Comment"),
        ("type", "md", "Type"),
        ("length", "core", "Length"),
        ("fixed", "core", "AllowedLength"),
        ("digits", "core", "Digits"),
        ("fractionDigits", "core", "FractionDigits"),
        ("nonNegative", "core", "AllowedSign"),
        ("fractions", "core", "DateFractions"),
    ];
    result["fieldCaptions"] = Value::Object(
        properties
            .into_iter()
            .map(|(key, namespace, property)| {
                (
                    key.to_owned(),
                    labels.caption(&format!("platform-property-{namespace}-{property}")),
                )
            })
            .collect(),
    );
    result["dateFractions"] = json!(
        [("date", "Date"), ("time", "Time"), ("dateTime", "DateTime"),]
            .into_iter()
            .map(|(value, token)| json!({"value":value,
        "caption":enum_caption(labels,"DateFractions",token)}))
            .collect::<Vec<_>>()
    );
    result["allowedLength"] = json!(
        [(false, "Variable"), (true, "Fixed"),]
            .into_iter()
            .map(|(value, token)| json!({"value":value,
        "caption":enum_caption(labels,"AllowedLength",token)}))
            .collect::<Vec<_>>()
    );
    result["allowedSign"] = json!(
        [(false, "Any"), (true, "Nonnegative"),]
            .into_iter()
            .map(|(value, token)| json!({"value":value,
        "caption":enum_caption(labels,"AllowedSign",token)}))
            .collect::<Vec<_>>()
    );
    if let Some(types) = result["primitiveTypes"].as_array_mut() {
        for (choice, native) in types.iter_mut().zip(&value.primitive_types) {
            choice["caption"] = type_caption(labels, &native.key);
        }
    }
    result
}

/// Discovery carries the same defaults, limits and reference identities to every client.
pub fn schema(labels: &Labels, value: &CreationSchema) -> Value {
    let mut result = json!(value);
    result["schemaVersion"] = json!(1);
    result["capabilities"] = capabilities(labels, &value.capabilities);
    result["kindOptions"] = kind_options(labels, &value.kinds);
    if let Some(references) = result["references"].as_array_mut() {
        for (choice, native) in references.iter_mut().zip(&value.references) {
            choice["caption"] = type_caption(labels, &native.key);
        }
    }
    result
}

/// Preview exposes computed replacements; only its typed prepared input can be applied.
pub fn plan(value: &CreationPlan) -> Value {
    json!({
        "schemaVersion":1,
        "valid":true,
        "changed":true,
        "objectId":value.object_id,
        "source":dto::path(&value.path),
        "snapshot":value.prepared.request.snapshot,
        "prepared":value.prepared,
        "changes":value.replacements().iter().map(|replacement| json!({
            "range":{"start":replacement.range.start,"end":replacement.range.end},
            "replacement":replacement.text,
        })).collect::<Vec<_>>(),
    })
}

/// Acknowledgement identifies the committed object so callers can inspect after an interrupted reply.
pub fn result(value: &CreationResult) -> Value {
    json!({"schemaVersion":1,"changed":true,"objectId":value.object_id,
        "uuid":value.uuid,"snapshot":value.snapshot})
}

/// Existing protection/conflict errors retain their meaning across property and structural edits.
pub fn failure(value: CreationError) -> Value {
    match value {
        CreationError::Workspace(error) => editing::failure(PropertyEditError::Workspace(error)),
        CreationError::Committed(_) => domain("creation_committed_refresh_required", json!({})),
        CreationError::Edit(error) => editing::failure(PropertyEditError::Edit(error)),
        CreationError::Name(reason) => domain("creation_invalid_name", json!({"reason":reason})),
        CreationError::Collision(id) => domain("creation_collision", json!({"objectId":id})),
        CreationError::UnsupportedOwner(kind) => {
            domain("creation_unsupported_owner", json!({"metadataKind":kind}))
        }
        CreationError::Invalid { field, reason } => {
            domain("creation_invalid", json!({"field":field,"reason":reason}))
        }
        CreationError::Random(_) => domain("creation_identity_unavailable", json!({})),
    }
}

fn kind_options(labels: &Labels, kinds: &[CreationKind]) -> Value {
    json!(
        kinds
            .iter()
            .map(|kind| json!({
                "value":kind,"metadataKind":kind.metadata_kind(),
                "caption":kind_caption(labels,kind.metadata_kind()),
            }))
            .collect::<Vec<_>>()
    )
}

fn kind_caption(labels: &Labels, kind: MetadataKind) -> Value {
    labels.caption(&format!("platform-kind-{}", kind.as_str()))
}

fn type_caption(labels: &Labels, key: &PropertyKey) -> Value {
    let caption = |locale: &eska_presentation::localization::Localizer| {
        locale
            .type_caption(key)
            .or_else(|| super::property_presentation::qualified_type_caption(locale, key))
            .unwrap_or_else(|| key.name.clone())
    };
    json!({
        "ru-RU":caption(&labels.ru),
        "en-US":caption(&labels.en),
    })
}

fn enum_caption(labels: &Labels, domain: &str, token: &str) -> Value {
    json!({
        "ru-RU":labels.ru.enum_caption(domain,token).unwrap_or_else(||token.to_owned()),
        "en-US":labels.en.enum_caption(domain,token).unwrap_or_else(||token.to_owned()),
    })
}

/// Nested semantic inputs are strict while the outer envelope retains the normal IDE context.
pub(super) fn query(
    labels: &Labels,
    project: &mut ProjectSession,
    method: &str,
    args: &Value,
) -> Result<Value, Value> {
    match method {
        "metadata/creationSchema" => {
            let owner: ObjectId = params::decode(&args["owner"])?;
            project
                .creation_schema(&owner)
                .map(|value| schema(labels, &value))
                .map_err(failure)
        }
        "metadata/previewCreate" => {
            let request: CreateRequest = params::decode(&args["request"])?;
            project
                .preview_create(&request)
                .map(|value| plan(&value))
                .map_err(failure)
        }
        _ => Err(error(-32601)),
    }
}

/// Publish once and send invalidation before the reply, including a committed-refresh failure.
pub(super) fn apply(
    session: &str,
    state: &mut ProjectState,
    project: &mut ProjectSession,
    args: &Value,
    events: &mut Vec<Value>,
) -> Result<Value, Value> {
    if state.event == u64::MAX {
        return Err(domain("generation_exhausted", json!({})));
    }
    let prepared: PreparedCreate = params::decode(&args["prepared"])?;
    let generation = project.generation();
    let created = project.apply_create(&prepared);
    if project.generation() != generation {
        state.refresh = created.is_err();
        let affected = created
            .as_ref()
            .map_or(Value::Null, |value| json!(value.refresh.affected));
        metadata::changed(session, state, project, affected, events)?;
        Server::progress_event(session, state, project, events);
    }
    created.map(|value| result(&value)).map_err(failure)
}
