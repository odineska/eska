//! Standalone DAP request parameters. Project policy is enforced at the other entry point.
use std::{env, path::PathBuf};

use eska_build_runtime::{
    build::ProcessingArtifactError,
    runtime::{DebugConnection, DebugLaunch},
};
use eska_project_core::project::local_settings::{
    Authentication, DebugEndpoint, Infobase, Profile, Runner, debug_server_url,
    valid_auto_attach_types,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    Mode,
    session::{AttachSelection, Config},
    source::{SourceError, Sources},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Arguments {
    source_root: Option<PathBuf>,
    #[serde(default)]
    extension_roots: Vec<PathBuf>,
    debug_server_url: Option<String>,
    #[serde(default)]
    managed_debug_server: bool,
    debug_server_port: Option<u16>,
    infobase_alias: Option<String>,
    target_id: Option<String>,
    auto_attach_types: Option<Vec<String>>,
    infobase: Option<Infobase>,
    runner: Option<Runner>,
    authentication: Option<Authentication>,
    external_processing: Option<ExternalProcessing>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExternalProcessing {
    source_root: PathBuf,
    artifact: PathBuf,
}

impl Arguments {
    fn valid(&self, mode: Mode) -> bool {
        let processing = self.external_processing.as_ref();
        (self.source_root.is_some() || processing.is_some())
            && self
                .source_root
                .as_ref()
                .is_none_or(|path| !path.as_os_str().is_empty())
            && processing.is_none_or(|processing| {
                mode == Mode::Launch
                    && !processing.source_root.as_os_str().is_empty()
                    && !processing.artifact.as_os_str().is_empty()
            })
            && self
                .extension_roots
                .iter()
                .all(|root| !root.as_os_str().is_empty())
            && self
                .auto_attach_types
                .as_deref()
                .is_none_or(valid_auto_attach_types)
            && self.target_id.as_deref().is_none_or(valid_target)
    }
}

impl ExternalProcessing {
    fn stage(self, sources: &mut Sources, launch: &mut DebugLaunch) -> Result<(), &'static str> {
        let (provenance, staged) = launch
            .stage_processing(&self.source_root, &self.artifact)
            .map_err(|error| match error {
                ProcessingArtifactError::SourceMismatch => "debug-epf-source-mismatch",
                ProcessingArtifactError::ArtifactMismatch => "debug-epf-artifact-mismatch",
                ProcessingArtifactError::Io(_) | ProcessingArtifactError::Manifest => {
                    "debug-epf-manifest"
                }
            })?;
        sources
            .add_processing(&self.source_root, &staged, provenance)
            .map_err(|_| "debug-epf-source-mismatch")
    }
}

pub fn configure(mode: Mode, arguments: &Value) -> Result<Config, &'static str> {
    const INVALID: &str = "debug-standalone-arguments";
    let args: Arguments = Arguments::deserialize(arguments).map_err(|_| INVALID)?;
    if !args.valid(mode) {
        return Err(INVALID);
    }
    let alias = args.infobase_alias.unwrap_or_else(|| match &args.infobase {
        Some(Infobase::Cluster { name, .. } | Infobase::Standalone { name, .. }) => name.clone(),
        _ => "DefAlias".to_owned(),
    });
    if alias.trim().is_empty() || alias.len() > 512 || alias.chars().any(char::is_control) {
        return Err(INVALID);
    }
    let debug = DebugEndpoint {
        url: args.debug_server_url,
        managed: args.managed_debug_server,
        port: args.debug_server_port,
        auto_attach_types: args.auto_attach_types,
    };
    let mut auto_attach_types = debug.auto_attach_types();
    let (connection, mut launch, attach_selection) = match mode {
        Mode::Launch if args.target_id.is_none() => {
            let infobase = args.infobase.ok_or(INVALID)?;
            let runner = args.runner.ok_or(INVALID)?;
            let profile = Profile {
                infobase,
                runner,
                debug: Some(debug),
                authentication: args.authentication,
            };
            let working_dir = env::current_dir().map_err(|_| INVALID)?;
            let launch =
                DebugLaunch::prepare_standalone(working_dir, profile).map_err(|error| {
                    if matches!(
                        error,
                        eska_build_runtime::runtime::RuntimeError::Settings(_)
                    ) {
                        INVALID
                    } else {
                        crate::runtime_failure(&error)
                    }
                })?;
            let connection = launch.connection().map_err(|_| INVALID)?;
            (connection, Some(launch), None)
        }
        Mode::Attach
            if args.runner.is_none()
                && args.infobase.is_none()
                && args.authentication.is_none()
                && !debug.managed
                && debug.port.is_none() =>
        {
            let selection = args
                .target_id
                .map_or(AttachSelection::MatchingTypes, AttachSelection::Exact);
            if matches!(selection, AttachSelection::Exact(_)) {
                auto_attach_types.clear();
            }
            let endpoint = debug
                .url
                .as_deref()
                .and_then(debug_server_url)
                .ok_or(INVALID)?;
            (DebugConnection::External(endpoint), None, Some(selection))
        }
        _ => return Err(INVALID),
    };
    let mut sources = Sources::from_designer(args.source_root.as_deref(), &args.extension_roots)
        .map_err(|error| match error {
            SourceError::DuplicateExtension => "debug-duplicate-extension",
            SourceError::Project | SourceError::ApplicationChoice => "debug-standalone-source",
        })?;
    if let Some(processing) = args.external_processing {
        processing.stage(&mut sources, launch.as_mut().ok_or(INVALID)?)?;
    }
    Ok(Config {
        connection,
        alias,
        auto_attach_types,
        sources,
        mode,
        launch,
        production: false,
        attach_selection,
    })
}

fn valid_target(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
