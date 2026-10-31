//! One dependency operation spans discovery, optional VCS checkout and checked configuration writes.

mod errors;

use crate::response::{self, Failure};
pub use eska_presentation::arguments::project::add::{AddArgs, localize};
use eska_presentation::{
    arguments::project::add::OutputFormat,
    localization::{LocalizationValue, Localizer},
    path_output,
};
use eska_project_core::{
    libraries::{AddPlan, Error, KnownLibrary, Revision},
    project::{
        discovery::{self, DiscoveryContext},
        selection::{SelectionIntent, select_projects},
    },
};
use eska_tool_runtime::{Component, Operation};
use serde_json::json;
use std::{ffi::OsString, path::Path, process::ExitCode};

pub fn run(args: &AddArgs, directory: &Path, localizer: &Localizer) -> ExitCode {
    match execute(args, directory, localizer) {
        Ok(result) => {
            if matches!(args.format, OutputFormat::Json) {
                println!(
                    "{}",
                    json!({"schema_version":1,"kind":"library-add","status":"added",
                    "library":result.library.name(),"source":response::path(&result.source),
                    "installation_revision":result.revision,"installed":result.installed,
                    "project_changed":result.project_changed,"catalog_changed":result.catalog_changed})
                );
            } else {
                println!(
                    "{}",
                    localizer.format(
                        if result.project_changed || result.catalog_changed || result.installed {
                            "library-add-attached"
                        } else {
                            "library-add-unchanged"
                        },
                        &[
                            ("name", LocalizationValue::Text(result.library.name())),
                            (
                                "path",
                                LocalizationValue::Text(&path_output::display_path(&result.source))
                            ),
                        ]
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => error.emit(match args.format {
            OutputFormat::Human => crate::cli::OutputFormat::Human,
            OutputFormat::Json => crate::cli::OutputFormat::Json,
        }),
    }
}

fn execute(
    args: &AddArgs,
    directory: &Path,
    localizer: &Localizer,
) -> Result<eska_project_core::libraries::AddResult, Failure> {
    let library =
        KnownLibrary::parse(&args.library).map_err(|error| errors::present(error, localizer))?;
    let context = discovery::discover_context(directory)
        .map_err(|error| response::discovery(&error, localizer))?;
    let selection = select_projects(
        &context,
        &args.project,
        false,
        SelectionIntent::SingleMutation,
    )
    .map_err(|_| {
        Failure::new(
            "project_selection",
            json!({}),
            localizer.text("library-add-selection"),
        )
    })?;
    let selected = selection.projects().first().ok_or_else(|| {
        Failure::new(
            "project_selection",
            json!({}),
            localizer.text("library-add-selection"),
        )
    })?;
    let workspace = match &context {
        DiscoveryContext::Standalone(_) => None,
        DiscoveryContext::Workspace { workspace, .. } => Some(workspace),
    };
    let plan = AddPlan::inspect(selected.project(), workspace, library)
        .map_err(|error| errors::present(error, localizer))?;
    let scope = Operation::new().map_err(|error| component_failure(&error, localizer))?;
    let mut component_error = None;
    let result = plan.execute(
        |library, destination| {
            eprintln!(
                "{}",
                localizer.format(
                    "library-add-downloading",
                    &[("name", LocalizationValue::Text(library.name()))]
                )
            );
            download(library, destination, selected.project().root(), &scope).map_err(|error| {
                component_error = Some(error);
                Error::Download
            })
        },
        || scope.is_cancelled(),
    );
    match result {
        Ok(result) => Ok(result),
        Err(error) => Err(component_error.as_ref().map_or_else(
            || errors::present(error, localizer),
            |error| component_failure(error, localizer),
        )),
    }
}

fn download(
    library: KnownLibrary,
    destination: &Path,
    root: &Path,
    scope: &Operation,
) -> Result<Revision, eska_tool_runtime::Error> {
    let binary = eska_tool_runtime::resolve(Component::Vcs)?;
    let args: Vec<OsString> = vec![
        "--project-dir".into(),
        root.as_os_str().to_owned(),
        "__bridge".into(),
        "library-checkout".into(),
        library.repository().into(),
        "--destination".into(),
        destination.as_os_str().to_owned(),
    ];
    let reply =
        eska_tool_runtime::exchange(Component::Vcs, &binary, &args, "library_checkout", scope)?;
    if reply.as_object().is_none_or(|object| object.len() != 2)
        || reply["reference"] != "refs/heads/master"
    {
        return Err(eska_tool_runtime::Error::InvalidResponse);
    }
    let revision = reply["revision"]
        .as_str()
        .ok_or(eska_tool_runtime::Error::InvalidResponse)?;
    Revision::parse(revision.into()).map_err(|_| eska_tool_runtime::Error::InvalidResponse)
}

fn component_failure(error: &eska_tool_runtime::Error, localizer: &Localizer) -> Failure {
    Failure::new(
        error.code(),
        json!({}),
        eska_presentation::components::present(error, localizer),
    )
}
