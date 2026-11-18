//! Installed 1C platform discovery for the effective machine runner.

pub use eska_presentation::arguments::build::platform::{PlatformArgs, localize};
use eska_presentation::arguments::build::platform::{
    PlatformCommand, PlatformListArgs, PlatformOutputFormat,
};

use std::process::ExitCode;

use serde::Serialize;

use crate::{
    cli::{diagnostics, localization::Localizer, platform},
    project::build::{Ibcmd, ToolSource},
};

#[derive(Serialize)]
struct PlatformDocument<'a> {
    schema_version: u8,
    platforms: Vec<PlatformEntry<'a>>,
}

#[derive(Serialize)]
struct PlatformEntry<'a> {
    version: &'a str,
    source: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InventorySource {
    Host,
    Distrobox { container: String },
}

#[derive(Serialize)]
struct InventoryEntry {
    version: String,
    ibcmd: String,
    source: InventorySource,
}

struct PlatformHandler<'a> {
    options: &'a PlatformArgs,
}

impl PlatformHandler<'_> {
    /// Scan platforms without requiring a project.
    pub fn run(&self, localizer: &Localizer) -> ExitCode {
        match &self.options.command {
            PlatformCommand::List(args) => PlatformListHandler { options: args }.run(localizer),
        }
    }
}

struct PlatformListHandler<'a> {
    options: &'a PlatformListArgs,
}

impl PlatformListHandler<'_> {
    /// Print every verified platform visible through the selected runner.
    fn run(&self, localizer: &Localizer) -> ExitCode {
        let scope = match super::artifact::cancellation_scope(localizer) {
            Ok(scope) => scope,
            Err(error) => {
                error.report();
                return ExitCode::FAILURE;
            }
        };
        let options = match platform::tool_options(
            self.options.ibcmd.clone(),
            self.options.platform_arch.clone(),
            self.options.distrobox.clone(),
        ) {
            Ok(options) => options,
            Err(error) => {
                eprintln!(
                    "{}",
                    diagnostics::present_global_config_error(&error, localizer)
                );
                return ExitCode::FAILURE;
            }
        };
        let options = super::artifact::scoped_options(options, scope.as_ref());
        if self.options.include_distrobox {
            return print_inventory(&options, self.options.format, localizer, scope.as_ref());
        }
        let platforms = match Ibcmd::installed(&options) {
            Ok(platforms) => platforms,
            Err(error) => {
                eprintln!("{}", diagnostics::present_tool_error(&error, localizer));
                return ExitCode::FAILURE;
            }
        };
        if scope
            .as_ref()
            .is_some_and(eska_tool_runtime::Operation::is_cancelled)
        {
            eprintln!("{}", localizer.text("component-cancelled"));
            return ExitCode::FAILURE;
        }
        match self.options.format {
            PlatformOutputFormat::Human => {
                if platforms.is_empty() {
                    println!("{}", localizer.text("platform-none"));
                } else {
                    for platform in platforms {
                        println!(
                            "{}\t{}",
                            platform.version().as_str(),
                            source_label(platform.source())
                        );
                    }
                }
            }
            PlatformOutputFormat::Json => {
                let document = PlatformDocument {
                    schema_version: 1,
                    platforms: platforms
                        .iter()
                        .map(|platform| PlatformEntry {
                            version: platform.version().as_str(),
                            source: source_label(platform.source()),
                        })
                        .collect(),
                };
                let Ok(json) = serde_json::to_string_pretty(&document) else {
                    eprintln!("{}", localizer.text("platform-json-error"));
                    return ExitCode::FAILURE;
                };
                println!("{json}");
            }
        }
        ExitCode::SUCCESS
    }
}

fn source_label(source: &ToolSource) -> String {
    match source {
        ToolSource::Explicit(path) | ToolSource::Path(path) | ToolSource::Standard(path) => {
            path.to_string_lossy().into_owned()
        }
        ToolSource::Distrobox { container, path } => {
            format!("{container}:{}", path.to_string_lossy())
        }
    }
}

/// Execute parsed platform inventory options in the native owner.
pub fn run(args: &PlatformArgs, localizer: &Localizer) -> ExitCode {
    PlatformHandler { options: args }.run(localizer)
}

/// Source failures remain independent so callers can still select a verified installation.
fn print_inventory(
    options: &crate::project::build::ToolOptions,
    format: PlatformOutputFormat,
    localizer: &Localizer,
    scope: Option<&eska_tool_runtime::Operation>,
) -> ExitCode {
    let mut entries = Vec::new();
    let mut collect = |container: Option<String>| {
        let Ok(platforms) = Ibcmd::installed(&options.inventory_source(container.clone())) else {
            return "unavailable";
        };
        let source = container.map_or(InventorySource::Host, |container| {
            InventorySource::Distrobox { container }
        });
        for platform in platforms {
            let path = match platform.source() {
                ToolSource::Explicit(path)
                | ToolSource::Path(path)
                | ToolSource::Standard(path) => std::path::absolute(path).ok(),
                ToolSource::Distrobox { path, .. } => path.is_absolute().then(|| path.clone()),
            };
            let Some(path) = path.as_deref().and_then(std::path::Path::to_str) else {
                continue;
            };
            entries.push(InventoryEntry {
                version: platform.version().as_str().to_owned(),
                ibcmd: path.to_owned(),
                source: source.clone(),
            });
        }
        "ready"
    };
    let host = collect(None);
    let distrobox = options.distrobox_name().map(|container| {
        let status = collect(Some(container.clone()));
        serde_json::json!({"container":container,"status":status})
    });
    if scope.is_some_and(eska_tool_runtime::Operation::is_cancelled) {
        eprintln!("{}", localizer.text("component-cancelled"));
        return ExitCode::FAILURE;
    }
    match format {
        PlatformOutputFormat::Json => println!(
            "{}",
            serde_json::json!({"schema_version":1,"kind":"installed-platforms", "platforms":entries,"sources":{"host":host,"distrobox":distrobox}})
        ),
        PlatformOutputFormat::Human => {
            if entries.is_empty() {
                println!("{}", localizer.text("platform-none"));
            }
            for entry in entries {
                let label = match entry.source {
                    InventorySource::Host => localizer.text("platform-inventory-host"),
                    InventorySource::Distrobox { container } => format!("Distrobox ({container})"),
                };
                println!("{}\t{label}\t{}", entry.version, entry.ibcmd);
            }
        }
    }
    ExitCode::SUCCESS
}
