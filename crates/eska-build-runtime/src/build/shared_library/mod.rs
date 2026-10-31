//! One verified CF and immutable database template serve registered library consumers.

mod storage;
mod template;

use super::{
    BuildError, BuildStage, Ibcmd, ProcessStream, ToolSource, base_configuration, execute,
    source_snapshot,
};
use eska_project_core::{
    application::LibrarySelection,
    config::{ProjectConfig, global},
    project::ProjectType,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u8,
    source_snapshot: String,
    platform_version: String,
    cf_sha256: String,
}

pub(super) struct PreparedLibrary {
    pub path: PathBuf,
    pub snapshot: String,
    pub tool_output: Vec<u8>,
    lease: storage::Lease,
    state: State,
}

pub(super) fn preflight(library: &LibrarySelection) -> Result<(), BuildError> {
    let root = global::library_cache_root(&library.config)
        .map_err(|source| BuildError::LibraryConfiguration(Box::new(source)))?;
    ensure_outside(&library.source, &root)
}

pub(super) fn ensure_outside(source: &Path, output: &Path) -> Result<(), BuildError> {
    let source = fs::canonicalize(source).map_err(|error| BuildError::BaseConfigurationRead {
        path: source.to_owned(),
        source: error,
    })?;
    let mut ancestor = output;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| BuildError::OutputParentMissing(output.to_owned()))?;
    }
    let resolved = fs::canonicalize(ancestor).map_err(|error| storage::error(ancestor, error))?;
    if resolved.starts_with(&source) {
        return Err(BuildError::BaseConfigurationOutputOverlap {
            source,
            output: output.to_owned(),
        });
    }
    Ok(())
}

pub(super) fn prepare<F>(
    library: &LibrarySelection,
    input: &Path,
    ibcmd: &Ibcmd,
    workspace: &Path,
    on_output: &mut F,
) -> Result<PreparedLibrary, BuildError>
where
    F: FnMut(BuildStage, ProcessStream, &[u8]),
{
    let root = global::library_cache_root(&library.config)
        .map_err(|source| BuildError::LibraryConfiguration(Box::new(source)))?
        .join(input_key(library, ibcmd)?);
    ensure_outside(&library.source, &root)?;
    let lease = storage::Lease::acquire(root, ibcmd)?;
    let cf = lease.root.join("base.cf");
    let stamp = lease.root.join("base.json");
    execute::validate_existing_output(&cf)?;
    execute::validate_existing_output(&stamp)?;
    let used = workspace.join("base.cf");
    let excluded = exclusions(input);
    let refs: Vec<_> = excluded.iter().map(PathBuf::as_path).collect();
    let current =
        snapshot_id(&source_snapshot::digest(input, &refs).map_err(BuildError::SourceSnapshot)?);
    if let Some(state) = storage::read::<State>(&stamp)
        && state.schema_version == 1
        && state.platform_version == ibcmd.version().as_str()
        && state.source_snapshot == current
        && cf.is_file()
    {
        match source_snapshot::file_digest(&cf, Some(&used)) {
            Ok(digest) if hex(&digest) == state.cf_sha256 => {
                return Ok(PreparedLibrary {
                    path: used,
                    snapshot: current,
                    tool_output: Vec::new(),
                    lease,
                    state,
                });
            }
            Err(source_snapshot::Error::Io { path, source }) if path == used => {
                return Err(storage::error(&path, source));
            }
            Ok(_) | Err(_) => {}
        }
        if used.exists() {
            fs::remove_file(&used).map_err(|source| storage::error(&used, source))?;
        }
    }
    let job = execute::Workspace::create(&lease.root)?;
    let copied = job.path.join("base-source");
    let snapshot = snapshot_id(
        &source_snapshot::capture(input, &copied, &refs).map_err(BuildError::SourceSnapshot)?,
    );
    check_cancel(ibcmd)?;
    let (artifact, tool_output) =
        base_configuration::compile_xml(&copied, ibcmd, &job.path, on_output)?;
    let state = State {
        schema_version: 1,
        source_snapshot: snapshot.clone(),
        platform_version: ibcmd.version().as_str().into(),
        cf_sha256: hex(
            &source_snapshot::file_digest(&artifact, None).map_err(BuildError::SourceSnapshot)?
        ),
    };
    let staged = job.path.join("state.json");
    storage::write(&staged, &state)?;
    check_cancel(ibcmd)?;
    execute::publish_pair(&artifact, &cf, &staged, &stamp)?;
    let digest =
        source_snapshot::file_digest(&cf, Some(&used)).map_err(BuildError::SourceSnapshot)?;
    if hex(&digest) != state.cf_sha256 {
        return Err(BuildError::BaseConfigurationChanged(cf));
    }
    Ok(PreparedLibrary {
        path: used,
        snapshot,
        tool_output,
        lease,
        state,
    })
}

/// A root-owned manifest may place build data below source='.'; it is never XML input.
fn exclusions(source: &Path) -> Vec<PathBuf> {
    let mut excluded = vec![source.join(".eska"), source.join("eska.toml")];
    if let Ok(config) = ProjectConfig::load(&source.join("eska.toml"))
        && let Ok(project) = config.into_project(source.to_owned())
        && project.configuration().project_type() == ProjectType::Configuration
        && project.source() == source
    {
        excluded.push(
            source.join(
                project
                    .configuration()
                    .build_settings()
                    .artifacts_directory(),
            ),
        );
    }
    excluded
}

fn input_key(library: &LibrarySelection, ibcmd: &Ibcmd) -> Result<String, BuildError> {
    let source =
        fs::canonicalize(&library.source).map_err(|source| BuildError::BaseConfigurationRead {
            path: library.source.clone(),
            source,
        })?;
    let mut hash = Sha256::new();
    hash.update(b"eska-library-build-v1\0");
    let (path, container) = match ibcmd.source() {
        ToolSource::Explicit(path) | ToolSource::Path(path) | ToolSource::Standard(path) => {
            (path, "")
        }
        ToolSource::Distrobox { container, path } => (path, container.as_str()),
    };
    for bytes in [
        source.as_os_str().as_encoded_bytes(),
        ibcmd.version().as_str().as_bytes(),
        ibcmd.source().runner_kind().as_bytes(),
        path.as_os_str().as_encoded_bytes(),
        container.as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(hex(&hash.finalize()))
}

fn check_cancel(ibcmd: &Ibcmd) -> Result<(), BuildError> {
    execute::reject_interrupted_import(ibcmd, BuildStage::PrepareBaseConfiguration)
}
fn snapshot_id(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(bytes))
}
fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}
