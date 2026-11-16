//! Recoverable binary/text creation, replacement and deletion under the existing metadata lock.

mod recovery;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{
    metadata_edit::{EditError, publish_binary_in},
    metadata_rename::transaction::{Guard, RecoveryStatus, finish, io},
};

const DIRECTORY: &str = "objects";
const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Byte plans are backend-owned; clients submit semantic requests and snapshots instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// Empty payload folders are part of native Designer source identity too.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryChange {
    pub path: PathBuf,
    pub before: bool,
    pub after: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredFile {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    source: PathBuf,
    files: Vec<StoredFile>,
    directories: Vec<DirectoryChange>,
}

/// Source snapshots use the same SHA-256 representation for binary and textual resources.
pub(crate) fn digest(bytes: &[u8]) -> String {
    super::metadata_edit::snapshot_bytes(bytes)
}

impl Guard {
    /// Stage every original and candidate before the first mutation; interrupted writes remain recoverable.
    pub(crate) fn publish_files(
        &self,
        source: &Path,
        files: &[FileChange],
        directories: &[DirectoryChange],
    ) -> Result<(), EditError> {
        self.check_clear(source)?;
        preflight(source, files, directories)?;
        if files.is_empty() && directories.is_empty() {
            return Ok(());
        }
        let staging = io::path(self.journal_root(), Path::new(DIRECTORY))?;
        fs::create_dir(&staging).map_err(EditError::Io)?;
        let journal = match prepare(source, files, directories, &staging) {
            Ok(journal) => journal,
            Err(error) => {
                cleanup(&staging)?;
                return Err(error);
            }
        };
        if let Err(error) = commit(&journal, &staging).and_then(|()| finish(&staging, b"committed"))
        {
            if recovery::rollback(&journal, &staging).is_err() {
                return Err(EditError::RecoveryRequired);
            }
            finish(&staging, b"restored")?;
            cleanup(&staging)?;
            return Err(error);
        }
        // Once committed, cleanup cannot turn success into a retryable source mutation.
        let _ = cleanup(&staging);
        Ok(())
    }
}

fn preflight(
    source: &Path,
    files: &[FileChange],
    directories: &[DirectoryChange],
) -> Result<(), EditError> {
    let mut paths = BTreeSet::new();
    for file in files {
        if !paths.insert(&file.path) || file.before.is_none() && file.after.is_none() {
            return Err(EditError::InvalidValue);
        }
        for bytes in [&file.before, &file.after].into_iter().flatten() {
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(EditError::UnsupportedValue);
            }
        }
        let path = io::planned_path(source, &file.path)?;
        if current(&path)? != file.before.as_deref().map(digest) {
            return Err(EditError::Conflict);
        }
        if file.before.is_some()
            && fs::metadata(path)
                .map_err(EditError::Io)?
                .permissions()
                .readonly()
        {
            return Err(EditError::ReadOnly);
        }
    }
    for directory in directories {
        if !paths.insert(&directory.path) {
            return Err(EditError::InvalidValue);
        }
        if directory_exists(&io::planned_path(source, &directory.path)?)? != directory.before {
            return Err(EditError::Conflict);
        }
    }
    Ok(())
}

fn prepare(
    source: &Path,
    files: &[FileChange],
    directories: &[DirectoryChange],
    staging: &Path,
) -> Result<Journal, EditError> {
    let journal = Journal {
        schema_version: 1,
        source: source.to_owned(),
        files: files
            .iter()
            .map(|file| StoredFile {
                path: file.path.clone(),
                before: file.before.as_deref().map(digest),
                after: file.after.as_deref().map(digest),
            })
            .collect(),
        directories: directories.to_vec(),
    };
    let encoded = serde_json::to_vec(&journal).map_err(|_| EditError::InvalidValue)?;
    if encoded.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(EditError::UnsupportedValue);
    }
    // The intent is durable before any staged payload exists; cleanup has an exact allowlist.
    io::create(&staging.join("journal.json"), &encoded)?;
    for (index, file) in files.iter().enumerate() {
        let permissions = file
            .before
            .as_ref()
            .map(|_| {
                fs::metadata(io::planned_path(source, &file.path)?)
                    .map(|value| value.permissions())
                    .map_err(EditError::Io)
            })
            .transpose()?;
        for (suffix, bytes) in [("before", &file.before), ("after", &file.after)] {
            if let Some(bytes) = bytes {
                let path = staging.join(format!("{index}.{suffix}"));
                io::create(&path, bytes)?;
                if let Some(permissions) = &permissions {
                    fs::set_permissions(&path, permissions.clone()).map_err(EditError::Io)?;
                }
            }
        }
    }
    io::create(&staging.join("ready"), b"1")?;
    Ok(journal)
}

fn commit(journal: &Journal, staging: &Path) -> Result<(), EditError> {
    let mut directories: Vec<_> = journal.directories.iter().collect();
    directories.sort_by_key(|entry| entry.path.components().count());
    for entry in &directories {
        if !entry.before && entry.after {
            fs::create_dir(io::planned_path(&journal.source, &entry.path)?)
                .map_err(publication_error)?;
        }
    }
    for (index, file) in journal.files.iter().enumerate() {
        write_file(journal, staging, index, file, false)?;
    }
    for entry in directories.iter().rev() {
        if entry.before && !entry.after {
            fs::remove_dir(io::planned_path(&journal.source, &entry.path)?)
                .map_err(EditError::Io)?;
        }
    }
    Ok(())
}

