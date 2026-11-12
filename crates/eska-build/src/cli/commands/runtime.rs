use std::{collections::BTreeSet, path::Path, process::ExitCode};

use eska_build_runtime::runtime::apply::{
    self, ApplyError, Change, Mode, ObjectKind, ObjectOperation, Plan, Review, ReviewReason, Stage,
    Status,
};
use eska_build_runtime::runtime::{self, RuntimeError};
use eska_presentation::{
    arguments::build::runtime::{ApplyArgs, OutputFormat, ProfileArg, RunArgs},
    diagnostics,
    localization::Localizer,
};
use eska_project_core::project::{
    discovery,
    local_settings::{ProfileName, SettingsError},
};
use serde_json::json;

pub fn run(args: &RunArgs, start: &Path, localizer: &Localizer, inspect: bool) -> ExitCode {
    let kind = if inspect { "runtime_probe" } else { "run" };
    if !inspect && matches!(args.profile, ProfileArg::Prod) {
        return failure(
            args,
            kind,
            "forbidden_action",
            &localizer.text("project-settings-forbidden"),
        );
    }
    let project = match discovery::discover(start) {
        Ok(project) => project,
        Err(error) => {
            return failure(
                args,
                kind,
                "project_discovery",
                &diagnostics::present_project_error(&error, localizer),
            );
        }
    };
    let result = if inspect {
        let profile = match args.profile {
            ProfileArg::Test => ProfileName::Test,
            ProfileArg::Prod => ProfileName::Prod,
        };
        runtime::inspect(&project, profile)
    } else {
        runtime::run(&project, ProfileName::Test)
    };
    match result {
        Ok(()) => {
            let (status, message) = if inspect {
                ("ready", "runtime-probe-ready")
            } else {
                ("finished", "runtime-client-exited")
            };
            match args.format {
                OutputFormat::Human => println!("{}", localizer.text(message)),
                OutputFormat::Json => println!(
                    "{}",
                    json!({"schema_version":1,"kind":kind,"status":status})
                ),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let (code, message) = match error {
                RuntimeError::Settings(SettingsError::MissingProfile(_)) => {
                    ("missing_profile", "project-settings-missing-profile")
                }
                RuntimeError::Settings(_) => ("invalid_settings", "project-settings-invalid"),
                RuntimeError::Platform(_) | RuntimeError::PlatformUnavailable => {
                    ("platform_unavailable", "runtime-platform-unavailable")
                }
                RuntimeError::FileInfobaseUnavailable => {
                    ("infobase_unavailable", "runtime-infobase-unavailable")
                }
                RuntimeError::EndpointUnavailable => {
                    ("endpoint_unavailable", "runtime-endpoint-unavailable")
                }
                RuntimeError::SecretUnavailable => {
                    ("secret_unavailable", "runtime-secret-unavailable")
                }
                RuntimeError::Run(eska_build_runtime::build::RunError::Interrupted) => {
                    ("cancelled", "component-cancelled")
                }
                RuntimeError::Run(_) | RuntimeError::Io(_) | RuntimeError::ClientFailed => {
                    ("client_failed", "runtime-client-failed")
                }
            };
            failure(args, kind, code, &localizer.text(message))
        }
    }
}

fn failure(args: &RunArgs, kind: &str, code: &str, message: &str) -> ExitCode {
    if matches!(args.format, OutputFormat::Json) {
        println!(
            "{}",
            json!({"schema_version":1,"kind":kind,"status":"error","error":{"code":code}})
        );
    }
    eprintln!("{message}");
    ExitCode::FAILURE
}

pub fn apply(args: &ApplyArgs, start: &Path, localizer: &Localizer) -> ExitCode {
    if matches!(args.profile, ProfileArg::Prod) {
        return apply_failure(
            args,
            localizer,
            "forbidden_action",
            &localizer.text("project-settings-forbidden"),
            None,
        );
    }
    let project = match discovery::discover(start) {
        Ok(project) => project,
        Err(error) => {
            return apply_failure(
                args,
                localizer,
                "project_discovery",
                &diagnostics::present_project_error(&error, localizer),
                None,
            );
        }
    };
    let result = args.confirm.as_ref().map_or_else(
        || apply::preview(&project, ProfileName::Test, args.initialize),
        |confirmation| {
            apply::execute(
                &project,
                ProfileName::Test,
                args.initialize,
                confirmation,
                |stage| {
                    let (phase, key) = if stage == Stage::Import {
                        ("import", "runtime-apply-importing")
                    } else {
                        ("apply", "runtime-apply-applying")
                    };
                    if matches!(args.format, OutputFormat::Json) {
                        eprintln!(
                            "{}",
                            json!({"schema_version":1,"kind":"apply_progress","phase":phase})
                        );
                    } else {
                        eprintln!("{}", localizer.text(key));
                    }
                },
            )
        },
    );
    match result {
        Ok(plan) => {
            let status = if args.confirm.is_some() {
                "applied"
            } else {
                "preview"
            };
            if matches!(args.format, OutputFormat::Json) {
                println!(
                    "{}",
                    json!({"schema_version":1,"kind":"apply","status":status,"plan":plan})
                );
            } else if args.confirm.is_some() {
                println!("{}", localizer.text("runtime-apply-done"));
            } else {
                print_apply_plan(&plan, localizer);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let (code, message) = apply_error(&error);
            let review = match &error {
                ApplyError::ReviewRequired(_, review) => Some(review.as_ref()),
                _ => None,
            };
            apply_failure(args, localizer, code, &localizer.text(message), review)
        }
    }
}

fn print_apply_plan(plan: &Plan, localizer: &Localizer) {
    println!(
        "{}",
        localizer.text(match plan.mode {
            Mode::Initialize => "runtime-apply-initialize",
            Mode::Incremental => "runtime-apply-incremental",
            Mode::Unchanged => "runtime-apply-unchanged",
            Mode::Unsupported => "runtime-apply-unsupported",
        })
    );
    println!("{} ({})", plan.target, plan.platform_version);
    println!("{}", localizer.text(inspection_key(plan.inspection.status)));
    let grouped = print_object_changes(plan, localizer);
    for (path, change) in &plan.changes {
        if grouped.contains(path) {
            continue;
        }
        let label = localizer.text(change_key(*change));
        println!("{label} {}", path.display());
    }
    if let Some(files) = &plan.import_files {
        for path in files
            .iter()
            .filter(|path| !plan.changes.contains_key(*path))
        {
            let label = localizer.text("runtime-apply-required-file");
            println!("{label} {}", path.display());
        }
    }
    if let Some(confirmation) = &plan.confirmation {
        println!("--confirm {confirmation}");
    }
}

fn print_object_changes<'a>(
    plan: &'a Plan,
    localizer: &Localizer,
) -> BTreeSet<&'a std::path::PathBuf> {
    let mut grouped = BTreeSet::new();
    for object in &plan.object_changes {
        let action = localizer.text(match object.operation {
            ObjectOperation::Added => "runtime-apply-added",
            ObjectOperation::Removed => "runtime-apply-removed",
            ObjectOperation::Renamed => "runtime-apply-renamed",
        });
        let kind = localizer.text(match object.kind {
            ObjectKind::Language => "diff-metadata-language",
            ObjectKind::CommonForm => "diff-metadata-common-form",
        });
        if let Some(previous) = &object.previous_name {
            println!("{action} {kind} {previous} → {}", object.name);
        } else {
            println!("{action} {kind} {}", object.name);
        }
        for path in &object.paths {
            grouped.insert(path);
            println!("  {}", path.display());
        }
    }
    grouped
}

