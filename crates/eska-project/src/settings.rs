//! Versioned transport for the shared project settings editor.

use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use clap::{ArgAction, Args, Subcommand, ValueEnum};
use eska_analyzer::{Error as AnalyzerError, configuration, process::Analyzer};
use eska_presentation::{
    diagnostics,
    localization::{LocalizationValue, Localizer},
};
use eska_project_core::{
    formatting::{self, Config, Language, Patch},
    project::{
        settings::{self, Edit, Error, SettingsFile, Snapshot, Update},
        templates,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{analyzer_config, cli::OutputFormat};

#[derive(Debug, Args)]
pub struct SettingsArgs {
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    #[arg(long, global = true)]
    analyzer: Option<PathBuf>,
    #[arg(long, global = true)]
    application: Option<String>,
    #[command(subcommand)]
    action: Action,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum Action {
    #[command(disable_help_flag = true)]
    Show(FileArgs),
    #[command(disable_help_flag = true)]
    Preview(FileArgs),
    #[command(disable_help_flag = true)]
    Save(FileArgs),
}

#[derive(Clone, Copy, Debug, Args)]
struct FileArgs {
    #[arg(long, value_enum)]
    file: File,
    #[arg(short, long, action = ArgAction::Help)]
    help: Option<bool>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum File {
    Manifest,
    Ignore,
    Attributes,
}

impl From<File> for SettingsFile {
    fn from(value: File) -> Self {
        match value {
            File::Manifest => Self::Manifest,
            File::Ignore => Self::Ignore,
            File::Attributes => Self::Attributes,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestInput {
    #[serde(deserialize_with = "required_revision")]
    revision: Option<String>,
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextInput {
    #[serde(deserialize_with = "required_revision")]
    revision: Option<String>,
    value: String,
}

fn required_revision<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::deserialize(deserializer)
}

pub fn run(root: &Path, args: &SettingsArgs, localizer: &Localizer) -> ExitCode {
    match execute(root, args, localizer) {
        Ok(result) => {
            match args.format {
                OutputFormat::Human => {
                    println!("{}", localizer.text("project-document-ready"));
                    if let Some(after) = result.get("after").and_then(Value::as_str) {
                        print!("{after}");
                    }
                }
                OutputFormat::Json => println!("{result}"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => match &error {
            CommandError::Settings(error) => emit_error(error, args.format, localizer),
            CommandError::Analyzer(error) => {
                let message = analyzer_config::error_message(error, localizer);
                eprintln!("{message}");
                analyzer_config::emit_check_output(error);
                if matches!(args.format, OutputFormat::Json) {
                    println!(
                        "{}",
                        json!({"schema_version": 1, "kind": "project-settings", "status": "error", "error": {
                            "code": "analyzer_validation", "data": analyzer_config::error_document(error)
                        }})
                    );
                }
                if matches!(**error, AnalyzerError::Cancelled) {
                    ExitCode::from(130)
                } else {
                    ExitCode::FAILURE
                }
            }
        },
    }
}

enum CommandError {
    Settings(Error),
    Analyzer(Box<AnalyzerError>),
}

impl From<Error> for CommandError {
    fn from(error: Error) -> Self {
        Self::Settings(error)
    }
}

impl From<AnalyzerError> for CommandError {
    fn from(error: AnalyzerError) -> Self {
        Self::Analyzer(Box::new(error))
    }
}

fn execute(root: &Path, args: &SettingsArgs, localizer: &Localizer) -> Result<Value, CommandError> {
    let (file, save) = match args.action {
        Action::Show(file) => {
            let mut result = ready(settings::show(root, file.file.into())?);
            if matches!(args.format, OutputFormat::Json) {
                match file.file {
                    File::Manifest => {
                        result["format_schema"] =
                            serde_json::from_str(include_str!("../schemas/format-v1.schema.json"))
                                .map_err(|_| Error::InvalidManifest)?;
                        result["format_presets"] = format_presets(root)?;
                        if let Some(inherited) = settings::inherited_formatting(root) {
                            result["format_inherited"] = inherited;
                        }
                    }
                    File::Ignore => result["git_template"] = git_template(".gitignore"),
                    File::Attributes => result["git_template"] = git_template(".gitattributes"),
                }
            }
            return Ok(result);
        }
        Action::Preview(file) => (file.file, false),
        Action::Save(file) => (file.file, true),
    };

    let (revision, update) = read_update(file)?;
    let plan = settings::preview(root, file.into(), revision.as_deref(), update)?;
    let refresh =
        save && matches!(file, File::Manifest) && plan.changed() && args.analyzer.is_some();
    let validate = plan.analyzer_policy_changed();
    let cancelled = Arc::new(AtomicBool::new(false));
    let analyzer = || Analyzer {
        binary: analyzer_config::binary(args.analyzer.clone()),
        timeout: Duration::from_secs(60),
        cancelled: &cancelled,
        connection_closed: None,
    };
    if validate {
        watch_cancellation(&cancelled)?;
        configuration::validate_manifest_candidate(
            plan.root(),
            args.application.as_deref(),
            plan.after(),
            &analyzer(),
        )?;
    }
    if save {
        let root = plan.root().to_path_buf();
        let mut result = ready(settings::save(plan)?);
        if refresh {
            let refresh = (|| {
                if !validate {
                    watch_cancellation(&cancelled)?;
                }
                let plan = configuration::inspect(&root, args.application.as_deref())?;
                configuration::prepare(&plan, &analyzer(), false)
            })();
            result["analyzer"] = match refresh {
                Ok(_) => json!({"status": "current"}),
                Err(error) => {
                    eprintln!("{}", analyzer_config::error_message(&error, localizer));
                    analyzer_config::emit_check_output(&error);
                    json!({"status": "error", "error": analyzer_config::error_document(&error)})
                }
            };
        }
        Ok(result)
    } else {
        let before = Value::from(plan.before());
        let after = Value::from(plan.after());
        let changed = plan.changed();
        let mut result = ready(plan.into_snapshot());
        result["before"] = before;
        result["after"] = after;
        result["changed"] = changed.into();
        if validate {
            result["analyzer"] = json!({"status": "checked"});
        }
        Ok(result)
    }
}

fn watch_cancellation(cancelled: &Arc<AtomicBool>) -> Result<(), AnalyzerError> {
    let signal = Arc::clone(cancelled);
    ctrlc::set_handler(move || signal.store(true, Ordering::Release))
        .map_err(|error| AnalyzerError::ProcessIo(io::Error::other(error)))
}

fn read_update(file: File) -> Result<(Option<String>, Update), Error> {
    const MAX_INPUT: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(Error::TooLarge);
    }
    match file {
        File::Manifest => serde_json::from_slice::<ManifestInput>(&bytes)
            .map(|input| (input.revision, Update::Manifest(input.edits))),
        File::Ignore | File::Attributes => serde_json::from_slice::<TextInput>(&bytes)
            .map(|input| (input.revision, Update::Text(input.value))),
    }
    .map_err(|_| Error::InvalidRequest)
}

fn ready(snapshot: Snapshot) -> Value {
    let mut response = json!({
        "schema_version": 1,
        "kind": "project-settings",
        "status": "ready",
        "revision": snapshot.revision,
    });
    response["value"] = snapshot.value;
    response
}

fn git_template(name: &str) -> Value {
    templates::built_in_git_files()
        .into_iter()
        .find(|file| file.path() == Path::new(name))
        .map_or(Value::Null, |file| json!(file.contents()))
}

fn format_presets(root: &Path) -> Result<Value, Error> {
    let mut result = json!({});
    for name in ["eska", "1c"] {
        let config = Config {
            profile: Some(name.into()),
            ..Config::default()
        };
        for (language, key) in [(Language::Bsl, "bsl"), (Language::Sdbl, "sdbl")] {
            let layer =
                formatting::Layer::new(root, "manifest", &config, "").map_err(Error::Formatting)?;
            let effective = formatting::resolve(&[layer], root, language, &Patch::default())
                .map_err(Error::Formatting)?;
            result[name][key] = json!(effective.options);
        }
    }
    Ok(result)
}

fn emit_error(error: &Error, format: OutputFormat, localizer: &Localizer) -> ExitCode {
    let (code, message, field) = match error {
        Error::MissingProject => (
            "project_discovery",
            localizer.text("project-document-missing"),
            None,
        ),
        Error::Io(_) => ("settings_io", localizer.text("project-document-io"), None),
        Error::UnsafePath => (
            "settings_unsafe_path",
            localizer.text("project-document-unsafe"),
            None,
        ),
        Error::TooLarge => (
            "settings_too_large",
            localizer.text("project-document-too-large"),
            None,
        ),
        Error::InvalidRequest => (
            "invalid_request",
            localizer.text("project-document-input"),
            None,
        ),
        Error::InvalidManifest => (
            "invalid_manifest",
            localizer.text("project-document-invalid"),
            None,
        ),
        Error::InvalidField(field) => (
            "invalid_field",
            field_message(localizer, field),
            Some(field.as_str()),
        ),
        Error::Manifest(error) => (
            "invalid_manifest",
            diagnostics::present_context_error(error, localizer),
            None,
        ),
        Error::Formatting(error) => (
            "invalid_manifest",
            field_message(localizer, &error.field),
            Some(error.field.as_str()),
        ),
        Error::Changed => (
            "settings_changed",
            localizer.text("project-document-changed"),
            None,
        ),
    };
    if matches!(format, OutputFormat::Json) {
        let data = field.map_or_else(|| json!({}), |field| json!({"field": field}));
        println!(
            "{}",
            json!({"schema_version": 1, "kind": "project-settings", "status": "error", "error": {"code": code, "data": data}})
        );
    }
    eprintln!("{message}");
    ExitCode::FAILURE
}

fn field_message(localizer: &Localizer, field: &str) -> String {
    localizer.format(
        "project-document-field",
        &[("field", LocalizationValue::Text(field))],
    )
}

pub fn localize(command: clap::Command, localizer: &Localizer) -> clap::Command {
    command
        .about(localizer.text("project-document-about"))
        .mut_arg("format", |arg| {
            arg.help(localizer.text("version-format-help"))
                .value_name(localizer.text("version-format-value"))
        })
        .mut_arg("analyzer", |arg| {
            arg.help(localizer.text("analyzer-binary-help"))
        })
        .mut_arg("application", |arg| {
            arg.help(localizer.text("project-application-help"))
        })
        .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        .mut_subcommands(|command| {
            let about = match command.get_name() {
                "show" => "project-document-show",
                "preview" => "project-document-preview",
                _ => "project-document-save",
            };
            command
                .about(localizer.text(about))
                .mut_arg("file", |arg| {
                    arg.help(localizer.text("project-document-file"))
                        .value_name(localizer.text("project-document-file-value"))
                })
                .mut_arg("help", |arg| arg.help(localizer.text("cli-help")))
        })
}