fn write_file(
    journal: &Journal,
    staging: &Path,
    index: usize,
    file: &StoredFile,
    reverse: bool,
) -> Result<(), EditError> {
    let (before, after, suffix) = if reverse {
        (&file.after, &file.before, "before")
    } else {
        (&file.before, &file.after, "after")
    };
    let path = io::planned_path(&journal.source, &file.path)?;
    let actual = current(&path)?;
    if actual == *after {
        return Ok(());
    }
    if actual != *before {
        return Err(EditError::Conflict);
    }
    if before.is_some()
        && fs::metadata(&path)
            .map_err(EditError::Io)?
            .permissions()
            .readonly()
    {
        return Err(EditError::ReadOnly);
    }
    let candidate = staging.join(format!("{index}.{suffix}"));
    match (before, after) {
        (None, Some(_)) => {
            if current(&candidate)? != *after {
                return Err(EditError::RecoveryRequired);
            }
            create_from_staged(&candidate, &path, staging)
        }
        (Some(_), None) => fs::remove_file(path).map_err(EditError::Io),
        (Some(_), Some(_)) => {
            let original = data(
                staging,
                index,
                if reverse { "after" } else { "before" },
                before.as_deref(),
            )?;
            let updated = data(staging, index, suffix, after.as_deref())?;
            publish_binary_in(&journal.source, &file.path, &original, &updated, staging)
        }
        (None, None) => Err(EditError::InvalidValue),
    }
}

/// Keep journal blobs immutable: the live source may be edited in place after publication.
fn create_from_staged(
    candidate: &Path,
    destination: &Path,
    staging: &Path,
) -> Result<(), EditError> {
    let temporary = io::path(staging, Path::new("publish.tmp"))?;
    let result = (|| {
        let mut input = fs::File::open(candidate).map_err(EditError::Io)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(EditError::Io)?;
        std::io::copy(&mut input, &mut output).map_err(EditError::Io)?;
        output
            .set_permissions(input.metadata().map_err(EditError::Io)?.permissions())
            .map_err(EditError::Io)?;
        output.sync_all().map_err(EditError::Io)?;
        drop(output);
        // Hard-link creation is atomic and refuses to replace a concurrent external file.
        fs::hard_link(&temporary, destination).map_err(publication_error)
    })();
    if temporary.try_exists().map_err(EditError::Io)? {
        fs::remove_file(&temporary).map_err(EditError::Io)?;
    }
    result
}

fn publication_error(error: std::io::Error) -> EditError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        EditError::Conflict
    } else {
        EditError::Io(error)
    }
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>, EditError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(EditError::Io)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(EditError::Io)?;
    if bytes.len() as u64 > limit {
        return Err(EditError::RecoveryRequired);
    }
    Ok(bytes)
}

fn current(path: &Path) -> Result<Option<String>, EditError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            Ok(Some(digest(&read(path, MAX_FILE_BYTES)?)))
        }
        Ok(_) => Err(EditError::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(EditError::Io(error)),
    }
}

fn directory_exists(path: &Path) -> Result<bool, EditError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(EditError::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(EditError::Io(error)),
    }
}

fn data(
    staging: &Path,
    index: usize,
    suffix: &str,
    expected: Option<&str>,
) -> Result<Vec<u8>, EditError> {
    let path = io::path(staging, Path::new(&format!("{index}.{suffix}")))?;
    let bytes = read(&path, MAX_FILE_BYTES)?;
    if expected != Some(digest(&bytes).as_str()) {
        return Err(EditError::RecoveryRequired);
    }
    Ok(bytes)
}

fn cleanup(staging: &Path) -> Result<(), EditError> {
    let manifest = io::path(staging, Path::new("journal.json"))?;
    let files = if manifest.try_exists().map_err(EditError::Io)? {
        serde_json::from_slice::<Journal>(&read(&manifest, MAX_JOURNAL_BYTES)?)
            .map_or(0, |journal| journal.files.len())
    } else {
        0
    };
    for entry in fs::read_dir(staging).map_err(EditError::Io)? {
        let entry = entry.map_err(EditError::Io)?;
        let name = entry.file_name();
        let name = name.to_str().ok_or(EditError::RecoveryRequired)?;
        let owned = matches!(
            name,
            "journal.json" | "ready" | "finished" | "finish.tmp" | "publish.tmp"
        ) || name.split_once('.').is_some_and(|(index, suffix)| {
            index.parse::<usize>().is_ok_and(|index| index < files)
                && matches!(suffix, "before" | "after")
        });
        if !owned || !entry.file_type().map_err(EditError::Io)?.is_file() {
            return Err(EditError::RecoveryRequired);
        }
    }
    let mut entries = fs::read_dir(staging)
        .map_err(EditError::Io)?
        .map(|entry| entry.map(|entry| entry.path()).map_err(EditError::Io))
        .collect::<Result<Vec<_>, _>>()?;
    // A final marker survives partial cleanup; its manifest remains until all payloads are gone.
    entries.sort_by_key(
        |path| match path.file_name().and_then(|name| name.to_str()) {
            Some("finished") => 3,
            Some("journal.json") => 2,
            Some("ready") => 1,
            _ => 0,
        },
    );
    for path in entries {
        fs::remove_file(path).map_err(EditError::Io)?;
    }
    fs::remove_dir(staging).map_err(EditError::Io)
}

pub(crate) use recovery::{check_clear, recover, recovery_status};
