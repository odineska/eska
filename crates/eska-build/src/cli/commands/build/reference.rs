//! Select the same read-only configuration as analysis for each external project.

use std::{path::Path, process::ExitCode};

use eska_presentation::diagnostics::application::{context_data, library_message};
use eska_project_core::{
    application::{ContextError, ReferenceError, resolve_project_reference},
    project::{ProjectName, ProjectType},
};
use serde_json::json;

use super::{
    BuildHandler, DiscoveryContext, LocalizationValue, Localizer, OutputFormat, SelectedProject,
    build::BaseConfiguration, json::write_build_error,
};

impl BuildHandler<'_> {
    pub(super) fn library_producer(
        &self,
        project: &eska_project_core::project::Project,
        localizer: &Localizer,
    ) -> Result<Option<eska_project_core::application::LibrarySelection>, ExitCode> {
        if project.configuration().project_type() != ProjectType::Configuration {
            return Ok(None);
        }
        eska_project_core::config::global::registered_library(project.source()).map_err(|error| {
            self.fail(
                "machine-config",
                Some("configuration"),
                None,
                &crate::cli::diagnostics::present_global_config_error(&error, localizer),
                localizer,
                ExitCode::FAILURE,
            )
        })
    }
    /// Resolve every declaration before machine-tool discovery so catalog errors retain ownership.
    pub(super) fn base_configurations(
        &self,
        context: &DiscoveryContext,
        selected: &[SelectedProject<'_>],
        localizer: &Localizer,
    ) -> Result<Vec<Option<BaseConfiguration>>, ExitCode> {
        if let Some(path) = self.resolved_base_configuration(context) {
            return Ok(selected
                .iter()
                .map(|_| Some(BaseConfiguration::Cf(path.clone())))
                .collect());
        }
        let workspace = match context {
            DiscoveryContext::Standalone(_) => None,
            DiscoveryContext::Workspace { workspace, .. } => Some(workspace),
        };
        selected
            .iter()
            .map(|selected| {
                if !matches!(
                    selected.project().configuration().project_type(),
                    ProjectType::Processing | ProjectType::Report
                ) {
                    return Ok(None);
                }
                resolve_project_reference(selected.project(), workspace)
                    .map(|source| {
                        source.map(|source| BaseConfiguration::DesignerXml {
                            source: source.project().source().to_owned(),
                            library: source.library().cloned(),
                        })
                    })
                    .map_err(|error| self.fail_reference(&error, *selected, localizer))
            })
            .collect()
    }

    fn fail_reference(
        &self,
        error: &ReferenceError,
        selected: SelectedProject<'_>,
        localizer: &Localizer,
    ) -> ExitCode {
        let reason = library_message(&error.source, localizer).unwrap_or_else(|| {
            localizer.text(
                if matches!(*error.source, ContextError::AmbiguousApplication { .. }) {
                    "build-reference-ambiguous"
                } else {
                    "build-reference-invalid"
                },
            )
        });
        let field = error.application.as_ref().map_or_else(
            || error.field.to_owned(),
            |application| format!("workspace.members.{application}.{}", error.field),
        );
        let data = json!({
            "file": encoded_path(&error.path), "field":field,
            "application":error.application, "cause":context_data(&error.source)
        });
        self.fail_configuration(
            "base-reference",
            "configuration",
            selected,
            &configuration_message(&error.path, &field, &reason, localizer),
            data,
            localizer,
        )
    }

    /// A missing inherited value points to a persistent configuration field, not a required flag.
    pub(super) fn fail_missing_platform(
        &self,
        selected: SelectedProject<'_>,
        workspace_root: Option<&Path>,
        localizer: &Localizer,
    ) -> ExitCode {
        let path = workspace_root
            .unwrap_or_else(|| selected.project().root())
            .join("eska.toml");
        self.fail_configuration(
            "platform-version-missing", "plan", selected,
            &configuration_message(&path, "build.platform_version", &localizer.text("build-platform-version-missing"), localizer),
            json!({"file":encoded_path(&path), "field":"build.platform_version", "reason":"missing"}),
            localizer,
        )
    }

    fn fail_configuration(
        &self,
        code: &'static str,
        stage: &'static str,
        selected: SelectedProject<'_>,
        detail: &str,
        data: serde_json::Value,
        localizer: &Localizer,
    ) -> ExitCode {
        if matches!(self.options.format, OutputFormat::Json) {
            write_build_error(
                code,
                Some(stage),
                selected.name().map(ProjectName::as_str),
                Some(data),
                localizer,
            );
        }
        eprintln!("{detail}");
        ExitCode::FAILURE
    }
}

fn configuration_message(path: &Path, field: &str, reason: &str, localizer: &Localizer) -> String {
    localizer.format(
        "build-configuration-error",
        &[
            (
                "path",
                LocalizationValue::Text(&eska_presentation::path_output::display_path(path)),
            ),
            ("field", LocalizationValue::Text(field)),
            ("reason", LocalizationValue::Text(reason)),
        ],
    )
}

fn encoded_path(path: &Path) -> serde_json::Value {
    let (value, encoding) = eska_presentation::encoding::json_path(path.as_os_str());
    json!({"value":value,"encoding":encoding})
}
