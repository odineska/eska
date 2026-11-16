//! Human and machine entry points for semantic creation share one backend transaction.

mod arguments;
mod presentation;
mod prompt;

use std::{io::IsTerminal, path::Path, process::ExitCode};

use eska_presentation::{
    arguments::project::metadata::{AddArgs, MetadataArgs, MetadataCommand, OutputFormat},
    localization::Localizer,
};
use eska_project_core::project::{
    metadata_create::{CreateRequest, PreparedCreate, capabilities},
    metadata_workspace::{MetadataWorkspace, ProjectSession},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ide::{creation as shared, dto, editing};

use super::{failure, read_input, read_json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InputRequest {
    schema_version: u32,
    request: CreateRequest,
}

/// Output selection changes only new add commands; the existing JSON contract stays intact.
pub(super) fn run(args: &MetadataArgs, directory: &Path, locale: &Localizer) -> ExitCode {
    let is_add = matches!(args.command, MetadataCommand::Add(_));
    let format = args.format.unwrap_or(if is_add {
        OutputFormat::Human
    } else {
        OutputFormat::Json
    });
    if !is_add && format == OutputFormat::Human {
        eprintln!("{}", locale.text("metadata-create-human-only-add"));
        return ExitCode::from(2);
    }
    let result = execute(args, directory, locale);
    if format == OutputFormat::Json {
        return super::json_result(result);
    }
    match result {
        Ok(value) => {
            presentation::success(&value, locale);
            ExitCode::SUCCESS
        }
        Err(error) => {
            presentation::error(&error, locale);
            ExitCode::FAILURE
        }
    }
}

fn execute(args: &MetadataArgs, directory: &Path, locale: &Localizer) -> Result<Value, Value> {
    let labels = dto::Labels::new().map_err(|_| failure("localization_failed"))?;
    if matches!(&args.command, MetadataCommand::CreateSchema(args) if args.owner.is_none()) {
        return Ok(
            json!({"schemaVersion":1,"capabilities":shared::capabilities(&labels, &capabilities())}),
        );
    }
    let selected: Vec<_> = args.project.iter().cloned().collect();
    let mut workspace = MetadataWorkspace::open(directory, &selected, false)
        .map_err(|error| editing::failure(error.into()))?;
    if workspace.projects().len() != 1 {
        return Err(failure("selection_invalid"));
    }
    let scope = workspace.projects()[0].scope().clone();
    let project = workspace
        .project_mut(&scope)
        .map_err(|error| editing::failure(error.into()))?;
    match &args.command {
        MetadataCommand::CreateSchema(args) => {
            let id = arguments::object_id(
                args.owner
                    .as_deref()
                    .ok_or_else(|| arguments::required(&["owner"]))?,
            )?;
            project
                .creation_schema(&id)
                .map(|schema| shared::schema(&labels, &schema))
                .map_err(shared::failure)
        }
        MetadataCommand::Add(add_args) => add(project, add_args, args.project.as_deref(), locale),
        _ => Err(failure("invalid_request")),
    }
}

fn add(
    project: &mut ProjectSession,
    args: &AddArgs,
    selected: Option<&str>,
    locale: &Localizer,
) -> Result<Value, Value> {
    if let Some(path) = &args.plan {
        let prepared = read_plan(path)?;
        if args.dry_run {
            // Replaying a saved plan must retain its allocated IDs even in another preview.
            return project
                .preview_prepared_create(&prepared)
                .map(|plan| shared::plan(&plan))
                .map_err(shared::failure);
        }
        return project
            .apply_create(&prepared)
            .map(|result| shared::result(&result))
            .map_err(shared::failure);
    }
    let missing = arguments::missing(args);
    let interactive = args.interactive
        || args.input.is_none()
            && !args.no_input
            && !missing.is_empty()
            && std::io::stdin().is_terminal()
            && std::io::stderr().is_terminal();
    if interactive && (!std::io::stdin().is_terminal() || !std::io::stderr().is_terminal()) {
        return Err(failure("creation_terminal_required"));
    }
    let request = if let Some(path) = &args.input {
        let value = read_input(path, &["schemaVersion", "request"])?;
        let input: InputRequest =
            serde_json::from_value(value).map_err(|_| input_error("input", "invalid_request"))?;
        if input.schema_version != 1 {
            return Err(input_error("schemaVersion", "unsupported_version"));
        }
        input.request
    } else if interactive {
        prompt::request(project, args, locale)?
    } else {
        if !missing.is_empty() {
            return Err(arguments::required(&missing));
        }
        let owner = arguments::object_id(
            args.owner
                .as_deref()
                .ok_or_else(|| arguments::required(&["owner"]))?,
        )?;
        let schema = project.creation_schema(&owner).map_err(shared::failure)?;
        arguments::request(args, &schema)?
    };
    let plan = project.preview_create(&request).map_err(shared::failure)?;
    if interactive {
        if !args.dry_run {
            presentation::review(&plan, locale);
        }
        presentation::equivalent(&request, selected, locale);
    }
    if args.dry_run {
        return Ok(shared::plan(&plan));
    }
    if interactive && !prompt::confirm(locale)? {
        return Err(failure("creation_cancelled"));
    }
    project
        .apply_create(&plan.prepared)
        .map(|result| shared::result(&result))
        .map_err(shared::failure)
}

/// Read only the typed prepared member; display-only diff/path fields are never executed.
fn read_plan(path: &Path) -> Result<PreparedCreate, Value> {
    let value = read_json(path)?;
    let outer = value
        .as_object()
        .ok_or_else(|| input_error("plan", "invalid_envelope"))?;
    if value["schema_version"] != 1
        || outer
            .keys()
            .any(|key| !["schema_version", "result"].contains(&key.as_str()))
    {
        return Err(input_error("plan", "invalid_envelope"));
    }
    let result = value["result"]
        .as_object()
        .ok_or_else(|| input_error("plan", "invalid_result"))?;
    if value["result"]["schemaVersion"] != 1
        || value["result"]["valid"] != true
        || result.keys().any(|key| {
            ![
                "schemaVersion",
                "valid",
                "changed",
                "objectId",
                "source",
                "snapshot",
                "prepared",
                "changes",
            ]
            .contains(&key.as_str())
        })
    {
        return Err(input_error("plan", "invalid_result"));
    }
    serde_json::from_value(value["result"]["prepared"].clone())
        .map_err(|_| input_error("plan", "invalid_prepared_request"))
}

fn input_error(field: &str, reason: &str) -> Value {
    json!({"kind":"creation_input_invalid","details":{"field":field,"reason":reason}})
}
