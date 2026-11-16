//! CLI authoring is a thin client of the same semantic plans exposed to the editor.

mod input;
mod objects;
mod presentation;
mod prompt;

use std::{io::IsTerminal, path::Path, process::ExitCode};

use eska_presentation::{
    arguments::project::metadata::{AuthoringInput, MetadataArgs, MetadataCommand, OutputFormat},
    localization::Localizer,
};
use eska_project_core::project::{
    metadata_form_edit::{FormEditRequest, PreparedFormEdit},
    metadata_workspace::{MetadataWorkspace, ProjectSession},
};
use serde_json::Value;

use super::failure;
use crate::ide::{authoring as shared, dto, editing};

pub(super) const fn handles(command: &MetadataCommand) -> bool {
    matches!(
        command,
        MetadataCommand::CreateModule(_)
            | MetadataCommand::Create(_)
            | MetadataCommand::Copy(_)
            | MetadataCommand::Delete(_)
            | MetadataCommand::ObjectSchema(_)
            | MetadataCommand::FormSchema(_)
            | MetadataCommand::FormEdit(_)
            | MetadataCommand::Recover(_)
            | MetadataCommand::List(_)
    )
}

pub(super) fn run(args: &MetadataArgs, directory: &Path, locale: &Localizer) -> ExitCode {
    let mutation = matches!(
        args.command,
        MetadataCommand::CreateModule(_)
            | MetadataCommand::Create(_)
            | MetadataCommand::Copy(_)
            | MetadataCommand::Delete(_)
            | MetadataCommand::FormEdit(_)
    );
    let format = args.format.unwrap_or(if mutation {
        OutputFormat::Human
    } else {
        OutputFormat::Json
    });
    let result = execute(args, directory, locale, format);
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

fn execute(
    args: &MetadataArgs,
    directory: &Path,
    locale: &Localizer,
    format: OutputFormat,
) -> Result<Value, Value> {
    let selected: Vec<_> = args.project.iter().cloned().collect();
    if let MetadataCommand::Recover(input) = &args.command {
        return recover(&selected_project(directory, &selected)?, &input.input);
    }
    let loaded = objects::load(&args.command)?;
    if objects::configuration(&args.command, loaded.as_ref()) {
        return objects::run_configuration(
            &selected_project(directory, &selected)?,
            &args.command,
            locale,
            format,
            loaded,
        );
    }
    if let MetadataCommand::ObjectSchema(options) = &args.command
        && options.parent.is_none()
    {
        let project = selected_project(directory, &selected)?;
        if project.configuration().project_type()
            == eska_project_core::project::ProjectType::Configuration
            && eska_project_core::project::metadata_objects::bootstrap::empty_source(&project)
                .map_err(shared::object_failure)?
        {
            let labels = dto::Labels::new().map_err(|_| failure("localization_failed"))?;
            let kind = options
                .kind
                .as_deref()
                .map(input::metadata_kind)
                .transpose()?;
            let mut result = shared::object_schema(
                &labels,
                &eska_project_core::project::metadata_objects::bootstrap::schema(),
            );
            if let Some(kind) = kind {
                if kind != eska_project_core::project::metadata_model::MetadataKind::Configuration {
                    return Err(shared::object_failure(
                        eska_project_core::project::metadata_objects::ObjectError::Unsupported(
                            kind,
                        ),
                    ));
                }
                result["selectedKind"] = serde_json::json!(kind);
                result["initialFields"] = serde_json::json!([]);
            }
            return Ok(result);
        }
    }
    let mut workspace = MetadataWorkspace::open(directory, &selected, false)
        .map_err(|error| editing::failure(error.into()))?;
    if workspace.projects().len() != 1 {
        return Err(failure("selection_invalid"));
    }
    let scope = workspace.projects()[0].scope().clone();
    let project = workspace
        .project_mut(&scope)
        .map_err(|error| editing::failure(error.into()))?;
    let labels = dto::Labels::new().map_err(|_| failure("localization_failed"))?;
    match &args.command {
        MetadataCommand::ObjectSchema(args) => {
            let parent = args.parent.as_deref().map(input::object_id).transpose()?;
            let kind = args.kind.as_deref().map(input::metadata_kind).transpose()?;
            shared::object_schema_for(&labels, project, parent.as_ref(), kind)
        }
        MetadataCommand::List(args) => {
            let parent = args.parent.as_deref().map(input::object_id).transpose()?;
            let kind = args.kind.as_deref().map(input::metadata_kind).transpose()?;
            shared::list(
                &labels,
                project,
                &shared::ListOptions {
                    parent: parent.as_ref(),
                    kind,
                    search: args.search.as_deref(),
                    limit: args.limit as usize,
                    after: args.after.as_deref(),
                },
            )
        }
        MetadataCommand::FormSchema(args) => project
            .form_edit_schema(&input::object_id(&args.form)?)
            .map(|schema| {
                let mut result = shared::form_schema(&labels, &schema);
                result["typeOptions"] = shared::form_type_options(&labels, project);
                result
            })
            .map_err(shared::form_failure),
        MetadataCommand::FormEdit(args) => edit_form(project, args),
        MetadataCommand::CreateModule(_)
        | MetadataCommand::Create(_)
        | MetadataCommand::Copy(_)
        | MetadataCommand::Delete(_) => {
            objects::run(project, &args.command, locale, format, loaded)
        }
        _ => Err(failure("invalid_request")),
    }
}

/// Reuse manifest discovery and explicit workspace selection before a root XML exists.
fn selected_project(
    directory: &Path,
    names: &[String],
) -> Result<eska_project_core::project::Project, Value> {
    use eska_project_core::project::{
        designer_source::SourceError,
        discovery::discover_context,
        metadata_workspace::WorkspaceError,
        selection::{SelectionIntent, select_projects},
    };
    let context = discover_context(directory).map_err(|error| {
        editing::failure(WorkspaceError::Source(SourceError::Discovery(error)).into())
    })?;
    let selected = select_projects(&context, names, false, SelectionIntent::SingleMutation)
        .map_err(|error| {
            editing::failure(WorkspaceError::Source(SourceError::Selection(error)).into())
        })?;
    selected
        .projects()
        .first()
        .map(|selected| selected.project().clone())
        .ok_or_else(|| failure("selection_invalid"))
}

fn recover(project: &eska_project_core::project::Project, path: &Path) -> Result<Value, Value> {
    use eska_project_core::project::{metadata_workspace::PropertyEditError, metadata_write};
    let input = super::read_input(path, &["schemaVersion", "action", "snapshot"])?;
    match input["action"].as_str() {
        Some("inspect") if input.get("snapshot").is_none() => {
            metadata_write::inspect_project_recovery(project)
                .map(|status| serde_json::json!(status))
                .map_err(|error| editing::failure(PropertyEditError::Edit(error)))
        }
        Some("restore") => {
            let expected = input["snapshot"]
                .as_str()
                .ok_or_else(|| failure("invalid_request"))?;
            metadata_write::recover_project(project, expected)
                .map_err(|error| editing::failure(PropertyEditError::Edit(error)))?;
            Ok(serde_json::json!({"recovered":true}))
        }
        _ => Err(failure("invalid_request")),
    }
}

fn edit_form(project: &mut ProjectSession, args: &AuthoringInput) -> Result<Value, Value> {
    if let Some(path) = &args.plan {
        let prepared: PreparedFormEdit = input::plan(path, "form-edit")?;
        return if args.dry_run {
            project
                .preview_prepared_form_edit(&prepared)
                .map(|plan| shared::form_plan(&plan))
                .map_err(shared::form_failure)
        } else {
            project
                .apply_form_edit(&prepared)
                .map(|result| shared::form_result(&result))
                .map_err(shared::form_failure)
        };
    }
    let path = args
        .input
        .as_deref()
        .ok_or_else(|| input::required(&["input"]))?;
    let request: FormEditRequest = input::request(path)?;
    let plan = project
        .preview_form_edit(&request)
        .map_err(shared::form_failure)?;
    if args.dry_run {
        return Ok(shared::form_plan(&plan));
    }
    project
        .apply_form_edit(&plan.prepared)
        .map(|result| shared::form_result(&result))
        .map_err(shared::form_failure)
}

fn interactive(args: &AuthoringInput, format: OutputFormat) -> bool {
    format == OutputFormat::Human
        && !args.no_input
        && args.input.is_none()
        && args.plan.is_none()
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
}
