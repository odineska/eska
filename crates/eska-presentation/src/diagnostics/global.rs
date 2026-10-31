//! Shared presentation of machine-local configuration errors.

use eska_project_core::config::global::GlobalConfigError;

use super::path_message;
use crate::{
    localization::{LocalizationValue, Localizer},
    path_output,
};

/// Keep existing consumers plain while new/import explicitly opt into terminal path links.
pub fn present_global_config_error(error: &GlobalConfigError, localizer: &Localizer) -> String {
    present_global_config_error_with_links(error, localizer, false)
}

/// Present machine-local config failures shared by config, build, platform and patch commands.
pub fn present_global_config_error_with_links(
    error: &GlobalConfigError,
    localizer: &Localizer,
    hyperlinks: bool,
) -> String {
    match error {
        GlobalConfigError::UnknownLibrary { path, name }
        | GlobalConfigError::EmptyLibrarySource { path, name } => localizer.format(
            if matches!(error, GlobalConfigError::UnknownLibrary { .. }) {
                "config-library-unknown"
            } else {
                "config-library-source-empty"
            },
            &[
                ("name", LocalizationValue::Text(name)),
                (
                    "path",
                    LocalizationValue::Text(&path_output::render(path, hyperlinks)),
                ),
            ],
        ),
        GlobalConfigError::LocationUnavailable => localizer.text("config-location-error"),
        GlobalConfigError::Io { path, source } | GlobalConfigError::Replace { path, source } => {
            localizer.format(
                "config-io-error",
                &[
                    (
                        "path",
                        LocalizationValue::Text(&path_output::render(path, hyperlinks)),
                    ),
                    ("reason", LocalizationValue::Text(&source.to_string())),
                ],
            )
        }
        GlobalConfigError::Invalid { path, source } => localizer.format(
            "config-invalid",
            &[
                (
                    "path",
                    LocalizationValue::Text(&path_output::render(path, hyperlinks)),
                ),
                ("reason", LocalizationValue::Text(&source.to_string())),
            ],
        ),
        GlobalConfigError::DistroboxContainerMissing { path } => path_message(
            localizer,
            "config-distrobox-container-missing",
            path,
            hyperlinks,
        ),
        GlobalConfigError::HostContainerUnexpected { path } => path_message(
            localizer,
            "config-host-container-unexpected",
            path,
            hyperlinks,
        ),
        GlobalConfigError::Editor { source } => localizer.format(
            "config-editor-error",
            &[("reason", LocalizationValue::Text(&source.to_string()))],
        ),
        GlobalConfigError::EditorFailed => localizer.text("config-editor-failed"),
    }
}