fn apply_failure(
    args: &ApplyArgs,
    localizer: &Localizer,
    code: &str,
    message: &str,
    review: Option<&Review>,
) -> ExitCode {
    if matches!(args.format, OutputFormat::Json) {
        let mut response =
            json!({"schema_version":1,"kind":"apply","status":"error","error":{"code":code}});
        if let Some(review) = review {
            response["review"] = json!(review);
        }
        println!("{response}");
    } else if let Some(review) = review {
        eprintln!(
            "{}",
            localizer.text(inspection_key(review.inspection.status))
        );
        if let Some(changes) = &review.changes {
            for (path, change) in changes {
                let label = localizer.text(change_key(*change));
                eprintln!("{label} {}", path.display());
            }
        }
    }
    eprintln!("{message}");
    ExitCode::FAILURE
}

const fn change_key(change: Change) -> &'static str {
    match change {
        Change::Added => "runtime-apply-added",
        Change::Modified => "runtime-apply-modified",
        Change::Removed => "runtime-apply-removed",
    }
}

const fn inspection_key(status: Status) -> &'static str {
    match status {
        Status::UpToDate => "runtime-apply-status-current",
        Status::LocalChanges => "runtime-apply-status-local",
        Status::DatabaseChanged => "runtime-apply-status-database",
        Status::BothChanged => "runtime-apply-status-both",
        Status::UnknownBaseline => "runtime-apply-status-unknown",
        Status::DatabaseUnavailable => "runtime-apply-status-unavailable",
        Status::Incomplete => "runtime-apply-status-incomplete",
    }
}

