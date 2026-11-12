//! 1C debugger entry point and production source policy.

mod dap;
mod dbgs;
mod http;
#[cfg(test)]
mod live;
mod processing;
mod session;
mod source;
mod standalone;
mod state;

use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use clap::{ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use eska_build_runtime::runtime::{self, DebugConnection, DebugLaunch};
use eska_presentation::{
    arguments::bootstrap_lang,
    localization::{Localizer, resolve_locale_from_environment},
};
use eska_project_core::project::{
    Project, discovery,
    local_settings::{Action, Profile, ProfileName},
};
use eska_tool_runtime::Component;
use serde_json::json;

#[derive(Debug, Parser)]
#[command(version, disable_help_flag = true, disable_help_subcommand = true)]
struct Cli {
    #[arg(long, global = true)]
    lang: Option<String>,
    #[arg(long, global = true, default_value = ".")]
    project_dir: PathBuf,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
    #[command(subcommand)]
    command: Operation,
}

#[derive(Debug, Subcommand)]
enum Operation {
    #[command(disable_help_flag = true)]
    Check(CheckArgs),
    #[command(disable_help_flag = true)]
    Dap(DapArgs),
    #[command(disable_help_flag = true)]
    Standalone(StandaloneArgs),
}

#[derive(Debug, Args)]
struct StandaloneArgs {
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Debug, Args)]
struct CheckArgs {
    #[arg(long)]
    unsaved: bool,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Debug, Args)]
struct DapArgs {
    #[arg(long, value_enum)]
    profile: ProfileArg,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, value_enum)]
    sources: Option<SourceChoice>,
    #[arg(long)]
    unsaved: bool,
    #[arg(long, hide = true)]
    expected_infobase: Option<String>,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProfileArg {
    Test,
    Prod,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Mode {
    Launch,
    Attach,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SourceChoice {
    Current,
    FastForward,
    SwitchMain,
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let language = resolve_locale_from_environment(bootstrap_lang(&args).as_deref());
    let Ok(localizer) = Localizer::try_new(language) else {
        return ExitCode::FAILURE;
    };
    let parsed = localized(&localizer).get_matches_from(args);
    let cli = Cli::from_arg_matches(&parsed).unwrap_or_else(|error| error.exit());
    if matches!(cli.command, Operation::Standalone(_)) {
        return if session::run(None, &localizer).is_ok() {
            ExitCode::SUCCESS
        } else {
            eprintln!("{}", localizer.text("debug-adapter-disconnected"));
            ExitCode::FAILURE
        };
    }
    let Ok(project) = discovery::discover(&cli.project_dir) else {
        eprintln!("{}", localizer.text("debug-project-unavailable"));
        return ExitCode::FAILURE;
    };
    match cli.command {
        Operation::Check(args) => {
            let result = source_status(&project, args.unsaved);
            println!(
                "{}",
                json!({"schema_version":1,"kind":"prod-source-check","check":result})
            );
            ExitCode::SUCCESS
        }
        Operation::Dap(args) => match serve(project, &args, &localizer) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        },
        Operation::Standalone(_) => ExitCode::FAILURE,
    }
}

fn serve(mut project: Project, args: &DapArgs, localizer: &Localizer) -> Result<(), String> {
    let profile = match args.profile {
        ProfileArg::Test => ProfileName::Test,
        ProfileArg::Prod => ProfileName::Prod,
    };
    let action = if args.mode == Mode::Launch {
        Action::DebugLaunch
    } else {
        Action::DebugAttach
    };
    let production = profile == ProfileName::Prod;
    if args.expected_infobase.is_some() && (production || args.mode != Mode::Launch) {
        return Err(localizer.text("project-settings-forbidden"));
    }
    if production {
        runtime::profile(&project, profile, action)
            .map_err(|error| runtime_error(&error, localizer))?;
        check_sources(&project, args).map_err(|key| localizer.text(key))?;
        if matches!(
            args.sources,
            Some(SourceChoice::FastForward | SourceChoice::SwitchMain)
        ) {
            project = discovery::discover(project.root())
                .map_err(|_| localizer.text("debug-project-unavailable"))?;
        }
    }
    let mut sources = source::Sources::load(&project).map_err(|error| {
        localizer.text(match error {
            source::SourceError::Project => "debug-project-unavailable",
            source::SourceError::ApplicationChoice => "debug-application-choice",
            source::SourceError::DuplicateExtension => "debug-duplicate-extension",
        })
    })?;
    let mut launch = if args.mode == Mode::Launch {
        Some(
            DebugLaunch::prepare(&project, profile, args.expected_infobase.as_deref())
                .map_err(|error| runtime_error(&error, localizer))?,
        )
    } else {
        None
    };
    let settings = match launch.as_ref() {
        Some(launch) => launch.profile().clone(),
        None => runtime::profile(&project, profile, action)
            .map_err(|error| runtime_error(&error, localizer))?,
    };
    let (alias, auto_attach_types) = connection_settings(&settings)
        .ok_or_else(|| localizer.text("debug-endpoint-unavailable"))?;
    let connection = launch
        .as_ref()
        .map_or_else(
            || DebugConnection::prepare(&project, &settings),
            DebugLaunch::connection,
        )
        .map_err(|error| runtime_error(&error, localizer))?;
    let mut attach_selection = None;
    if let Some(processing) = settings
        .debug
        .as_ref()
        .and_then(|debug| debug.external_processing.as_ref())
    {
        processing::ExternalProcessing::from_project(&project, processing)
            .and_then(|processing| processing.configure(&mut sources, launch.as_mut()))
            .map_err(|key| localizer.text(key))?;
        if args.mode == Mode::Attach {
            attach_selection = Some(session::AttachSelection::MatchingTypes);
        }
    }
    session::run(
        Some(session::Config {
            connection,
            alias,
            auto_attach_types,
            sources,
            mode: args.mode,
            launch,
            production,
            attach_selection,
        }),
        localizer,
    )
    .map_err(|_| localizer.text("debug-adapter-disconnected"))
}

fn check_sources(project: &Project, args: &DapArgs) -> Result<(), &'static str> {
    match args.sources {
        Some(SourceChoice::Current) => Ok(()),
        Some(choice @ (SourceChoice::FastForward | SourceChoice::SwitchMain)) => {
            let options = source_options(args.unsaved);
            let mut options = options.into_iter().collect::<Vec<_>>();
            options.push(
                match choice {
                    SourceChoice::FastForward => "fast-forward",
                    _ => "switch-main",
                }
                .into(),
            );
            let binary =
                eska_tool_runtime::resolve(Component::Vcs).map_err(|_| "debug-update-failed")?;
            let result = eska_tool_runtime::query(
                Component::Vcs,
                &binary,
                "debug_source_update",
                project.root(),
                &options,
            )
            .map_err(|_| "debug-update-failed")?;
            if result["updated"] == true {
                Ok(())
            } else {
                Err("debug-update-failed")
            }
        }
        None if source_status(project, args.unsaved)["status"] == "current" => Ok(()),
        None => Err("debug-source-choice-required"),
    }
}

