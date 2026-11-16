//! Selection, confirmation and output belong to the CLI; ownership and validation do not.

use eska_presentation::{
    arguments::project::metadata::{AuthoringInput, MetadataCommand, OutputFormat},
    localization::Localizer,
};
use eska_project_core::project::{
    metadata_objects::{ObjectRequest, PreparedObject},
    metadata_workspace::ProjectSession,
};
use serde_json::Value;

use super::super::creation::prompt;
use super::{input, presentation};
use crate::ide::authoring as shared;

pub(super) enum Loaded {
    Request(ObjectRequest),
    Plan(PreparedObject),
}

pub(super) fn load(command: &MetadataCommand) -> Result<Option<Loaded>, Value> {
    if !matches!(
        command,
        MetadataCommand::Create(_) | MetadataCommand::Copy(_) | MetadataCommand::Delete(_)
    ) {
        return Ok(None);
    }
    let (operation, args, _) = options(command)?;
    let loaded = if let Some(path) = &args.plan {
        Some(Loaded::Plan(input::plan(path, operation)?))
    } else if let Some(path) = &args.input {
        Some(Loaded::Request(input::request(path)?))
    } else {
        None
    };
    if loaded
        .as_ref()
        .is_some_and(|value| shared::object_operation(value.request()) != operation)
    {
        return Err(input::invalid("request", "operation_mismatch"));
    }
    Ok(loaded)
}

impl Loaded {
    const fn request(&self) -> &ObjectRequest {
        match self {
            Self::Request(request) => request,
            Self::Plan(plan) => &plan.request,
        }
    }
}

pub(super) fn configuration(command: &MetadataCommand, loaded: Option<&Loaded>) -> bool {
    loaded.map_or_else(
        || matches!(command, MetadataCommand::Create(args) if args.kind.as_deref() == Some("configuration")),
        |loaded| matches!(loaded.request(), ObjectRequest::Create { kind:eska_project_core::project::metadata_model::MetadataKind::Configuration, .. }),
    )
}

pub(super) fn run(
    project: &mut ProjectSession,
    command: &MetadataCommand,
    locale: &Localizer,
    format: OutputFormat,
    loaded: Option<Loaded>,
) -> Result<Value, Value> {
    let (_, args, _) = options(command)?;
    let interactive = super::interactive(args, format);
    let plan = match loaded {
        // Applying a saved plan already validates its inputs and output under the core write guard.
        // A second CLI preview would repeat the complete reference scan of a large configuration.
        Some(Loaded::Plan(prepared)) if !args.dry_run => {
            return project
                .apply_object(&prepared)
                .map(|result| shared::object_result(&result))
                .map_err(shared::object_failure);
        }
        Some(Loaded::Plan(prepared)) => project.preview_prepared_object(&prepared),
        Some(Loaded::Request(request)) => project.preview_object(&request),
        None => {
            let mut request = request(project, command, locale, interactive)?;
            if interactive {
                super::prompt::required_properties(project, &mut request, locale)?;
            }
            project.preview_object(&request)
        }
    }
    .map_err(shared::object_failure)?;
    finish(&plan, command, locale, format, |prepared| {
        project.apply_object(prepared)
    })
}

pub(super) fn run_configuration(
    project: &eska_project_core::project::Project,
    command: &MetadataCommand,
    locale: &Localizer,
    format: OutputFormat,
    loaded: Option<Loaded>,
) -> Result<Value, Value> {
    use eska_project_core::project::{metadata_model::MetadataKind, metadata_objects::bootstrap};
    let MetadataCommand::Create(args) = command else {
        return Err(super::failure("invalid_request"));
    };
    let plan = match loaded {
        Some(Loaded::Plan(prepared)) if !args.source.dry_run => {
            return bootstrap::apply(project, &prepared)
                .map(|result| shared::object_result(&result))
                .map_err(shared::object_failure);
        }
        Some(Loaded::Plan(prepared)) => bootstrap::preview_prepared(project, &prepared),
        Some(Loaded::Request(request)) => bootstrap::preview(project, &request),
        None => bootstrap::preview(
            project,
            &ObjectRequest::Create {
                kind: MetadataKind::Configuration,
                name: name(
                    args.name.as_deref(),
                    locale,
                    super::interactive(&args.source, format),
                )?,
                parent: args.parent.as_deref().map(input::object_id).transpose()?,
                template: args.template.as_deref().map(input::object_id).transpose()?,
                property_values: input::properties(&args.property_values)?,
                property_references: input::references(&args.property_references)?,
                property_types: input::types(&args.property_types)?,
            },
        ),
    }
    .map_err(shared::object_failure)?;
    finish(&plan, command, locale, format, |prepared| {
        bootstrap::apply(project, prepared)
    })
}

