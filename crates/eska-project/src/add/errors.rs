//! Localized dependency errors retain stable categories and reversible-operation coordinates.

use crate::response::{self, Failure};
use eska_presentation::{
    diagnostics,
    localization::{LocalizationValue, Localizer},
    path_output,
};
use eska_project_core::libraries::Error;
use serde_json::json;
use std::path::Path;

pub(super) fn present(error: Error, localizer: &Localizer) -> Failure {
    match error {
        Error::UnknownLibrary(name) => Failure::new(
            "library_unknown",
            json!({"library":name}),
            localizer.format(
                "library-add-unknown",
                &[("name", LocalizationValue::Text(&name))],
            ),
        ),
        Error::HomeUnavailable => simple("library_home", "library-add-home", localizer),
        Error::UnsupportedType(kind) => Failure::new(
            "library_project_type",
            json!({"project_type":kind.as_str()}),
            localizer.text("library-add-type"),
        ),
        Error::Conflict { path, field } => Failure::new(
            "library_conflict",
            json!({"path":response::path(&path),"field":field}),
            localizer.format(
                "library-add-conflict",
                &[
                    (
                        "path",
                        LocalizationValue::Text(&path_output::display_path(&path)),
                    ),
                    ("field", LocalizationValue::Text(&field)),
                ],
            ),
        ),
        Error::Changed(path) => {
            location("library_changed", "library-add-changed", &path, localizer)
        }
        Error::Locked(path) => location("library_locked", "library-add-locked", &path, localizer),
        Error::NonUtf8(path) => location(
            "library_path_encoding",
            "library-add-path",
            &path,
            localizer,
        ),
        Error::InvalidReceipt(path) => location(
            "library_install_invalid",
            "library-add-install-invalid",
            &path,
            localizer,
        ),
        Error::Io { path, source } => Failure::new(
            "library_io",
            json!({"path":response::path(&path),"os_code":source.raw_os_error()}),
            localizer.format(
                "library-add-io",
                &[
                    (
                        "path",
                        LocalizationValue::Text(&path_output::display_path(&path)),
                    ),
                    ("reason", LocalizationValue::Text(&source.to_string())),
                ],
            ),
        ),
        Error::Toml { path, .. } => {
            location("library_manifest", "library-add-manifest", &path, localizer)
        }
        Error::Global(source) => Failure::new(
            "library_catalog",
            json!({"cause":source.code(),"path":source.path().map(response::path)}),
            diagnostics::global::present_global_config_error(&source, localizer),
        ),
        Error::Context(source) => Failure::new(
            "library_application",
            diagnostics::application::context_data(&source),
            localizer.text("library-add-owner"),
        ),
        Error::Source(source) => response::source(&source, Path::new("src/cf"), localizer),
        Error::Project(_) | Error::Workspace(_) | Error::ProjectPath(_) => {
            simple("library_manifest", "library-add-config", localizer)
        }
        Error::Download => simple("library_download", "library-add-download", localizer),
        Error::Cancelled => simple("cancelled", "component-cancelled", localizer),
        Error::Rollback { path, source } => {
            let reason = present(*source, localizer);
            Failure::new(
                "library_rollback",
                json!({"path":response::path(&path)}),
                localizer.format(
                    "library-add-rollback",
                    &[
                        (
                            "path",
                            LocalizationValue::Text(&path_output::display_path(&path)),
                        ),
                        ("reason", LocalizationValue::Text(reason.message())),
                    ],
                ),
            )
        }
    }
}

fn simple(code: &'static str, key: &str, localizer: &Localizer) -> Failure {
    Failure::new(code, json!({}), localizer.text(key))
}
fn location(code: &'static str, key: &str, path: &Path, localizer: &Localizer) -> Failure {
    Failure::new(
        code,
        json!({"path":response::path(path)}),
        localizer.format(
            key,
            &[(
                "path",
                LocalizationValue::Text(&path_output::display_path(path)),
            )],
        ),
    )
}
