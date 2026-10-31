//! Native base preparation shares the consumer's lease and never writes into the XML source.

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

use eska_project_core::{
    application::LibrarySelection,
    project::{ProjectType, designer_xml},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    BaseConfiguration, BuildError, BuildPlan, BuildStage, Ibcmd, ProcessStream, execute,
    source_snapshot,
};
use crate::platform_path::native_path;

const CACHE_CF: &str = "base.cf";
const CACHE_STATE: &str = "base.json";

pub(super) struct PreparedBase {
    pub path: PathBuf,
    pub context: Option<BaseContext>,
    pub tool_output: Vec<u8>,
    pub shared: Option<super::shared_library::PreparedLibrary>,
}

#[derive(Serialize)]
pub(super) struct BaseContext {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    library: Option<String>,
    snapshot_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u8,
    input: String,
    platform_version: String,
    source_snapshot: String,
    cf_sha256: String,
}

/// Validate the selected input before creating any build state or invoking the platform.
pub(super) fn preflight(plan: &BuildPlan) -> Result<(), BuildError> {
    if let Some(library) = plan.library_build() {
        super::shared_library::preflight(library)?;
        // The configured artifact directory is excluded from root-layout XML snapshots.
        // An arbitrary explicit result inside that XML would become input on the next build.
        if plan.has_explicit_output() && !plan.output().starts_with(plan.artifacts_directory()) {
            super::shared_library::ensure_outside(
                &library.source,
                plan.output().parent().unwrap_or_else(|| plan.output()),
            )?;
        }
    }
    let Some(input) = plan.base_configuration() else {
        return Ok(());
    };
    match input {
        BaseConfiguration::Cf(path) => {
            let metadata =
                fs::metadata(path).map_err(|source| BuildError::BaseConfigurationRead {
                    path: path.clone(),
                    source,
                })?;
            if !metadata.is_file()
                || !path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("cf"))
            {
                return Err(BuildError::BaseConfigurationInvalid(path.clone()));
            }
        }
        BaseConfiguration::DesignerXml { source, library } => {
            if !source.is_absolute() {
                return Err(BuildError::BaseConfigurationXmlInvalid(source.clone()));
            }
            let path = source.join("Configuration.xml");
            let xml =
                fs::read_to_string(&path).map_err(|source| BuildError::BaseConfigurationRead {
                    path: path.clone(),
                    source,
                })?;
            if designer_xml::project_type(&xml).ok().flatten() != Some(ProjectType::Configuration) {
                return Err(BuildError::BaseConfigurationXmlInvalid(path));
            }
            validate_readonly_source(plan, source)?;
            if let Some(library) = library {
                super::shared_library::preflight(library)?;
            }
        }
    }
    Ok(())
}

/// Neither staging nor managed data may be created below the read-only source, including aliases.
fn validate_readonly_source(plan: &BuildPlan, source: &Path) -> Result<(), BuildError> {
    let source = fs::canonicalize(source).map_err(|error| BuildError::BaseConfigurationRead {
        path: source.to_owned(),
        source: error,
    })?;
    for output in [
        plan.output().parent().unwrap_or_else(|| plan.output()),
        plan.infobase_root(),
    ] {
        let mut ancestor = output;
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| BuildError::OutputParentMissing(output.to_owned()))?;
        }
        let resolved = fs::canonicalize(ancestor).map_err(|error| BuildError::CreateDirectory {
            path: ancestor.to_owned(),
            source: error,
        })?;
        if resolved.starts_with(&source) {
            return Err(BuildError::BaseConfigurationOutputOverlap {
                source,
                output: output.to_owned(),
            });
        }
    }
    Ok(())
}

/// A valid cached CF is copied into the operation before loading it or recording its checksum.
pub(super) fn prepare<F>(
    plan: &BuildPlan,
    ibcmd: &Ibcmd,
    root: &Path,
    workspace: &Path,
    manifested: bool,
    on_output: &mut F,
) -> Result<Option<PreparedBase>, BuildError>
where
    F: FnMut(BuildStage, ProcessStream, &[u8]),
{
    match plan.base_configuration() {
        None => Ok(None),
        Some(BaseConfiguration::Cf(source)) => {
            let path = if manifested {
                let target = workspace.join(CACHE_CF);
                source_snapshot::file_digest(source, Some(&target))
                    .map_err(BuildError::SourceSnapshot)?;
                target
            } else {
                source.clone()
            };
            Ok(Some(PreparedBase {
                path,
                context: None,
                tool_output: Vec::new(),
                shared: None,
            }))
        }
        Some(BaseConfiguration::DesignerXml { source, library }) => {
            if let Some(library) = library {
                let shared =
                    super::shared_library::prepare(library, source, ibcmd, workspace, on_output)?;
                let mut result =
                    prepared(shared.path.clone(), Some(library), shared.snapshot.clone());
                result.shared = Some(shared);
                return Ok(Some(result));
            }
            prepare_xml(source, library.as_ref(), ibcmd, root, workspace, on_output).map(Some)
        }
    }
}