fn finish(
    plan: &eska_project_core::project::metadata_objects::ObjectPlan,
    command: &MetadataCommand,
    locale: &Localizer,
    format: OutputFormat,
    apply: impl FnOnce(
        &PreparedObject,
    ) -> Result<
        eska_project_core::project::metadata_objects::ObjectResult,
        eska_project_core::project::metadata_objects::ObjectError,
    >,
) -> Result<Value, Value> {
    let (operation, args, yes) = options(command)?;
    let interactive = super::interactive(args, format);
    if args.dry_run {
        return Ok(shared::object_plan(plan));
    }
    if interactive {
        presentation::review(&shared::object_plan(plan), locale);
        if !prompt::confirm(locale)? {
            return Err(super::failure("creation_cancelled"));
        }
    } else if operation == "delete" && !yes && args.plan.is_none() {
        return Err(
            serde_json::json!({"kind":"object_confirmation_required","details":{"preview":shared::object_plan(plan)}}),
        );
    }
    apply(&plan.prepared)
        .map(|result| shared::object_result(&result))
        .map_err(shared::object_failure)
}

fn options(command: &MetadataCommand) -> Result<(&'static str, &AuthoringInput, bool), Value> {
    match command {
        MetadataCommand::Create(args) => Ok(("create", &args.source, false)),
        MetadataCommand::Copy(args) => Ok(("copy", &args.input, false)),
        MetadataCommand::Delete(args) => Ok(("delete", &args.source, args.yes)),
        _ => Err(super::failure("invalid_request")),
    }
}

fn request(
    project: &mut ProjectSession,
    command: &MetadataCommand,
    locale: &Localizer,
    interactive: bool,
) -> Result<ObjectRequest, Value> {
    match command {
        MetadataCommand::Create(args) => {
            let parent = args.parent.as_deref().map(input::object_id).transpose()?;
            let kind = if let Some(kind) = &args.kind {
                input::metadata_kind(kind)?
            } else if interactive {
                let schema = project
                    .object_schema(parent.as_ref())
                    .map_err(shared::object_failure)?;
                let choices: Vec<_> = schema
                    .kinds
                    .iter()
                    .map(|kind| {
                        (
                            kind.as_str().to_owned(),
                            locale.text(&format!("platform-kind-{}", kind.as_str())),
                        )
                    })
                    .collect();
                input::metadata_kind(&prompt::choose(
                    locale,
                    "metadata-create-kind-prompt",
                    &choices,
                    None,
                )?)?
            } else {
                return Err(input::required(&["kind"]));
            };
            let name = name(args.name.as_deref(), locale, interactive)?;
            Ok(ObjectRequest::Create {
                kind,
                name,
                parent,
                template: args.template.as_deref().map(input::object_id).transpose()?,
                property_values: input::properties(&args.property_values)?,
                property_references: input::references(&args.property_references)?,
                property_types: input::types(&args.property_types)?,
            })
        }
        MetadataCommand::Copy(args) => Ok(ObjectRequest::Copy {
            source: selector(args.source.as_deref(), "source", locale, interactive)?,
            name: name(args.name.as_deref(), locale, interactive)?,
            parent: args.parent.as_deref().map(input::object_id).transpose()?,
            property_values: input::properties(&args.property_values)?,
            property_references: input::references(&args.property_references)?,
            property_types: input::types(&args.property_types)?,
        }),
        MetadataCommand::Delete(args) => Ok(ObjectRequest::Delete {
            object: selector(args.object.as_deref(), "object", locale, interactive)?,
            force: args.force,
        }),
        _ => Err(super::failure("invalid_request")),
    }
}

fn name(value: Option<&str>, locale: &Localizer, interactive: bool) -> Result<String, Value> {
    match value {
        Some(value) => Ok(value.to_owned()),
        None if interactive => prompt::required_text(locale, "metadata-create-name-prompt"),
        None => Err(input::required(&["name"])),
    }
}

fn selector(
    value: Option<&str>,
    field: &str,
    locale: &Localizer,
    interactive: bool,
) -> Result<eska_project_core::project::metadata_model::ObjectId, Value> {
    match value {
        Some(value) => input::object_id(value),
        None if interactive => input::object_id(&prompt::line(
            locale,
            "metadata-authoring-object-prompt",
            None,
        )?),
        None => Err(input::required(&[field])),
    }
}
