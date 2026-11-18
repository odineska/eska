//! Revision-checked edits of the project manifest and root Git policy files.

mod manifest;

use std::{
    fs::{self, File, Metadata},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::{config::FILE_NAME, formatting::protocol::source_sha256};

use super::source_lock::SourceLease;

pub use manifest::Edit;

const MAX_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum SettingsFile {
    Manifest,
    Ignore,
    Attributes,
}

impl SettingsFile {
    const fn name(self) -> &'static str {
        match self {
            Self::Manifest => FILE_NAME,
            Self::Ignore => ".gitignore",
            Self::Attributes => ".gitattributes",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    MissingProject,
    Io(io::Error),
    UnsafePath,
    TooLarge,
    InvalidRequest,
    InvalidManifest,
    InvalidField(String),
    Manifest(Box<super::discovery::ContextDiscoveryError>),
    Formatting(crate::formatting::Error),
    Changed,
}

#[derive(Debug)]
pub struct Snapshot {
    pub revision: Option<String>,
    pub value: serde_json::Value,
}

pub enum Update {
    Manifest(Vec<Edit>),
    Text(String),
}

#[derive(Debug)]
pub struct Preview {
    root: PathBuf,
    file: SettingsFile,
    snapshot: Snapshot,
    before: String,
    after: String,
    analyzer_policy_changed: bool,
}

impl Preview {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn before(&self) -> &str {
        &self.before
    }

    #[must_use]
    pub fn after(&self) -> &str {
        &self.after
    }

    #[must_use]
    pub fn changed(&self) -> bool {
        self.before != self.after
    }

    #[must_use]
    pub const fn analyzer_policy_changed(&self) -> bool {
        self.analyzer_policy_changed
    }

    #[must_use]
    pub fn into_snapshot(self) -> Snapshot {
        self.snapshot
    }
}

/// Read the selected file from the exact directory containing the project manifest.
///
/// # Errors
/// Rejects missing projects, unsafe paths and unreadable files.
pub fn show(directory: &Path, file: SettingsFile) -> Result<Snapshot, Error> {
    let root = root(directory)?;
    let path = root.join(file.name());
    let raw = Raw::read(&path)?;
    let value = value(file, &raw.text)?;
    Ok(Snapshot {
        revision: raw.revision,
        value,
    })
}

/// Return the authored formatting policy of the exact member's workspace root, if available.
#[must_use]
pub fn inherited_formatting(directory: &Path) -> Option<serde_json::Value> {
    use super::discovery::{self, DiscoveryContext};

    let root = root(directory).ok()?;
    let DiscoveryContext::Workspace {
        workspace,
        current_member: Some(_),
    } = discovery::discover_context(&root).ok()?
    else {
        return None;
    };
    let manifest = Raw::read(&workspace.root().join(FILE_NAME)).ok()?;
    let mut value = manifest::json(&manifest.text).ok()?;
    value.as_object_mut()?.remove("format")
}

/// Apply an update in memory and validate the candidate at the original project root.
///
/// # Errors
/// Rejects stale revisions, invalid content and the same filesystem failures as `show`.
pub fn preview(
    directory: &Path,
    file: SettingsFile,
    revision: Option<&str>,
    update: Update,
) -> Result<Preview, Error> {
    let root = root(directory)?;
    let path = root.join(file.name());
    let raw = Raw::read(&path)?;
    plan(&root, file, revision, raw, update)
}

/// Atomically replace a file after rechecking the revision under the project lease.
///
/// # Errors
/// Returns validation, lock or filesystem errors without publishing a partial file.
pub fn save(preview: Preview) -> Result<Snapshot, Error> {
    let root = root(&preview.root)?;
    let path = root.join(preview.file.name());
    if !preview.changed() {
        if Raw::read(&path)?.revision != preview.snapshot.revision {
            return Err(Error::Changed);
        }
        return Ok(preview.into_snapshot());
    }
    // The manifest always exists and gives us a stable lease key even for missing Git policy files.
    let lease_path = root.join(FILE_NAME);
    regular_file(&lease_path)?.ok_or(Error::MissingProject)?;
    let _lease =
        SourceLease::acquire(&root, &[lease_path]).map_err(|error| match error.kind() {
            io::ErrorKind::WouldBlock => Error::Changed,
            io::ErrorKind::InvalidInput => Error::UnsafePath,
            _ => Error::Io(error),
        })?;

    let current = Raw::read(&path)?;
    if current.revision != preview.snapshot.revision {
        return Err(Error::Changed);
    }
    if matches!(preview.file, SettingsFile::Manifest) {
        manifest::validate(&root, &preview.after)?;
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&root).map_err(Error::Io)?;
    if let Some(metadata) = regular_file(&path)? {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(Error::Io)?;
    }
    temporary
        .write_all(preview.after.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(Error::Io)?;
    if Raw::read(&path)?.revision != current.revision {
        return Err(Error::Changed);
    }
    if current.revision.is_some() {
        temporary.persist(&path)
    } else {
        temporary.persist_noclobber(&path)
    }
    .map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            Error::Changed
        } else {
            Error::Io(error.error)
        }
    })?;
    Ok(Snapshot {
        revision: Some(source_sha256(&preview.after)),
        value: preview.snapshot.value,
    })
}

