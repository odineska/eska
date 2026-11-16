//! Structural authoring accepts semantic inputs and replayable plans for both people and agents.

use std::path::PathBuf;

use clap::Args;

use crate::localization::Localizer;

#[derive(Clone, Debug, Args)]
pub struct AuthoringInput {
    #[arg(long, conflicts_with = "plan")]
    pub input: Option<PathBuf>,
    #[arg(long)]
    pub plan: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub no_input: bool,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

#[derive(Clone, Debug, Args)]
pub struct ModuleCreateArgs {
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub owner: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub role: Option<String>,
    #[command(flatten)]
    pub source: AuthoringInput,
}

#[derive(Clone, Debug, Args)]
pub struct ObjectCreateArgs {
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub kind: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub name: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub parent: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub template: Option<String>,
    #[arg(long = "set", value_name = "PROPERTY=VALUE", conflicts_with_all = ["input", "plan"])]
    pub property_values: Vec<String>,
    #[arg(long = "reference", value_name = "PROPERTY=VALUE", conflicts_with_all = ["input", "plan"])]
    pub property_references: Vec<String>,
    #[arg(long = "type", value_name = "PROPERTY=TYPE", conflicts_with_all = ["input", "plan"])]
    pub property_types: Vec<String>,
    #[command(flatten)]
    pub source: AuthoringInput,
}

#[derive(Clone, Debug, Args)]
pub struct ObjectCopyArgs {
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub source: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub name: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub parent: Option<String>,
    #[arg(long = "set", value_name = "PROPERTY=VALUE", conflicts_with_all = ["input", "plan"])]
    pub property_values: Vec<String>,
    #[arg(long = "reference", value_name = "PROPERTY=VALUE", conflicts_with_all = ["input", "plan"])]
    pub property_references: Vec<String>,
    #[arg(long = "type", value_name = "PROPERTY=TYPE", conflicts_with_all = ["input", "plan"])]
    pub property_types: Vec<String>,
    #[command(flatten)]
    pub input: AuthoringInput,
}

#[derive(Clone, Debug, Args)]
pub struct ObjectDeleteArgs {
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub object: Option<String>,
    #[arg(long, conflicts_with_all = ["input", "plan"])]
    pub force: bool,
    #[arg(long)]
    pub yes: bool,
    #[command(flatten)]
    pub source: AuthoringInput,
}

#[derive(Debug, Args)]
pub struct ObjectSchemaArgs {
    #[arg(long)]
    pub kind: Option<String>,
    #[arg(long)]
    pub parent: Option<String>,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

#[derive(Debug, Args)]
pub struct FormSchemaArgs {
    #[arg(long)]
    pub form: String,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

#[derive(Debug, Args)]
pub struct MetadataListArgs {
    #[arg(long)]
    pub parent: Option<String>,
    #[arg(long)]
    pub kind: Option<String>,
    #[arg(long)]
    pub search: Option<String>,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
    pub limit: u32,
    #[arg(long)]
    pub after: Option<String>,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

pub(super) fn localize(mut command: clap::Command, locale: &Localizer) -> clap::Command {
    for name in [
        "create",
        "create-module",
        "copy",
        "delete",
        "object-schema",
        "form-schema",
        "form-edit",
        "list",
    ] {
        command = command.mut_subcommand(name, |mut command| {
            command = command.about(locale.text(&format!("metadata-authoring-{name}-about")));
            let fields: Vec<_> = command
                .get_arguments()
                .map(|arg| arg.get_id().as_str().to_owned())
                .collect();
            for field in fields {
                let key = match field.as_str() {
                    "help" => "cli-help".to_owned(),
                    "name" => "metadata-create-name-help".to_owned(),
                    "input" => "metadata-create-input-help".to_owned(),
                    "plan" => "metadata-create-plan-help".to_owned(),
                    "dry_run" => "metadata-create-dry-run-help".to_owned(),
                    "no_input" => "metadata-create-no-input-help".to_owned(),
                    "object" => "metadata-object-help".to_owned(),
                    "project" => "metadata-project-help".to_owned(),
                    "format" => "metadata-format-help".to_owned(),
                    _ => format!("metadata-authoring-{}-help", field.replace('_', "-")),
                };
                command = command.mut_arg(&field, |arg| arg.help(locale.text(&key)));
            }
            command
        });
    }
    command
}
