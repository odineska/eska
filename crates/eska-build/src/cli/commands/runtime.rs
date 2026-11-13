use std::{
    collections::BTreeSet,
    io::{self, Read as _},
    path::Path,
    process::ExitCode,
};

use eska_build_runtime::runtime::apply::{
    self, ApplyError, Change, Mode, ObjectKind, ObjectOperation, Plan, Review, ReviewReason, Stage,
    Status,
    application::{self, ComponentOutcome, ComponentStatus},
    installation, removal,
};
use eska_build_runtime::runtime::{self, RuntimeError};
use eska_presentation::{
    arguments::build::runtime::{
        ApplyArgs, InstallExtensionArgs, OutputFormat, ProbeArgs, ProfileArg, RemoveExtensionArgs,
        RunArgs,
    },
    diagnostics,
    localization::{LocalizationValue, Localizer},
};
use eska_project_core::project::{
    Project, discovery,
    local_settings::{Profile, ProfileName, SettingsError},
};
use serde_json::json;

pub fn run(args: &RunArgs, start: &Path, localizer: &Localizer) -> ExitCode {
    execute(args, start, localizer, Operation::Run)
}

pub fn probe(args: &ProbeArgs, start: &Path, localizer: &Localizer) -> ExitCode {
    let operation = if args.stdin {
        Operation::ProbeDraft
    } else {
        Operation::ProbeSaved
    };
    execute(&args.common, start, localizer, operation)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Operation {
    Run,
    ProbeSaved,
    ProbeDraft,
}

fn execute(args: &RunArgs, start: &Path, localizer: &Localizer, operation: Operation) -> ExitCode {
    let (kind, status, success) = match operation {
        Operation::Run => ("run", "finished", "runtime-client-exited"),
        Operation::ProbeSaved => ("runtime_probe", "ready", "runtime-probe-ready"),
        Operation::ProbeDraft => ("runtime_probe", "ready", "runtime-probe-draft-ready"),
    };
    let forbidden = match operation {
        Operation::Run => matches!(args.profile, ProfileArg::Prod),
        _ => args.expected_infobase.is_some(),
    };
    if forbidden {
        return failure(
            args,
            kind,
            &json!({"code":"forbidden_action"}),
            &localizer.text("project-settings-forbidden"),
        );
    }
    let project = match discovery::discover(start) {
        Ok(project) => project,
        Err(error) => {
            return failure(
                args,
                kind,
                &json!({"code":"project_discovery"}),
                &diagnostics::present_project_error(&error, localizer),
            );
        }
    };
    let (profile, profile_id) = match args.profile {
        ProfileArg::Test => (ProfileName::Test, "test"),
        ProfileArg::Prod => (ProfileName::Prod, "prod"),
    };
    let result = match operation {
        Operation::Run => runtime::run(&project, profile, args.expected_infobase.as_deref()),
        Operation::ProbeSaved => runtime::inspect(&project, profile),
        Operation::ProbeDraft => read_probe_profile()
            .map_err(RuntimeError::Settings)
            .and_then(|draft| runtime::inspect_draft(&project, profile, &draft)),
    };
    match result {
        Ok(()) => {
            match args.format {
                OutputFormat::Human => println!("{}", localizer.text(success)),
                OutputFormat::Json => println!(
                    "{}",
                    json!({"schema_version":1,"kind":kind,"status":status})
                ),
            }
            ExitCode::SUCCESS
        }
        Err(RuntimeError::Settings(SettingsError::InvalidField { field, .. }))
            if operation == Operation::ProbeDraft =>
        {
            invalid_probe_field(args, localizer, profile, profile_id, field)
        }
        Err(error) => {
            let (code, message) = match error {
                RuntimeError::Settings(SettingsError::MissingProfile(_)) => {
                    ("missing_profile", "project-settings-missing-profile")
                }
                RuntimeError::Settings(SettingsError::TooLarge)
                    if operation == Operation::ProbeDraft =>
                {
                    ("settings_too_large", "project-settings-too-large")
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
                RuntimeError::LaunchTargetChanged => {
                    ("launch_target_changed", "runtime-launch-target-changed")
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
            failure(args, kind, &json!({"code":code}), &localizer.text(message))
        }
    }
}

fn read_probe_profile() -> Result<Profile, SettingsError> {
    const MAX_INPUT: u64 = 128 * 1024;
    let mut bytes = Vec::new();
    io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(SettingsError::Io)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(SettingsError::TooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| SettingsError::InvalidFile)
}

fn invalid_probe_field(
    args: &RunArgs,
    localizer: &Localizer,
    profile: ProfileName,
    profile_id: &str,
    field: &str,
) -> ExitCode {
    let message = localizer.format(
        "project-settings-invalid-field",
        &[
            ("profile", LocalizationValue::Text(profile_id)),
            ("field", LocalizationValue::Text(field)),
        ],
    );
    failure(
        args,
        "runtime_probe",
        &json!({"code":"invalid_field","data":{"profile":profile,"field":field}}),
        &message,
    )
}

fn failure(args: &RunArgs, kind: &str, error: &serde_json::Value, message: &str) -> ExitCode {
    if matches!(args.format, OutputFormat::Json) {
        println!(
            "{}",
            json!({"schema_version":1,"kind":kind,"status":"error","error":error})
        );
    }
    eprintln!("{message}");
    ExitCode::FAILURE
}

pub fn install_extension(
    args: &InstallExtensionArgs,
    start: &Path,
    localizer: &Localizer,
) -> ExitCode {
    let result = extension_project(start, args.profile, localizer).and_then(|project| {
        installation::install(
            &project,
            ProfileName::Test,
            args.confirm.as_deref(),
            |stage| {
                extension_progress(stage, args.format, localizer);
            },
        )
        .map_err(|error| present_apply_error(&error, localizer))
    });
    extension_response(
        "extension_install",
        args.format,
        args.confirm.as_ref().map(|_| "installed"),
        result,
        |plan| {
            if args.confirm.is_some() {
                println!("{}", localizer.text("runtime-extension-installed"));
            } else {
                print_installation_plan(plan, localizer);
            }
        },
    )
}

pub fn remove_extension(
    args: &RemoveExtensionArgs,
    start: &Path,
    localizer: &Localizer,
) -> ExitCode {
    let result = extension_project(start, args.profile, localizer).and_then(|project| {
        removal::remove(
            &project,
            ProfileName::Test,
            args.confirm.as_deref(),
            args.force,
            || {
                extension_progress(Stage::Remove, args.format, localizer);
            },
        )
        .map_err(|error| present_apply_error(&error, localizer))
    });
    extension_response(
        "extension_remove",
        args.format,
        args.confirm.as_ref().map(|_| "removed"),
        result,
        |plan| {
            if args.confirm.is_some() {
                println!("{}", localizer.text("runtime-extension-removed"));
            } else {
                print_removal_plan(plan, localizer);
            }
        },
    )
}

type ExtensionError = (serde_json::Value, String);

fn extension_project(
    start: &Path,
    profile: ProfileArg,
    localizer: &Localizer,
) -> Result<Project, ExtensionError> {
    if matches!(profile, ProfileArg::Prod) {
        return Err((
            json!({"code":"forbidden_action"}),
            localizer.text("project-settings-forbidden"),
        ));
    }
    discovery::discover(start).map_err(|error| {
        (
            json!({"code":"project_discovery"}),
            diagnostics::present_project_error(&error, localizer),
        )
    })
}

fn extension_progress(stage: Stage, format: OutputFormat, localizer: &Localizer) {
    let (phase, key) = match stage {
        Stage::Import => ("import", "runtime-apply-importing"),
        Stage::Apply => ("apply", "runtime-apply-applying"),
        Stage::Remove => ("remove", "runtime-extension-removing"),
        Stage::Inspect => return,
    };
    if matches!(format, OutputFormat::Json) {
        eprintln!(
            "{}",
            json!({"schema_version":1,"kind":"extension_progress","phase":phase})
        );
    } else {
        eprintln!("{}", localizer.text(key));
    }
}

fn extension_response<T: serde::Serialize>(
    kind: &str,
    format: OutputFormat,
    confirmed_status: Option<&str>,
    result: Result<T, ExtensionError>,
    print: impl FnOnce(&T),
) -> ExitCode {
    match result {
        Ok(plan) => {
            if matches!(format, OutputFormat::Json) {
                let status = confirmed_status.unwrap_or("preview");
                println!(
                    "{}",
                    json!({"schema_version":1,"kind":kind,"status":status,"plan":plan})
                );
            } else {
                print(&plan);
            }
            ExitCode::SUCCESS
        }
        Err((details, message)) => {
            if matches!(format, OutputFormat::Json) {
                println!(
                    "{}",
                    json!({"schema_version":1,"kind":kind,"status":"error","error":details})
                );
            }
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn print_installation_plan(plan: &installation::Plan, localizer: &Localizer) {
    println!("{}", localizer.text("runtime-extension-install-about"));
    println!("{} ({})", plan.target, plan.platform_version);
    let label = localizer.text("runtime-apply-extension");
    println!("{label} {} ({})", plan.extension.name, plan.extension.uuid);
    let label = localizer.text("runtime-extension-dependency-generation");
    for (name, generation) in &plan.dependency_generations {
        println!("{label} {name} ({generation})");
    }
    for path in &plan.import_files {
        println!("  {}", path.display());
    }
    println!("--confirm {}", plan.confirmation);
}

fn print_removal_plan(plan: &removal::Plan, localizer: &Localizer) {
    println!("{} ({})", plan.target, plan.platform_version);
    let label = localizer.text("runtime-apply-extension");
    println!("{label} {} ({})", plan.extension.name, plan.extension.uuid);
    if let Some(generation) = &plan.extension_generation {
        println!(
            "{} {generation}",
            localizer.text("runtime-extension-generation")
        );
        if plan.pending_operation {
            println!("{}", localizer.text("runtime-apply-status-incomplete"));
        }
        println!("{}", localizer.text("runtime-extension-remove-data"));
    } else {
        println!("{}", localizer.text("runtime-extension-remove-finalize"));
    }
    println!("--confirm {} --force", plan.confirmation);
}

pub fn apply(args: &ApplyArgs, start: &Path, localizer: &Localizer) -> ExitCode {
    if let Some(name) = args.application.as_deref() {
        return apply_application(args, start, name, localizer);
    }
    if matches!(args.profile, ProfileArg::Prod) {
        return apply_failure(
            args,
            localizer,
            &json!({"code":"forbidden_action"}),
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
                &json!({"code":"project_discovery"}),
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
                print_apply_plan(&plan, localizer, true);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let (details, message) = present_apply_error(&error, localizer);
            let review = match &error {
                ApplyError::ReviewRequired(_, review) => Some(review.as_ref()),
                _ => None,
            };
            apply_failure(args, localizer, &details, &message, review)
        }
    }
}

fn apply_application(
    args: &ApplyArgs,
    start: &Path,
    name: &str,
    localizer: &Localizer,
) -> ExitCode {
    let profile = match args.profile {
        ProfileArg::Test => ProfileName::Test,
        ProfileArg::Prod => ProfileName::Prod,
    };
    let json_output = matches!(args.format, OutputFormat::Json);
    let failure = if let Some(token) = args.confirm.as_deref() {
        match application::execute(
            start,
            name,
            profile,
            args.initialize,
            token,
            |member, stage| {
                let (phase, key) = match stage {
                    Stage::Import => ("import", "runtime-apply-importing"),
                    _ => ("apply", "runtime-apply-applying"),
                };
                if json_output {
                    eprintln!(
                        "{}",
                        json!({"schema_version":1,"kind":"apply_progress","phase":phase,"component":member})
                    );
                } else {
                    eprintln!("{member}  {}", localizer.text(key));
                }
            },
        ) {
            Ok(applied) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"schema_version":1,"kind":"application_apply","status":"applied",
                        "launch":applied.launch,"outcomes":applied.outcomes})
                    );
                } else {
                    print_outcomes(&applied.outcomes, localizer);
                }
                return ExitCode::SUCCESS;
            }
            Err(failure) => failure,
        }
    } else {
        match application::preview(start, name, profile, args.initialize) {
            Ok(plan) => {
                if json_output {
                    println!(
                        "{}",
                        json!({"schema_version":1,"kind":"application_apply","status":"preview","plan":plan})
                    );
                } else {
                    for component in &plan.components {
                        println!("{}", component.member);
                        if let Some(plan) = &component.plan {
                            print_apply_plan(plan, localizer, false);
                        }
                        if let Some(blocked) = &component.review {
                            let (_, message) = review_error(blocked.reason);
                            println!(
                                "{}",
                                localizer.text(inspection_key(blocked.review.inspection.status))
                            );
                            if let Some(changes) = &blocked.review.changes {
                                for (path, change) in changes {
                                    let label = localizer.text(change_key(*change));
                                    println!("{label} {}", path.display());
                                }
                            }
                            println!("{}", localizer.text(message));
                        }
                    }
                    if let Some(token) = plan.confirmation {
                        println!("--confirm {token}");
                    }
                }
                return ExitCode::SUCCESS;
            }
            Err(failure) => failure,
        }
    };
    let (mut details, message) = present_apply_error(&failure.error, localizer);
    if json_output {
        details["component"] = json!(failure.member);
        println!(
            "{}",
            json!({"schema_version":1,"kind":"application_apply","status":"error",
            "error":details,"outcomes":failure.outcomes})
        );
    } else {
        print_outcomes(&failure.outcomes, localizer);
    }
    if let Some(member) = failure.member {
        eprintln!("{member}  {message}");
    } else {
        eprintln!("{message}");
    }
    ExitCode::FAILURE
}

fn print_outcomes(outcomes: &[ComponentOutcome], localizer: &Localizer) {
    for outcome in outcomes {
        let key = match outcome.status {
            ComponentStatus::Applied => "runtime-apply-done",
            ComponentStatus::Unchanged => "runtime-apply-unchanged",
            ComponentStatus::NotStarted => "runtime-apply-component-not-started",
            ComponentStatus::Failed => "runtime-apply-component-failed",
        };
        println!("{}  {}", outcome.member, localizer.text(key));
    }
}

fn print_apply_plan(plan: &Plan, localizer: &Localizer, show_confirmation: bool) {
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
    if let Some(extension) = &plan.extension {
        let label = localizer.text("runtime-apply-extension");
        println!("{label} {} ({})", extension.name, extension.uuid);
    }
    println!("{}", localizer.text(inspection_key(plan.inspection.status)));
    for path in &plan.picture_references {
        let label = localizer.text("runtime-apply-picture-reference");
        println!("{label} {}", path.display());
    }
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
    if show_confirmation && let Some(confirmation) = &plan.confirmation {
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
            ObjectKind::Role => "diff-metadata-role",
            ObjectKind::CommonModule => "diff-metadata-common-module",
            ObjectKind::CommonPicture => "diff-metadata-common-picture",
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
    details: &serde_json::Value,
    message: &str,
    review: Option<&Review>,
) -> ExitCode {
    if matches!(args.format, OutputFormat::Json) {
        let mut response =
            json!({"schema_version":1,"kind":"apply","status":"error","error":details});
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

fn present_apply_error(error: &ApplyError, localizer: &Localizer) -> (serde_json::Value, String) {
    let (code, key) = apply_error(error);
    if let ApplyError::ExtensionNotInstalled(name)
    | ApplyError::ExtensionAlreadyInstalled(name)
    | ApplyError::ExtensionUuidInUse(name)
    | ApplyError::ExtensionInstallationIncomplete(name) = error
    {
        return (
            json!({"code":code,"extension":name}),
            localizer.format(key, &[("name", LocalizationValue::Text(name))]),
        );
    }
    if let ApplyError::ExtensionHasDependent(name) = error {
        return (
            json!({"code":code,"dependent":name}),
            localizer.format(key, &[("name", LocalizationValue::Text(name))]),
        );
    }
    (json!({"code":code}), localizer.text(key))
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
        ApplyError::Runtime(RuntimeError::Settings(SettingsError::ForbiddenAction)) => {
            ("forbidden_action", "project-settings-forbidden")
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
        ApplyError::ReviewRequired(reason, _) => review_error(*reason),
        ApplyError::Busy => ("busy", "runtime-apply-busy"),
        ApplyError::StalePreview => ("stale_preview", "runtime-apply-stale"),
        ApplyError::ProjectType => ("project_type_unsupported", "runtime-apply-project-type"),
        ApplyError::ExtensionApplication => (
            "extension_application_required",
            "runtime-apply-extension-application",
        ),
        ApplyError::ApplicationContext => {
            ("application_required", "runtime-apply-application-required")
        }
        ApplyError::ExtensionScope => (
            "extension_scope_unsupported",
            "runtime-apply-extension-scope",
        ),
        ApplyError::ExtensionDescriptor => (
            "extension_descriptor_invalid",
            "runtime-apply-extension-descriptor",
        ),
        ApplyError::ExtensionAmbiguous => {
            ("extension_ambiguous", "runtime-apply-extension-ambiguous")
        }
        ApplyError::ExtensionUnavailable => (
            "extension_unavailable",
            "runtime-apply-extension-unavailable",
        ),
        ApplyError::ExtensionNotInstalled(_) => (
            "extension_not_installed",
            "runtime-apply-extension-not-installed",
        ),
        ApplyError::ExtensionAlreadyInstalled(_) => (
            "extension_already_installed",
            "runtime-extension-already-installed",
        ),
        ApplyError::ExtensionUuidInUse(_) => {
            ("extension_uuid_in_use", "runtime-extension-uuid-in-use")
        }
        ApplyError::ExtensionInstallationIncomplete(_) => (
            "extension_installation_incomplete",
            "runtime-extension-installation-incomplete",
        ),
        ApplyError::ExtensionIdentityMismatch => (
            "extension_identity_mismatch",
            "runtime-apply-extension-identity",
        ),
        ApplyError::ExtensionHasDependent(_) => {
            ("extension_has_dependent", "runtime-extension-has-dependent")
        }
        ApplyError::ForceRequired => ("force_required", "runtime-extension-force-required"),
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
        ApplyError::PlatformFailed(Stage::Remove) => {
            ("remove_failed", "runtime-extension-remove-failed")
        }
        ApplyError::Interrupted => ("cancelled", "component-cancelled"),
        _ => ("invalid_state", "runtime-apply-state-error"),
    }
}

const fn review_error(reason: ReviewReason) -> (&'static str, &'static str) {
    match reason {
        ReviewReason::BaselineRequired => ("baseline_required", "runtime-apply-baseline-required"),
        ReviewReason::BaselineChanged => ("baseline_changed", "runtime-apply-baseline-changed"),
        ReviewReason::Incomplete => ("incomplete_apply", "runtime-apply-incomplete"),
        ReviewReason::GenerationUnavailable => {
            ("generation_failed", "runtime-apply-inspect-failed")
        }
        ReviewReason::InvalidBaseline | ReviewReason::InvalidGeneration => {
            ("invalid_state", "runtime-apply-state-error")
        }
    }
}