fn plan(
    root: &Path,
    file: SettingsFile,
    expected_revision: Option<&str>,
    raw: Raw,
    update: Update,
) -> Result<Preview, Error> {
    if raw.revision.as_deref() != expected_revision {
        return Err(Error::Changed);
    }
    let after = match (file, update) {
        (SettingsFile::Manifest, Update::Manifest(edits)) => {
            let after = manifest::apply(&raw.text, edits)?;
            manifest::validate(root, &after)?;
            after
        }
        (SettingsFile::Ignore | SettingsFile::Attributes, Update::Text(text)) => {
            if text.contains('\0') {
                return Err(Error::InvalidRequest);
            }
            text
        }
        _ => return Err(Error::InvalidRequest),
    };
    if after.len() > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let candidate = value(file, &after)?;
    let analyzer_policy_changed = matches!(file, SettingsFile::Manifest)
        && after != raw.text
        && manifest::json(&raw.text)?.pointer("/tools/bsl-analyzer")
            != candidate.pointer("/tools/bsl-analyzer");
    Ok(Preview {
        root: root.to_path_buf(),
        file,
        snapshot: Snapshot {
            revision: raw.revision,
            value: candidate,
        },
        before: raw.text,
        after,
        analyzer_policy_changed,
    })
}

fn value(file: SettingsFile, text: &str) -> Result<serde_json::Value, Error> {
    match file {
        SettingsFile::Manifest => manifest::json(text),
        SettingsFile::Ignore | SettingsFile::Attributes => Ok(text.into()),
    }
}

fn root(directory: &Path) -> Result<PathBuf, Error> {
    // Check the user-supplied path before canonicalization would hide a symlink.
    for part in directory.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if part.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(part).map_err(Error::Io)?;
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(Error::UnsafePath);
        }
    }
    let root = fs::canonicalize(directory).map_err(Error::Io)?;
    regular_file(&root.join(FILE_NAME))?.ok_or(Error::MissingProject)?;
    Ok(root)
}

fn regular_file(path: &Path) -> Result<Option<Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => Ok(Some(metadata)),
        Ok(_) => Err(Error::UnsafePath),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

struct Raw {
    text: String,
    revision: Option<String>,
}

impl Raw {
    fn read(path: &Path) -> Result<Self, Error> {
        let Some(metadata) = regular_file(path)? else {
            return Ok(Self {
                text: String::new(),
                revision: None,
            });
        };
        if metadata.len() > MAX_BYTES as u64 {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(Error::Io)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(Error::Io)?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        let text = String::from_utf8(bytes).map_err(|_| Error::InvalidRequest)?;
        let revision = Some(source_sha256(&text));
        Ok(Self { text, revision })
    }
}
