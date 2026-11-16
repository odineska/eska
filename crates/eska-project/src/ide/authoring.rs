//! Structural metadata and form clients share plans, discovery and stable domain failures.

use eska_project_core::project::{
    metadata_edit::snapshot_bytes,
    metadata_form_edit::{FormEditError, FormEditPlan, FormEditResult, FormEditSchema},
    metadata_objects::{ObjectError, ObjectPlan, ObjectRequest, ObjectResult, ObjectSchema},
    metadata_workspace::PropertyEditError,
};
use serde_json::{Value, json};

use super::{
    dto::{self, Labels},
    editing,
    envelope::domain,
};

mod contracts;
mod discovery;
mod rpc;
pub use contracts::request_schemas;
pub use discovery::{ListOptions, list};
pub(super) use rpc::{apply, query};

pub fn object_schema(labels: &Labels, schema: &ObjectSchema) -> Value {
    let mut result = json!(schema);
    result["kindOptions"] = json!(
        schema
            .kinds
            .iter()
            .map(|kind| json!({
                "key":kind.as_str(), "value":kind,
                "caption":labels.caption(&format!("platform-kind-{}", kind.as_str())),
            }))
            .collect::<Vec<_>>()
    );
    result["moduleOptions"] = json!(
        schema
            .module_roles
            .iter()
            .map(|role| json!({
                "role":role, "caption":labels.caption(&format!("tree-module-{}",role.as_str())),
            }))
            .collect::<Vec<_>>()
    );
    result["requestSchemas"] = request_schemas();
    result["propertyCaptions"] = Value::Object(
        schema
            .required_properties
            .values()
            .chain(schema.reference_properties.values())
            .chain(schema.type_properties.values())
            .chain(schema.unique_properties.values())
            .flatten()
            .map(|key| {
                (
                    key.clone(),
                    labels.caption(&format!("platform-property-md-{key}")),
                )
            })
            .collect(),
    );
    result
}

pub fn object_schema_for(
    labels: &Labels,
    project: &mut eska_project_core::project::metadata_workspace::ProjectSession,
    parent: Option<&eska_project_core::project::metadata_model::ObjectId>,
    kind: Option<eska_project_core::project::metadata_model::MetadataKind>,
) -> Result<Value, Value> {
    use eska_project_core::project::metadata_objects::ObjectInitialChoices;
    let schema = project.object_schema(parent).map_err(object_failure)?;
    let mut result = object_schema(labels, &schema);
    if let Some(kind) = kind {
        let fields = project
            .object_initial_fields(parent, kind)
            .map_err(object_failure)?;
        result["selectedKind"] = json!(kind);
        result["initialFields"] = json!(fields.into_iter().map(|field| {
            let mut value = match field.choices {
                ObjectInitialChoices::Types(choices) => json!({"kind":"types","multiple":true,"options":choices.iter().map(|choice| editing::type_choice(labels,choice)).collect::<Vec<_>>()}),
                ObjectInitialChoices::References { multiple, choices } => json!({"kind":"references","multiple":multiple,"options":choices.iter().map(|choice| editing::reference_choice(labels,choice)).collect::<Vec<_>>()}),
                ObjectInitialChoices::Values { domain, values } => json!({"kind":"values","multiple":false,"options":values.into_iter().map(|value| json!({"caption":editing::paired(labels,|locale| locale.enum_caption(&domain,&value).unwrap_or_else(|| editing::reference_caption(locale,&value))),"value":value})).collect::<Vec<_>>()}),
            };
            value["property"] = json!(field.property);
            value
        }).collect::<Vec<_>>());
    }
    Ok(result)
}

pub fn object_plan(plan: &ObjectPlan) -> Value {
    json!({
        "schemaVersion":1,"valid":true,"changed":!plan.files.is_empty() || !plan.directories.is_empty(),
        "operation":object_operation(&plan.prepared.request),
        "objectId":plan.object_id,"prepared":plan.prepared,
        "inputs":plan.inputs.iter().map(|path| dto::path(path)).collect::<Vec<_>>(),
        "files":plan.files.iter().map(|file| json!({
            "path":dto::path(&file.path),
            "action":if file.before.is_none() {"create"} else if file.after.is_none() {"delete"} else {"update"},
            "before":file.before.as_ref().map(|bytes| json!({"bytes":bytes.len(),"snapshot":snapshot_bytes(bytes)})),
            "after":file.after.as_ref().map(|bytes| json!({"bytes":bytes.len(),"snapshot":snapshot_bytes(bytes)})),
        })).collect::<Vec<_>>(),
        "directories":plan.directories,
        "references":plan.references.iter().map(|reference| json!({
            "path":dto::path(&reference.path),"range":reference.range,"reason":reference.reason,"confirmed":reference.confirmed,
        })).collect::<Vec<_>>(),
    })
}

pub fn object_result(result: &ObjectResult) -> Value {
    json!({"schemaVersion":1,"changed":true,"objectId":result.object_id,"snapshot":result.snapshot})
}

