//! 1C debugger entry point and production source policy.

mod dap;
mod dbgs;
mod http;
#[cfg(test)]
mod live;
mod session;
mod source;
mod state;

use std::{ffi::OsString, path::PathBuf, process::ExitCode};

use clap::{ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use eska_build_runtime::runtime::{self, DebugLaunch};
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
    let sources =
        source::Sources::load(&project).map_err(|_| localizer.text("debug-project-unavailable"))?;
    let settings = runtime::profile(&project, profile, action)
        .map_err(|error| runtime_error(&error, localizer))?;
    let (endpoint, alias) =
        connection(&settings).ok_or_else(|| localizer.text("debug-endpoint-unavailable"))?;
    let launch = if args.mode == Mode::Launch {
        Some(
            DebugLaunch::prepare(project, profile)
                .map_err(|error| runtime_error(&error, localizer))?,
        )
    } else {
        None
    };
    session::run(
        session::Config {
            endpoint,
            alias,
            sources,
            mode: args.mode,
            launch,
            production,
        },
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

fn connection(profile: &Profile) -> Option<(url::Url, String)> {
    use eska_project_core::project::local_settings::Infobase;
    let endpoint = url::Url::parse(&profile.debug.as_ref()?.url).ok()?;
    let alias = match &profile.infobase {
        Infobase::File { .. } => "DefAlias".to_owned(),
        Infobase::Cluster { name, .. } | Infobase::Standalone { name, .. } => name.clone(),
    };
    Some((endpoint, alias))
}

fn runtime_error(error: &runtime::RuntimeError, localizer: &Localizer) -> String {
    use eska_project_core::project::local_settings::SettingsError;
    localizer.text(match error {
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
        _ => "runtime-client-failed",
    })
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
}
