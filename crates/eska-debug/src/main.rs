//! Native launch and source policy around an independently installed stdio DAP adapter.

mod dap;

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

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
use eska_vcs::project::debug_sources::{self, Status, Update};
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
    #[arg(long)]
    adapter: PathBuf,
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
            let result = debug_sources::check(&project, args.unsaved);
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
    let executable = args
        .adapter
        .canonicalize()
        .ok()
        .filter(|path| path.is_file())
        .ok_or_else(|| localizer.text("debug-adapter-unavailable"))?;
    let action = if args.mode == Mode::Launch {
        Action::DebugLaunch
    } else {
        Action::DebugAttach
    };
    let warning = (profile == ProfileName::Prod).then(|| localizer.text("debug-prod-warning"));
    if profile == ProfileName::Prod {
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
    let root = project.root().to_owned();
    let source = project.source().to_owned();
    let (config, launch) = if args.mode == Mode::Launch {
        let launch = DebugLaunch::prepare(project, profile)
            .map_err(|error| runtime_error(&error, localizer))?;
        let config = connection(launch.profile(), &root, &source)
            .ok_or_else(|| localizer.text("debug-endpoint-unavailable"))?;
        (config, Some(launch))
    } else {
        let profile = runtime::profile(&project, profile, action)
            .map_err(|error| runtime_error(&error, localizer))?;
        let config = connection(&profile, &root, &source)
            .ok_or_else(|| localizer.text("debug-endpoint-unavailable"))?;
        (config, None)
    };
    dap::run(&executable, config, args.mode, launch, warning, localizer)
        .map_err(|_| localizer.text("debug-adapter-disconnected"))
}

fn check_sources(project: &Project, args: &DapArgs) -> Result<(), &'static str> {
    match args.sources {
        Some(SourceChoice::FastForward | SourceChoice::SwitchMain) => {
            let update = if matches!(args.sources, Some(SourceChoice::SwitchMain)) {
                Update::SwitchMain
            } else {
                Update::FastForward
            };
            debug_sources::update(project, args.unsaved, update).map_err(|_| "debug-update-failed")
        }
        choice => {
            let check = debug_sources::check(project, args.unsaved);
            if check.status == Status::Current || matches!(choice, Some(SourceChoice::Current)) {
                Ok(())
            } else {
                Err("debug-source-choice-required")
            }
        }
    }
}

fn connection(profile: &Profile, root: &Path, source: &Path) -> Option<serde_json::Value> {
    use eska_project_core::project::local_settings::Infobase;
    let endpoint = url::Url::parse(&profile.debug.as_ref()?.url).ok()?;
    let connection = match &profile.infobase {
        Infobase::File { path } => format!("/F{}", root.join(path).to_str()?),
        Infobase::Cluster { host, name } => format!("/S{host}\\{name}"),
        // The adapter's attach uses only the suffix as the platform's debug alias.
        Infobase::Standalone { name, .. } => format!("/Slocalhost\\{name}"),
    };
    Some(json!({
        "request": "attach",
        "connectionString": connection,
        "rootProject": source.to_str()?,
        "debugServerHost": endpoint.host_str()?,
        "debugServerPort": endpoint.port_or_known_default()?,
        "autoAttachTypes": ["ManagedClient", "Client", "Server", "ServerEmulation"],
    }))
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
                .mut_arg("adapter", |arg| {
                    arg.help(localizer.text("debug-adapter-help"))
                })
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