pub const fn object_operation(request: &ObjectRequest) -> &'static str {
    match request {
        ObjectRequest::Create { .. } => "create",
        ObjectRequest::CreateModule { .. } => "create-module",
        ObjectRequest::Copy { .. } => "copy",
        ObjectRequest::Delete { .. } => "delete",
    }
}

pub fn object_failure(error: ObjectError) -> Value {
    match error {
        ObjectError::Workspace(error) => editing::failure(PropertyEditError::Workspace(error)),
        ObjectError::Edit(error) => editing::failure(PropertyEditError::Edit(error)),
        ObjectError::Name(reason) => domain("creation_invalid_name", json!({"reason":reason})),
        ObjectError::Collision(object) => domain("creation_collision", json!({"objectId":object})),
        ObjectError::Unsupported(kind) => {
            domain("object_unsupported", json!({"metadataKind":kind}))
        }
        ObjectError::Invalid { field, reason } => {
            domain("object_invalid", json!({"field":field,"reason":reason}))
        }
        ObjectError::ReviewRequired(references) => {
            domain("object_references", json!({"references":references}))
        }
        ObjectError::Committed(_) => domain("object_committed_refresh_required", json!({})),
        ObjectError::Io { path, source } => domain(
            "object_io",
            json!({"path":dto::path(&path),"reason":format!("{:?}",source.kind())}),
        ),
        ObjectError::Identity(_) => domain("creation_identity_unavailable", json!({})),
        ObjectError::PropertyConflict {
            property,
            value,
            object,
        } => domain(
            "object_property_conflict",
            json!({"property":property,"value":value,"objectId":object}),
        ),
    }
}

pub fn form_schema(labels: &Labels, schema: &FormEditSchema) -> Value {
    let mut result = json!(schema);
    let catalogs = schema
        .element_kinds
        .iter()
        .map(|kind| &kind.catalog)
        .chain(schema.definition_kinds.iter().map(|kind| &kind.catalog));
    let mut property_captions = serde_json::Map::new();
    let mut choice_captions = serde_json::Map::new();
    for field in catalogs.flatten() {
        property_captions.entry(field.id).or_insert_with(|| {
            let key = eska_project_core::project::metadata_model::PropertyKey {
                namespace: Some(eska_project_core::project::forms::FORM_NAMESPACE.to_owned()),
                name: field.id.to_owned(),
            };
            editing::paired(labels, |locale| {
                locale
                    .property_caption(None, &key)
                    .unwrap_or_else(|| field.id.to_owned())
            })
        });
        if !field.enum_captions.is_empty() {
            choice_captions.entry(field.id).or_insert_with(|| {
                Value::Object(
                    field
                        .enum_captions
                        .iter()
                        .map(|(value, key)| {
                            (
                                (*value).to_owned(),
                                editing::paired(labels, |locale| {
                                    locale
                                        .form_choice_caption(key)
                                        .unwrap_or_else(|| (*value).to_owned())
                                }),
                            )
                        })
                        .collect(),
                )
            });
        }
    }
    result["propertyCaptions"] = Value::Object(property_captions);
    result["choiceCaptions"] = Value::Object(choice_captions);
    result["schemaVersion"] = json!(1);
    result["path"] = dto::path(&schema.path);
    result["requestSchemas"] = request_schemas();
    result
}

pub fn form_plan(plan: &FormEditPlan) -> Value {
    json!({
        "schemaVersion":1,"operation":"form-edit","valid":true,"changed":!plan.is_empty(),
        "form":plan.prepared.request.form,"source":dto::path(&plan.path),"prepared":plan.prepared,
        "assignedIds":plan.assigned_ids,
        "inputs":plan.inputs.iter().map(|path| dto::path(path)).collect::<Vec<_>>(),
        "changes":plan.replacements().iter().map(|replacement| json!({
            "range":replacement.range,"replacement":replacement.text,
        })).collect::<Vec<_>>(),
    })
}

pub fn form_result(result: &FormEditResult) -> Value {
    json!({"schemaVersion":1,"changed":result.refresh.is_some(),"form":result.form,"snapshot":result.snapshot,"assignedIds":result.assigned_ids})
}

pub fn form_failure(error: FormEditError) -> Value {
    match error {
        FormEditError::Edit(error) => editing::failure(PropertyEditError::Edit(error)),
        FormEditError::Workspace(error) => editing::failure(PropertyEditError::Workspace(error)),
        FormEditError::Name(reason) => domain("creation_invalid_name", json!({"reason":reason})),
        FormEditError::Parse(_) => domain("form_invalid_xml", json!({})),
        FormEditError::Invalid { field, reason } => {
            domain("form_invalid", json!({"field":field,"reason":reason}))
        }
        FormEditError::MissingTarget(target) => {
            domain("form_target_missing", json!({"target":target}))
        }
        FormEditError::Collision(name) => domain("form_name_collision", json!({"name":name})),
        FormEditError::References { target, paths } => {
            domain("form_references", json!({"target":target,"paths":paths}))
        }
        FormEditError::Committed(_) => domain("form_committed_refresh_required", json!({})),
    }
}