const fn apply_error(error: &ApplyError) -> (&'static str, &'static str) {
    match error {
        ApplyError::Runtime(RuntimeError::Settings(SettingsError::MissingProfile(_))) => {
            ("missing_profile", "project-settings-missing-profile")
        }
        ApplyError::Runtime(RuntimeError::Settings(SettingsError::SharedInfobase)) => {
            ("shared_infobase", "project-settings-shared-infobase")
        }
        ApplyError::Runtime(RuntimeError::Settings(SettingsError::ClusterApplyUnsupported)) => (
            "cluster_apply_unsupported",
            "project-settings-cluster-apply",
        ),
        ApplyError::Runtime(RuntimeError::Settings(SettingsError::AdminEndpointRequired)) => {
            ("missing_admin_endpoint", "project-settings-missing-admin")
        }
        ApplyError::Runtime(RuntimeError::Settings(_)) => {
            ("invalid_settings", "project-settings-invalid")
        }
        ApplyError::Runtime(RuntimeError::FileInfobaseUnavailable) => {
            ("infobase_unavailable", "runtime-infobase-unavailable")
        }
        ApplyError::Runtime(RuntimeError::SecretUnavailable) => {
            ("secret_unavailable", "runtime-secret-unavailable")
        }
        ApplyError::Runtime(RuntimeError::Platform(_) | RuntimeError::PlatformUnavailable) => {
            ("platform_unavailable", "runtime-platform-unavailable")
        }
        ApplyError::ReviewRequired(reason, _) => match reason {
            ReviewReason::BaselineRequired => {
                ("baseline_required", "runtime-apply-baseline-required")
            }
            ReviewReason::BaselineChanged => ("baseline_changed", "runtime-apply-baseline-changed"),
            ReviewReason::Incomplete => ("incomplete_apply", "runtime-apply-incomplete"),
            ReviewReason::GenerationUnavailable => {
                ("generation_failed", "runtime-apply-inspect-failed")
            }
            ReviewReason::InvalidBaseline | ReviewReason::InvalidGeneration => {
                ("invalid_state", "runtime-apply-state-error")
            }
        },
        ApplyError::Busy => ("busy", "runtime-apply-busy"),
        ApplyError::StalePreview => ("stale_preview", "runtime-apply-stale"),
        ApplyError::ProjectType => ("project_type_unsupported", "runtime-apply-project-type"),
        ApplyError::SourceInsideInfobase => (
            "source_inside_infobase",
            "runtime-apply-source-inside-infobase",
        ),
        ApplyError::PlatformFailed(Stage::Inspect) => {
            ("generation_failed", "runtime-apply-inspect-failed")
        }
        ApplyError::PlatformFailed(Stage::Import) => {
            ("import_failed", "runtime-apply-import-failed")
        }
        ApplyError::PlatformFailed(Stage::Apply) => ("apply_failed", "runtime-apply-apply-failed"),
        ApplyError::Interrupted => ("cancelled", "component-cancelled"),
        _ => ("invalid_state", "runtime-apply-state-error"),
    }
}
