use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{ArtifactType, BuildPlan, source_snapshot};
use eska_project_core::project::{Project, ProjectPathError, version};

#[derive(Debug)]
pub enum ManifestError {
    SnapshotIo { path: PathBuf, source: io::Error },
    SnapshotEntryUnsupported(PathBuf),
    SnapshotProject(ProjectPathError),
    ArtifactRead { path: PathBuf, source: io::Error },
    Write { path: PathBuf, source: io::Error },
    Serialize(serde_json::Error),
}

pub(super) struct ManifestSeed {
    project_name: Option<String>,
    git: GitProvenance,
}

impl ManifestSeed {
    /// Bind caller-supplied provenance to the immutable build seed without inspecting repositories.
    #[must_use]
    pub(super) fn capture(project_name: Option<&str>, git: GitProvenance) -> Self {
        Self {
            project_name: project_name.map(str::to_owned),
            git,
        }
    }
}

pub(super) struct SourceSnapshot {
    source: PathBuf,
    id: String,
    version: VersionDocument,
}

impl SourceSnapshot {
    #[must_use]
    pub(super) fn source(&self) -> &Path {
        &self.source
    }
}

/// Return the stable sibling path used for an artifact manifest.
#[must_use]
pub fn path_for_artifact(artifact: &Path) -> PathBuf {
    let mut name = artifact.file_name().unwrap_or_default().to_os_string();
    name.push(".manifest.json");
    artifact.with_file_name(name)
}

/// Copy the exact Designer source tree that will be passed to ibcmd and identify its bytes.
///
/// # Errors
/// Returns a structured error for unreadable entries, failed copies, unsupported links or an
/// invalid isolated project path.
pub(super) fn capture_source(
    project: &Project,
    plan: &BuildPlan,
    workspace: &Path,
) -> Result<SourceSnapshot, ManifestError> {
    let snapshot_root = workspace.join("snapshot");
    let snapshot_source = snapshot_root.join("source");
    fs::create_dir(&snapshot_root).map_err(|source| ManifestError::SnapshotIo {
        path: snapshot_root.clone(),
        source,
    })?;
    let manifest_path = path_for_artifact(plan.output());
    let excluded = [
        plan.artifacts_directory(),
        plan.output(),
        manifest_path.as_path(),
        workspace,
    ];
    let digest =
        source_snapshot::capture(plan.source(), &snapshot_source, &excluded).map_err(|error| {
            match error {
                source_snapshot::Error::Io { path, source } => {
                    ManifestError::SnapshotIo { path, source }
                }
                source_snapshot::Error::UnsupportedEntry(path) => {
                    ManifestError::SnapshotEntryUnsupported(path)
                }
            }
        })?;
    let snapshot_project = Project::new(
        snapshot_root,
        snapshot_source.clone(),
        project.configuration().clone(),
    )
    .map_err(ManifestError::SnapshotProject)?;
    let version = version::inspect(&snapshot_project).map_or_else(
        |_| VersionDocument::unavailable(),
        |info| VersionDocument::available(info.version.to_string()),
    );
    Ok(SourceSnapshot {
        source: snapshot_source,
        id: format!("sha256:{}", hex(&digest)),
        version,
    })
}

/// Serialize a manifest for the staged artifact and the source snapshot used to build it.
///
/// # Errors
/// Returns a structured error when the artifact cannot be read or JSON serialization fails.
pub(super) fn write(
    path: &Path,
    artifact: &Path,
    artifact_type: ArtifactType,
    platform_version: &str,
    seed: ManifestSeed,
    snapshot: &SourceSnapshot,
    base_configuration: Option<&Path>,
) -> Result<(), ManifestError> {
    let document = ArtifactManifestDocument {
        schema_version: 1,
        kind: "artifact-manifest",
        project: ProjectDocument {
            name: seed.project_name,
            version: snapshot.version.clone(),
        },
        artifact: ArtifactDocument {
            r#type: artifact_type.as_str(),
            checksum: ChecksumDocument::sha256(file_checksum(artifact)?),
        },
        platform: PlatformDocument {
            version: platform_version.to_owned(),
        },
        source: SourceDocument {
            snapshot_id: snapshot.id.clone(),
            git: seed.git,
            base_configuration: base_configuration
                .map(file_checksum)
                .transpose()?
                .map(ChecksumDocument::sha256),
        },
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(ManifestError::Serialize)?;
    let mut file = File::create(path).map_err(|source| ManifestError::Write {
        path: path.to_owned(),
        source,
    })?;
    file.write_all(&bytes)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|source| ManifestError::Write {
            path: path.to_owned(),
            source,
        })
}

/// Read and hash a completed artifact without loading it into memory.
fn file_checksum(path: &Path) -> Result<String, ManifestError> {
    let mut file = File::open(path).map_err(|source| ManifestError::ArtifactRead {
        path: path.to_owned(),
        source,
    })?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| ManifestError::ArtifactRead {
                path: path.to_owned(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hex(&hash.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[derive(Clone, Serialize)]
struct VersionDocument {
    status: &'static str,
    value: Option<String>,
}

impl VersionDocument {
    const fn available(value: String) -> Self {
        Self {
            status: "available",
            value: Some(value),
        }
    }

    const fn unavailable() -> Self {
        Self {
            status: "unavailable",
            value: None,
        }
    }
}

#[derive(Serialize)]
struct ArtifactManifestDocument {
    schema_version: u8,
    kind: &'static str,
    project: ProjectDocument,
    artifact: ArtifactDocument,
    platform: PlatformDocument,
    source: SourceDocument,
}

#[derive(Serialize)]
struct ProjectDocument {
    name: Option<String>,
    version: VersionDocument,
}

#[derive(Serialize)]
struct ArtifactDocument {
    r#type: &'static str,
    checksum: ChecksumDocument,
}

#[derive(Serialize)]
struct ChecksumDocument {
    algorithm: &'static str,
    value: String,
}

impl ChecksumDocument {
    const fn sha256(value: String) -> Self {
        Self {
            algorithm: "sha256",
            value,
        }
    }
}

#[derive(Serialize)]
struct PlatformDocument {
    version: String,
}

#[derive(Serialize)]
struct SourceDocument {
    snapshot_id: String,
    git: GitProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_configuration: Option<ChecksumDocument>,
}

pub use eska_project_core::provenance::GitProvenance;
