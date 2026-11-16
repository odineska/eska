//! Semantic creation arguments; validation and XML ownership belong to the project core.

use std::path::PathBuf;

use clap::{Args, ValueEnum};

use crate::localization::Localizer;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CreationKind {
    Attribute,
    TablePart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum DateFractions {
    Date,
    Time,
    DateTime,
}

#[derive(Debug, Args)]
pub struct CreationSchemaArgs {
    #[arg(long)]
    pub owner: Option<String>,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

#[derive(Clone, Debug, Args)]
pub struct AddArgs {
    #[arg(value_enum)]
    pub kind: Option<CreationKind>,
    #[arg(long)]
    pub owner: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[command(flatten)]
    pub type_args: CreationTypeArgs,
    #[arg(long, value_name = "LANGUAGE=TEXT")]
    pub synonym: Vec<String>,
    #[arg(long)]
    pub comment: Option<String>,
    #[arg(long)]
    pub snapshot: Option<String>,
    #[arg(long, conflicts_with_all = ["kind", "owner", "name", "value_type", "reference", "length", "digits", "fraction_digits", "nonnegative", "fixed_length", "date_fractions", "synonym", "comment", "snapshot", "interactive", "plan"])]
    pub input: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["kind", "owner", "name", "value_type", "reference", "length", "digits", "fraction_digits", "nonnegative", "fixed_length", "date_fractions", "synonym", "comment", "snapshot", "interactive", "input"])]
    pub plan: Option<PathBuf>,
    #[arg(long, conflicts_with = "no_input")]
    pub interactive: bool,
    #[arg(long)]
    pub no_input: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

/// Type flags are flattened for users but grouped by their common semantic purpose.
#[derive(Clone, Debug, Args)]
pub struct CreationTypeArgs {
    #[arg(long = "type")]
    pub value_type: Option<String>,
    #[arg(long)]
    pub reference: Option<String>,
    #[arg(long)]
    pub length: Option<u32>,
    #[arg(long)]
    pub digits: Option<u32>,
    #[arg(long)]
    pub fraction_digits: Option<u32>,
    #[arg(long)]
    pub nonnegative: bool,
    #[arg(long)]
    pub fixed_length: bool,
    #[arg(long, value_enum)]
    pub date_fractions: Option<DateFractions>,
}

pub(super) fn localize(command: clap::Command, locale: &Localizer) -> clap::Command {
    command
        .mut_subcommand("create-schema", |command| {
            command
                .about(locale.text("metadata-create-schema-about"))
                .mut_arg("owner", |arg| {
                    arg.help(locale.text("metadata-create-owner-help"))
                })
                .mut_arg("help", |arg| arg.help(locale.text("cli-help")))
        })
        .mut_subcommand("add", |mut command| {
            command = command.about(locale.text("metadata-create-about"));
            for (name, key) in [
                ("kind", "metadata-create-kind-help"),
                ("owner", "metadata-create-owner-help"),
                ("name", "metadata-create-name-help"),
                ("value_type", "metadata-create-type-help"),
                ("reference", "metadata-create-reference-help"),
                ("length", "metadata-create-length-help"),
                ("digits", "metadata-create-digits-help"),
                ("fraction_digits", "metadata-create-fraction-digits-help"),
                ("nonnegative", "metadata-create-nonnegative-help"),
                ("fixed_length", "metadata-create-fixed-length-help"),
                ("date_fractions", "metadata-create-date-fractions-help"),
                ("synonym", "metadata-create-synonym-help"),
                ("comment", "metadata-create-comment-help"),
                ("snapshot", "metadata-create-snapshot-help"),
                ("input", "metadata-create-input-help"),
                ("plan", "metadata-create-plan-help"),
                ("interactive", "metadata-create-interactive-help"),
                ("no_input", "metadata-create-no-input-help"),
                ("dry_run", "metadata-create-dry-run-help"),
                ("help", "cli-help"),
            ] {
                command = command.mut_arg(name, |arg| arg.help(locale.text(key)));
            }
            command
        })
}
