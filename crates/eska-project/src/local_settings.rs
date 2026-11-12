//! CLI entry point for editing the shared project-local settings.

use std::{
    io::{self, Read},
    path::Path,
    process::ExitCode,
};

use clap::{ArgAction, Args, Subcommand};
use eska_presentation::{diagnostics, localization::Localizer};
use eska_project_core::project::{
    discovery,
    local_settings::{self, LocalSettings, ProfileName, SETTINGS_PATH, SettingsError, Snapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{cli::OutputFormat, response::Failure};

#[derive(Debug, Args)]
pub struct SettingsArgs {
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    #[command(subcommand)]
    action: SettingsCommand,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    Show,
    Save,
    Validate,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum GitProtection {
    NoRepository,
    Ignored,
    Tracked,
    Exposed,
    Unknown,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    revision: Option<String>,
    settings: LocalSettings,
}

pub fn run(start: &Path, args: &SettingsArgs, localizer: &Localizer) -> ExitCode {
    let project = match discovery::discover(start) {
        Ok(project) => project,
        Err(error) => {
            return Failure::new(
                "project_discovery",
                json!({}),
                diagnostics::present_project_error(&error, localizer),
            )
            .emit(args.format);
        }
    };
    let root = project.root();
    let protection = git_protection(root);
    let snapshot = match args.action {
        SettingsCommand::Show => local_settings::load(root),
        SettingsCommand::Validate => local_settings::load(root).and_then(|snapshot| {
            snapshot.settings.validate()?;
            Ok(snapshot)
        }),
        SettingsCommand::Save => {
            if let Some((code, key)) = protection.warning() {
                return Failure::new(code, json!({}), localizer.text(key)).emit(args.format);
            }
            let input = match read_update() {
                Ok(input) => input,
                Err(error) => return failure(&error, localizer).emit(args.format),
            };
            local_settings::save(root, input.revision.as_deref(), &input.settings)
        }
    };
    match snapshot {
        Ok(snapshot) => {
            if let Some((_, key)) = protection.warning() {
                eprintln!("{}", localizer.text(key));
            }
            output(&snapshot, protection, args.format, localizer);
            ExitCode::SUCCESS
        }
        Err(error) => failure(&error, localizer).emit(args.format),
    }
}

fn read_update() -> Result<Update, SettingsError> {
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

fn output(
    snapshot: &Snapshot,
    protection: GitProtection,
    format: OutputFormat,
    localizer: &Localizer,
) {
    match format {
        OutputFormat::Human => println!("{}", localizer.text("project-settings-ready")),
        OutputFormat::Json => println!(
            "{}",
            json!({
                "schema_version": 1, "kind": "project-local-settings", "status": "ready",
                "revision": snapshot.revision, "settings": snapshot.settings, "git_protection": protection,
            })
        ),
    }
}

impl GitProtection {
    const fn warning(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::NoRepository | Self::Ignored => None,
            Self::Tracked => Some(("settings_tracked", "project-settings-tracked")),
            Self::Exposed => Some(("settings_exposed", "project-settings-exposed")),
            Self::Unknown => Some(("settings_git_unknown", "project-settings-git-unknown")),
        }
    }
}

fn git_protection(root: &Path) -> GitProtection {
    let repository = match gix::discover(root) {
        Ok(repository) => repository,
        Err(error) if error.classify().has(gix::error::Class::NotFound) => {
            return GitProtection::NoRepository;
        }
        Err(_) => return GitProtection::Unknown,
    };
    let Some(relative) = repository
        .workdir()
        .and_then(|worktree| root.strip_prefix(worktree).ok())
        .map(|path| path.join(SETTINGS_PATH))
    else {
        return GitProtection::Unknown;
    };
    check_index(&repository, &relative).unwrap_or(GitProtection::Unknown)
}

fn check_index(repository: &gix::Repository, path: &Path) -> Option<GitProtection> {
    let index = repository.index_or_empty().ok()?;
    let relative = gix::path::into_bstr(path).ok()?;
    let normalized = gix::path::to_unix_separators_on_windows(relative);
    if index.prefixed_entries(&normalized).is_some_and(|entries| {
        entries
            .iter()
            .any(|entry| entry.path(&index) == normalized.as_ref())
    }) {
        return Some(GitProtection::Tracked);
    }
    let mut excludes = repository
        .excludes(
            &index,
            None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )
        .ok()?;
    let ignored = excludes.at_path(path, None).ok()?.is_excluded();
    Some(if ignored {
        GitProtection::Ignored
    } else {
        GitProtection::Exposed
    })
}

fn failure(error: &SettingsError, localizer: &Localizer) -> Failure {
    let (code, key, data) = match error {
        SettingsError::Io(_) => ("settings_io", "project-settings-io", json!({})),
        SettingsError::InvalidFile => ("invalid_settings", "project-settings-invalid", json!({})),
        SettingsError::TooLarge => (
            "settings_too_large",
            "project-settings-too-large",
            json!({}),
        ),
        SettingsError::InvalidField { profile, field } => (
            "invalid_field",
            "project-settings-invalid-field",
            json!({"profile": profile, "field": field}),
        ),
        SettingsError::MissingProfile(profile) => (
            "missing_profile",
            "project-settings-missing-profile",
            json!({"profile": profile}),
        ),
        SettingsError::DebugEndpointRequired(profile) => (
            "missing_debug_endpoint",
            "project-settings-missing-debug",
            json!({"profile": profile}),
        ),
        SettingsError::AdminEndpointRequired => (
            "missing_admin_endpoint",
            "project-settings-missing-admin",
            json!({}),
        ),
        SettingsError::ForbiddenAction => (
            "forbidden_action",
            "project-settings-forbidden",
            json!({"profile": ProfileName::Prod}),
        ),
        SettingsError::ClusterApplyUnsupported => (
            "cluster_apply_unsupported",
            "project-settings-cluster-apply",
            json!({}),
        ),
        SettingsError::SharedInfobase => (
            "shared_infobase",
            "project-settings-shared-infobase",
            json!({}),
        ),
        SettingsError::Changed => ("settings_changed", "project-settings-changed", json!({})),
    };
    let message = if let SettingsError::InvalidField { profile, field } = error {
        let profile = match profile {
            ProfileName::Test => "test",
            ProfileName::Prod => "prod",
        };
        localizer.format(
            key,
            &[
                (
                    "profile",
                    eska_presentation::localization::LocalizationValue::Text(profile),
                ),
                (
                    "field",
                    eska_presentation::localization::LocalizationValue::Text(field),
                ),
            ],
        )
    } else {
        localizer.text(key)
    };
    Failure::new(code, data, message)
}

pub fn localize(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .about(localizer.text("project-settings-about"))
        .mut_arg("format", |arg| {
            arg.help(localizer.text("version-format-help"))
                .value_name(localizer.text("version-format-value"))
        })
        .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        .mut_subcommand("show", |command| {
            command.about(localizer.text("project-settings-show"))
        })
        .mut_subcommand("save", |command| {
            command.about(localizer.text("project-settings-save"))
        })
        .mut_subcommand("validate", |command| {
            command.about(localizer.text("project-settings-validate"))
        })
}