fn prepare_xml<F>(
    source: &Path,
    library: Option<&LibrarySelection>,
    ibcmd: &Ibcmd,
    root: &Path,
    workspace: &Path,
    on_output: &mut F,
) -> Result<PreparedBase, BuildError>
where
    F: FnMut(BuildStage, ProcessStream, &[u8]),
{
    let cf = root.join(CACHE_CF);
    let stamp = root.join(CACHE_STATE);
    execute::validate_existing_output(&cf)?;
    execute::validate_existing_output(&stamp)?;
    let used = workspace.join(CACHE_CF);
    let input = input_key(source, library);
    let current =
        snapshot_id(&source_snapshot::digest(source, &[]).map_err(BuildError::SourceSnapshot)?);
    if let Some(state) = read_state(&stamp)
        && state.schema_version == 1
        && state.input == input
        && state.platform_version == ibcmd.version().as_str()
        && state.source_snapshot == current
        && cf.is_file()
    {
        match source_snapshot::file_digest(&cf, Some(&used)) {
            Ok(digest) if hex(&digest) == state.cf_sha256 => {
                return Ok(prepared(used, library, state.source_snapshot));
            }
            Err(source_snapshot::Error::Io { path, source }) if path == used => {
                return Err(BuildError::BaseConfigurationCache { path, source });
            }
            Ok(_) | Err(_) => {}
        }
    }
    let copied = workspace.join("base-source");
    let snapshot = snapshot_id(
        &source_snapshot::capture(source, &copied, &[]).map_err(BuildError::SourceSnapshot)?,
    );
    let (artifact, output) = compile_xml(&copied, ibcmd, workspace, on_output)?;
    let state = State {
        schema_version: 1,
        input,
        platform_version: ibcmd.version().as_str().to_owned(),
        source_snapshot: snapshot.clone(),
        cf_sha256: hex(
            &source_snapshot::file_digest(&artifact, None).map_err(BuildError::SourceSnapshot)?
        ),
    };
    let staged = workspace.join("base-state.json");
    let bytes = serde_json::to_vec_pretty(&state)
        .map_err(|source| cache_error(&staged, io::Error::other(source)))?;
    fs::write(&staged, bytes).map_err(|source| cache_error(&staged, source))?;
    execute::publish_pair(&artifact, &cf, &staged, &stamp)?;
    let copied =
        source_snapshot::file_digest(&cf, Some(&used)).map_err(BuildError::SourceSnapshot)?;
    if hex(&copied) != state.cf_sha256 {
        return Err(BuildError::BaseConfigurationChanged(cf));
    }
    let mut result = prepared(used, library, snapshot);
    result.tool_output = output;
    Ok(result)
}

pub(super) fn compile_xml<F>(
    source: &Path,
    ibcmd: &Ibcmd,
    workspace: &Path,
    on_output: &mut F,
) -> Result<(PathBuf, Vec<u8>), BuildError>
where
    F: FnMut(BuildStage, ProcessStream, &[u8]),
{
    let data = workspace.join("base-data");
    let pid = workspace.join("base.pid");
    let created = execute::create_infobase(
        ibcmd,
        &data,
        None,
        &pid,
        BuildStage::PrepareBaseConfiguration,
        on_output,
    )?;
    let artifact = workspace.join("base-built.cf");
    let imported = execute::run(
        ibcmd,
        BuildStage::PrepareBaseConfiguration,
        [
            "infobase".into(),
            "config".into(),
            "import".into(),
            execute::option("--data", &data),
            execute::option("--out", &artifact),
            native_path(source).as_os_str().to_owned(),
        ],
        &pid,
        on_output,
    )?;
    execute::reject_interrupted_import(ibcmd, BuildStage::PrepareBaseConfiguration)?;
    execute::validate_artifact(&artifact)?;
    Ok((
        artifact,
        execute::collect_tool_output(&[], Some(&created), &imported),
    ))
}

fn prepared(
    path: PathBuf,
    library: Option<&LibrarySelection>,
    snapshot_id: String,
) -> PreparedBase {
    PreparedBase {
        path,
        tool_output: Vec::new(),
        shared: None,
        context: Some(BaseContext {
            kind: "designer-xml",
            library: library.map(|entry| entry.name.clone()),
            snapshot_id,
        }),
    }
}

fn read_state(path: &Path) -> Option<State> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn cache_error(path: &Path, source: io::Error) -> BuildError {
    BuildError::BaseConfigurationCache {
        path: path.to_owned(),
        source,
    }
}

fn input_key(source: &Path, library: Option<&LibrarySelection>) -> String {
    let mut hash = Sha256::new();
    hash.update(b"eska-native-base-v1\0");
    for bytes in [
        source.as_os_str().as_encoded_bytes(),
        library.map_or("", |entry| entry.name.as_str()).as_bytes(),
    ] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hex(&hash.finalize())
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
