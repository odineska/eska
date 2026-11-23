//! Repository monitoring options shared by the facade and standalone VCS tool.

use super::status::OutputFormat;
use crate::localization::Localizer;
use clap::Args;

#[derive(Debug, Args)]
pub struct MonitorArgs {
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    #[arg(short, long, action = clap::ArgAction::Help)]
    pub help: Option<bool>,
}

/// Attach localized help without changing the machine-readable command contract.
#[must_use]
pub fn localize(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .about(localizer.text("monitor-about"))
        .override_usage(localizer.text("monitor-usage"))
        .mut_arg("format", |argument| {
            argument
                .help(localizer.text("status-format-help"))
                .value_name(localizer.text("status-format-value"))
        })
        .mut_arg("help", |argument| argument.help(localizer.text("cli-help")))
}

#[cfg(test)]
mod tests {
    use super::{MonitorArgs, OutputFormat};
    use clap::Parser;

    #[derive(Parser)]
    #[command(disable_help_flag = true)]
    struct Cli {
        #[command(flatten)]
        monitor: MonitorArgs,
    }

    #[test]
    fn accepts_only_supported_output_formats() {
        let default = Cli::try_parse_from(["monitor"]).expect("default format");
        assert!(matches!(default.monitor.format, OutputFormat::Human));
        let json = Cli::try_parse_from(["monitor", "--format", "json"]).expect("JSON format");
        assert!(matches!(json.monitor.format, OutputFormat::Json));
        assert!(Cli::try_parse_from(["monitor", "--format", "xml"]).is_err());
    }
}
