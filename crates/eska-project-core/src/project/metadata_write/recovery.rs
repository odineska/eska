//! Recover only known before/after states; external edits always require review.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use super::{
    DIRECTORY, EditError, Guard, Journal, MAX_JOURNAL_BYTES, RecoveryStatus, cleanup, current,
    data, directory_exists, finish, io, read, write_file,
};

fn load(
    source: &Path,
    staging: &Path,
    validate_payloads: bool,
) -> Result<Option<Journal>, EditError> {
    if !io::path(staging, Path::new("ready"))?
        .try_exists()
        .map_err(EditError::Io)?
    {
        return Ok(None);
    }
    let encoded = read(
        &io::path(staging, Path::new("journal.json"))?,
        MAX_JOURNAL_BYTES,
    )?;
    let journal: Journal =
        serde_json::from_slice(&encoded).map_err(|_| EditError::RecoveryRequired)?;
    if journal.schema_version != 1 || journal.source != source {
        return Err(EditError::RecoveryRequired);
    }
    let mut paths = BTreeSet::new();
    for (index, file) in journal.files.iter().enumerate() {
        if validate_payloads {
            io::planned_path(source, &file.path)?;
        }
        if !paths.insert(&file.path) || file.before.is_none() && file.after.is_none() {
            return Err(EditError::RecoveryRequired);
        }
        for (suffix, expected) in [("before", &file.before), ("after", &file.after)] {
            if validate_payloads && expected.is_some() {
                data(staging, index, suffix, expected.as_deref())?;
            }
        }
    }
    for directory in &journal.directories {
        if validate_payloads {
            io::planned_path(source, &directory.path)?;
        }
        if !paths.insert(&directory.path) {
            return Err(EditError::RecoveryRequired);
        }
    }
    Ok(Some(journal))
}

pub fn recovery_status(guard: &Guard, source: &Path) -> Result<Option<RecoveryStatus>, EditError> {
    let staging = io::path(guard.journal_root(), Path::new(DIRECTORY))?;
    if !staging.try_exists().map_err(EditError::Io)? {
        return Ok(None);
    }
    let snapshot = io::tree_hash(guard.journal_root(), Path::new(DIRECTORY), true)?;
    let finished = io::path(&staging, Path::new("finished"))?;
    let state = if finished.try_exists().map_err(EditError::Io)? {
        read(&finished, 16)?
    } else {
        Vec::new()
    };
    let (committed, restored) = match state.as_slice() {
        b"" => (false, false),
        b"committed" => (true, false),
        b"restored" => (false, true),
        _ => return Err(EditError::RecoveryRequired),
    };
    let journal = load(source, &staging, !committed && !restored)?;
    let mut files = BTreeSet::new();
    let mut conflicts = Vec::new();
    if let Some(journal) = &journal {
        for file in &journal.files {
            files.insert(file.path.clone());
        }
        for directory in &journal.directories {
            files.insert(directory.path.clone());
        }
        if !committed && !restored {
            conflicts = rollback_conflicts(journal)?;
        }
    }
    Ok(Some(RecoveryStatus {
        pending: true,
        snapshot: Some(snapshot),
        committed,
        restored,
        files: files.into_iter().collect(),
        conflicts,
    }))
}

pub fn check_clear(guard: &Guard, source: &Path) -> Result<(), EditError> {
    if let Some(status) = recovery_status(guard, source)? {
        if !status.committed && !status.restored {
            return Err(EditError::RecoveryRequired);
        }
        recover(
            guard,
            source,
            status
                .snapshot
                .as_deref()
                .ok_or(EditError::RecoveryRequired)?,
        )?;
    }
    Ok(())
}

pub fn recover(guard: &Guard, source: &Path, expected: &str) -> Result<bool, EditError> {
    let Some(status) = recovery_status(guard, source)? else {
        return Ok(false);
    };
    if status.snapshot.as_deref() != Some(expected) || !status.conflicts.is_empty() {
        return Err(EditError::Conflict);
    }
    let staging = io::path(guard.journal_root(), Path::new(DIRECTORY))?;
    if !status.committed
        && !status.restored
        && let Some(journal) = load(source, &staging, true)?
    {
        rollback(&journal, &staging)?;
        finish(&staging, b"restored")?;
    }
    cleanup(&staging)?;
    Ok(true)
}

pub(super) fn rollback(journal: &Journal, staging: &Path) -> Result<(), EditError> {
    // Preflight all files and newly created folders before restoring even the first byte.
    if !rollback_conflicts(journal)?.is_empty() {
        return Err(EditError::Conflict);
    }
    let temporary = io::path(staging, Path::new("publish.tmp"))?;
    if temporary.try_exists().map_err(EditError::Io)? {
        fs::remove_file(temporary).map_err(EditError::Io)?;
    }
    let mut directories: Vec<_> = journal.directories.iter().collect();
    directories.sort_by_key(|entry| entry.path.components().count());
    for entry in &directories {
        let path = io::planned_path(&journal.source, &entry.path)?;
        if entry.before && !directory_exists(&path)? {
            fs::create_dir(path).map_err(EditError::Io)?;
        }
    }
    for (index, file) in journal.files.iter().enumerate().rev() {
        write_file(journal, staging, index, file, true)?;
    }
    for entry in directories.iter().rev() {
        let path = io::planned_path(&journal.source, &entry.path)?;
        if !entry.before && directory_exists(&path)? {
            fs::remove_dir(path).map_err(EditError::Io)?;
        }
    }
    Ok(())
}

fn rollback_conflicts(journal: &Journal) -> Result<Vec<PathBuf>, EditError> {
    let mut conflicts = BTreeSet::new();
    let owned: BTreeSet<_> = journal
        .files
        .iter()
        .map(|entry| &entry.path)
        .chain(journal.directories.iter().map(|entry| &entry.path))
        .collect();
    for file in &journal.files {
        let actual = current(&io::planned_path(&journal.source, &file.path)?)?;
        if actual != file.before && actual != file.after {
            conflicts.insert(file.path.clone());
        }
    }
    for directory in journal.directories.iter().filter(|entry| !entry.before) {
        let path = io::planned_path(&journal.source, &directory.path)?;
        if !directory_exists(&path)? {
            continue;
        }
        for child in fs::read_dir(path).map_err(EditError::Io)? {
            let child = child.map_err(EditError::Io)?;
            let relative = directory.path.join(child.file_name());
            if !owned.contains(&relative) {
                conflicts.insert(relative);
            }
        }
    }
    Ok(conflicts.into_iter().collect())
}
