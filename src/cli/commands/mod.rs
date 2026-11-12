//! Register shared command arguments and route each invocation to its component.

use crate::component::{self, Component, Invocation};
use std::{ffi::OsString, process::ExitCode};

use clap::Subcommand;
use eska_presentation::arguments::{
    build::{self, clean, config, import, patch, platform, runtime},
    check, formatting,
    project::{add, doctor, init, metadata, new, version},
    vcs::{clone, diff, finish, history, save, shelves, start, status, switch},
};

use super::localization::Localizer;

mod ide;
mod update;

#[derive(Debug, Subcommand)]
pub(super) enum Commands {
    #[command(disable_help_flag = true)]
    Add(add::AddArgs),
    #[command(disable_help_flag = true)]
    Update(update::UpdateArgs),
    #[command(disable_help_flag = true)]
    Metadata(metadata::MetadataArgs),
    #[command(disable_help_flag = true)]
    Ide(ide::IdeArgs),
    #[command(disable_help_flag = true)]
    Build(build::BuildArgs),
    #[command(disable_help_flag = true)]
    Run(runtime::RunArgs),
    #[command(disable_help_flag = true)]
    Apply(runtime::ApplyArgs),
    #[command(disable_help_flag = true)]
    Extension(runtime::ExtensionArgs),
    #[command(disable_help_flag = true)]
    Check(check::CheckArgs),
    #[command(name = "fmt", disable_help_flag = true)]
    Fmt(formatting::FmtArgs),
    #[command(disable_help_flag = true)]
    Clean(clean::CleanArgs),
    #[command(disable_help_flag = true)]
    Clone(clone::CloneArgs),
    #[command(disable_help_flag = true)]
    Config(config::ConfigArgs),
    #[command(disable_help_flag = true)]
    New(new::NewArgs),
    #[command(disable_help_flag = true)]
    Init(init::InitArgs),
    #[command(disable_help_flag = true)]
    Import(import::ImportArgs),
    #[command(disable_help_flag = true)]
    Platform(platform::PlatformArgs),
    #[command(disable_help_flag = true)]
    Patch(patch::PatchArgs),
    #[command(disable_help_flag = true)]
    Diff(diff::DiffArgs),
    #[command(disable_help_flag = true)]
    Doctor(doctor::DoctorArgs),
    #[command(disable_help_flag = true)]
    Finish(finish::FinishArgs),
    #[command(disable_help_flag = true)]
    History(history::HistoryArgs),
    #[command(disable_help_flag = true)]
    Save(save::SaveArgs),
    #[command(disable_help_flag = true)]
    Start(start::StartArgs),
    #[command(disable_help_flag = true)]
    Status(status::StatusArgs),
    #[command(disable_help_flag = true)]
    Switch(switch::SwitchArgs),
    #[command(disable_help_flag = true)]
    Shelve(shelves::ShelveArgs),
    #[command(disable_help_flag = true)]
    Unshelve(shelves::UnshelveArgs),
    #[command(disable_help_flag = true)]
    Shelves(shelves::ShelvesArgs),
    #[command(disable_help_flag = true)]
    Version(version::VersionArgs),
}

/// Routes the parsed intent while preserving the original native process arguments.
pub(super) fn run(
    command: Option<&Commands>,
    localizer: &Localizer,
    process_args: &[OsString],
) -> ExitCode {
    let (component, invocation) = match command {
        Some(Commands::Update(args)) => return args.run(localizer),
        Some(Commands::Check(_)) => (Component::Check, Invocation::OmitSubcommand),
        Some(Commands::Fmt(_)) => (Component::Fmt, Invocation::OmitSubcommand),
        Some(
            Commands::Add(_) | Commands::Metadata(_) | Commands::Version(_) | Commands::Doctor(_),
        ) => (Component::Project, Invocation::Direct),
        Some(Commands::Ide(_)) => (Component::Project, Invocation::LegacyIde),
        Some(
            Commands::Shelve(_)
            | Commands::Unshelve(_)
            | Commands::Shelves(_)
            | Commands::Clone(_)
            | Commands::Diff(_)
            | Commands::Finish(_)
            | Commands::History(_)
            | Commands::Save(_)
            | Commands::Start(_)
            | Commands::Status(_)
            | Commands::Switch(_),
        ) => (Component::Vcs, Invocation::Direct),
        Some(
            Commands::Clean(_)
            | Commands::Config(_)
            | Commands::Platform(_)
            | Commands::Import(_)
            | Commands::Patch(_)
            | Commands::Run(_)
            | Commands::Apply(_)
            | Commands::Extension(_),
        ) => (Component::Build, Invocation::Direct),
        Some(Commands::Build(args)) => (
            Component::Build,
            Invocation::Build {
                provenance: args.needs_provenance(),
            },
        ),
        Some(Commands::New(_)) => (Component::Project, Invocation::Rename("__shell-new")),
        Some(Commands::Init(_)) => (Component::Project, Invocation::Rename("__shell-init")),
        None => (Component::Project, Invocation::LegacyValidate),
    };
    component::run(component, process_args, invocation, localizer)
}

pub(super) fn localize(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .mut_subcommand("add", |command| add::localize(command, localizer))
        .mut_subcommand("metadata", |command| metadata::localize(command, localizer))
        .mut_subcommand("update", |command| update::localize(command, localizer))
        .mut_subcommand("ide", |command| ide::localize(command, localizer))
        .mut_subcommand("build", |command| build::localize(command, localizer))
        .mut_subcommand("run", |command| runtime::localize(command, localizer))
        .mut_subcommand("apply", |command| {
            runtime::localize_apply(command, localizer)
        })
        .mut_subcommand("extension", |command| {
            runtime::localize_extension(command, localizer)
        })
        .mut_subcommand("check", |command| check::localize(command, localizer))
        .mut_subcommand("fmt", |command| formatting::localize(command, localizer))
        .mut_subcommand("clean", |command| clean::localize(command, localizer))
        .mut_subcommand("clone", |command| clone::localize(command, localizer))
        .mut_subcommand("config", |command| config::localize(command, localizer))
        .mut_subcommand("new", |command| new::localize(command, localizer))
        .mut_subcommand("init", |command| init::localize(command, localizer))
        .mut_subcommand("import", |command| import::localize(command, localizer))
        .mut_subcommand("platform", |command| platform::localize(command, localizer))
        .mut_subcommand("patch", |command| patch::localize(command, localizer))
        .mut_subcommand("diff", |command| diff::localize(command, localizer))
        .mut_subcommand("doctor", |command| doctor::localize(command, localizer))
        .mut_subcommand("finish", |command| finish::localize(command, localizer))
        .mut_subcommand("history", |command| history::localize(command, localizer))
        .mut_subcommand("save", |command| save::localize(command, localizer))
        .mut_subcommand("start", |command| start::localize(command, localizer))
        .mut_subcommand("status", |command| status::localize(command, localizer))
        .mut_subcommand("switch", |command| switch::localize(command, localizer))
        .mut_subcommand("shelve", |command| shelves::localize(command, localizer))
        .mut_subcommand("unshelve", |command| shelves::localize(command, localizer))
        .mut_subcommand("shelves", |command| shelves::localize(command, localizer))
        .mut_subcommand("version", |command| version::localize(command, localizer))
}
