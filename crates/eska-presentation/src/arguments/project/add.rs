//! Shared add intent; the project component owns dependency installation and binding.

pub use super::version::OutputFormat;
use crate::localization::Localizer;
use clap::Args;

#[derive(Debug, Args)]
pub struct AddArgs {
    pub library: String,
    #[arg(short = 'p', long)]
    pub project: Vec<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,
    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

#[must_use]
pub fn localize(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .about(localizer.text("library-add-about"))
        .help_template(format!(
            "{{about-with-newline}}\n{}: {{usage}}\n\n{}:\n{{positionals}}\n{}:\n{{options}}",
            localizer.text("cli-usage"),
            localizer.text("cli-arguments"),
            localizer.text("cli-options")
        ))
        .override_usage(localizer.text("library-add-usage"))
        .mut_arg("library", |argument| {
            argument
                .help(localizer.text("library-add-name-help"))
                .value_name(localizer.text("library-add-name-value"))
        })
        .mut_arg("project", |argument| {
            argument
                .help(localizer.text("version-project-help"))
                .value_name(localizer.text("version-project-value"))
        })
        .mut_arg("format", |argument| {
            argument
                .help(localizer.text("version-format-help"))
                .value_name(localizer.text("version-format-value"))
        })
        .mut_arg("help", |argument| argument.help(localizer.text("cli-help")))
}
