use clap::{ArgAction, Args, Subcommand, ValueEnum};

use crate::localization::Localizer;

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(long, value_enum, default_value = "test")]
    pub profile: ProfileArg,
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,
    #[arg(long, hide = true)]
    pub expected_infobase: Option<String>,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(long, value_enum, default_value = "test")]
    pub profile: ProfileArg,
    #[arg(long)]
    pub application: Option<String>,
    #[arg(long)]
    pub initialize: bool,
    #[arg(long)]
    pub confirm: Option<String>,
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Debug, Args)]
pub struct ExtensionArgs {
    #[command(subcommand)]
    pub command: ExtensionCommand,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Debug, Subcommand)]
pub enum ExtensionCommand {
    #[command(disable_help_flag = true)]
    Remove(RemoveExtensionArgs),
}

#[derive(Debug, Args)]
pub struct RemoveExtensionArgs {
    #[arg(long, value_enum, default_value = "test")]
    pub profile: ProfileArg,
    #[arg(long)]
    pub confirm: Option<String>,
    #[arg(long, requires = "confirm")]
    pub force: bool,
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ProfileArg {
    Test,
    Prod,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
}

pub fn localize(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .about(localizer.text("runtime-run-about"))
        .mut_arg("profile", |arg| {
            arg.help(localizer.text("runtime-profile-help"))
        })
        .mut_arg("format", |arg| {
            arg.help(localizer.text("version-format-help"))
        })
        .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
}

pub fn localize_apply(command: clap::Command, localizer: &Localizer) -> clap::Command {
    localize(command, localizer)
        .about(localizer.text("runtime-apply-about"))
        .mut_arg("application", |arg| {
            arg.help(localizer.text("runtime-apply-application-help"))
        })
        .mut_arg("initialize", |arg| {
            arg.help(localizer.text("runtime-apply-initialize-help"))
        })
        .mut_arg("confirm", |arg| {
            arg.help(localizer.text("runtime-apply-confirm-help"))
        })
}

pub fn localize_extension(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .about(localizer.text("runtime-extension-about"))
        .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        .mut_subcommand("remove", |command| {
            localize(command, localizer)
                .about(localizer.text("runtime-extension-remove-about"))
                .mut_arg("confirm", |arg| {
                    arg.help(localizer.text("runtime-apply-confirm-help"))
                })
                .mut_arg("force", |arg| {
                    arg.help(localizer.text("runtime-extension-remove-force"))
                })
        })
}