fn source_status(project: &Project, unsaved: bool) -> serde_json::Value {
    eska_tool_runtime::resolve(Component::Vcs)
        .and_then(|binary| {
            eska_tool_runtime::query(
                Component::Vcs,
                &binary,
                "debug_source_check",
                project.root(),
                &source_options(unsaved).into_iter().collect::<Vec<_>>(),
            )
        })
        .unwrap_or_else(|_| {
            json!({"status":"unverified","reason":"repository","main_branch":null,
            "remote":null,"active_branch":null,"local_main_ahead":null,"local_main_behind":null,
            "active_ahead":null,"active_behind":null,"dirty":null,"unsaved":unsaved,"update":null})
        })
}

fn source_options(unsaved: bool) -> Option<OsString> {
    unsaved.then(|| "--unsaved".into())
}

fn connection_settings(profile: &Profile) -> Option<(String, Vec<String>)> {
    use eska_project_core::project::local_settings::Infobase;
    let debug = profile.debug.as_ref()?;
    let alias = match &profile.infobase {
        Infobase::File { .. } => "DefAlias".to_owned(),
        Infobase::Cluster { name, .. } | Infobase::Standalone { name, .. } => name.clone(),
    };
    Some((alias, debug.auto_attach_types()))
}

fn runtime_error(error: &runtime::RuntimeError, localizer: &Localizer) -> String {
    localizer.text(runtime_failure(error))
}

const fn runtime_failure(error: &runtime::RuntimeError) -> &'static str {
    use eska_project_core::project::local_settings::SettingsError;
    match error {
        runtime::RuntimeError::Settings(SettingsError::MissingProfile(_)) => {
            "project-settings-missing-profile"
        }
        runtime::RuntimeError::Settings(SettingsError::DebugEndpointRequired(_)) => {
            "project-settings-missing-debug"
        }
        runtime::RuntimeError::Settings(_) => "project-settings-invalid",
        runtime::RuntimeError::Platform(_) | runtime::RuntimeError::PlatformUnavailable => {
            "runtime-platform-unavailable"
        }
        runtime::RuntimeError::FileInfobaseUnavailable => "runtime-infobase-unavailable",
        runtime::RuntimeError::SecretUnavailable => "runtime-secret-unavailable",
        runtime::RuntimeError::EndpointUnavailable => "debug-endpoint-unavailable",
        runtime::RuntimeError::LaunchTargetChanged => "runtime-launch-target-changed",
        _ => "runtime-client-failed",
    }
}

fn localized(localizer: &Localizer) -> clap::Command {
    Cli::command()
        .about(localizer.text("debug-tool-about"))
        .mut_arg("lang", |arg| arg.help(localizer.text("cli-lang-help")))
        .mut_arg("project_dir", |arg| {
            arg.help(localizer.text("cli-project-dir-help"))
        })
        .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        .mut_subcommand("check", |command| {
            command
                .about(localizer.text("debug-check-about"))
                .mut_arg("unsaved", |arg| {
                    arg.help(localizer.text("debug-unsaved-help"))
                })
                .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        })
        .mut_subcommand("dap", |command| {
            command
                .about(localizer.text("debug-dap-about"))
                .mut_arg("profile", |arg| {
                    arg.help(localizer.text("runtime-profile-help"))
                })
                .mut_arg("mode", |arg| arg.help(localizer.text("debug-mode-help")))
                .mut_arg("sources", |arg| {
                    arg.help(localizer.text("debug-sources-help"))
                })
                .mut_arg("unsaved", |arg| {
                    arg.help(localizer.text("debug-unsaved-help"))
                })
                .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        })
        .mut_subcommand("standalone", |command| {
            command
                .about(localizer.text("debug-standalone-about"))
                .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        })
}
