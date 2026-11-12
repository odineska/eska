//! Standalone DAP request parameters. Project policy is enforced at the other entry point.
use std::{env, path::PathBuf};

use eska_build_runtime::runtime::DebugLaunch;
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
    source_root: PathBuf,
    #[serde(default)]
    extension_roots: Vec<PathBuf>,
    debug_server_url: String,
    infobase_alias: Option<String>,
    target_id: Option<String>,
    auto_attach_types: Option<Vec<String>>,
    infobase: Option<Infobase>,
    runner: Option<Runner>,
    authentication: Option<Authentication>,
}

pub fn configure(mode: Mode, arguments: &Value) -> Result<Config, &'static str> {
    const INVALID: &str = "debug-standalone-arguments";
    let args: Arguments = Arguments::deserialize(arguments).map_err(|_| INVALID)?;
    if args.source_root.as_os_str().is_empty()
        || args
            .extension_roots
            .iter()
            .any(|root| root.as_os_str().is_empty())
        || args
            .auto_attach_types
            .as_deref()
            .is_some_and(|types| !valid_auto_attach_types(types))
        || args
            .target_id
            .as_deref()
            .is_some_and(|id| !valid_target(id))
    {
        return Err(INVALID);
    }
    let endpoint = debug_server_url(&args.debug_server_url).ok_or(INVALID)?;
    let alias = args.infobase_alias.unwrap_or_else(|| match &args.infobase {
        Some(Infobase::Cluster { name, .. } | Infobase::Standalone { name, .. }) => name.clone(),
        _ => "DefAlias".to_owned(),
    });
    if alias.trim().is_empty() || alias.len() > 512 || alias.chars().any(char::is_control) {
        return Err(INVALID);
    }
    let debug = DebugEndpoint {
        url: args.debug_server_url,
        auto_attach_types: args.auto_attach_types,
    };
    let mut auto_attach_types = debug.auto_attach_types();
    let (launch, attach_selection) = match mode {
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
            (Some(launch), None)
        }
        Mode::Attach
            if args.runner.is_none()
                && args.infobase.is_none()
                && args.authentication.is_none() =>
        {
            let selection = args
                .target_id
                .map_or(AttachSelection::MatchingTypes, AttachSelection::Exact);
            if matches!(selection, AttachSelection::Exact(_)) {
                auto_attach_types.clear();
            }
            (None, Some(selection))
        }
        _ => return Err(INVALID),
    };
    let sources =
        Sources::from_designer(&args.source_root, &args.extension_roots).map_err(|error| {
            match error {
                SourceError::DuplicateExtension => "debug-duplicate-extension",
                SourceError::Project | SourceError::ApplicationChoice => "debug-standalone-source",
            }
        })?;
    Ok(Config {
        endpoint,
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
