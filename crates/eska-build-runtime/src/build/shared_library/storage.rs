//! A stable namespace lease protects derived files, including publication of prepared databases.

use super::{BuildError, Ibcmd, execute};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const OWNER: &[u8] = b"eska-library-build-v1\n";

pub(super) struct Lease {
    pub root: PathBuf,
    _lock: File,
}

impl Lease {
    pub fn acquire(root: PathBuf, ibcmd: &Ibcmd) -> Result<Self, BuildError> {
        let parent = root
            .parent()
            .ok_or_else(|| error(&root, io::Error::from(io::ErrorKind::InvalidInput)))?;
        fs::create_dir_all(parent).map_err(|source| error(parent, source))?;
        let path = root.with_extension("lock");
        execute::validate_existing_output(&path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| error(&path, source))?;
        loop {
            super::check_cancel(ibcmd)?;
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(20)),
                Err(TryLockError::Error(source)) => return Err(error(&path, source)),
            }
        }
        if !owned(&root)? {
            create_owned(&root)?;
        }
        Ok(Self { root, _lock: lock })
    }
}

pub(super) fn owned(path: &Path) -> Result<bool, BuildError> {
    match fs::symlink_metadata(path) {
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(error(path, source)),
        Ok(metadata) if metadata.file_type().is_dir() => {
            let marker = path.join("owner");
            if fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_file())
                && fs::read(&marker).ok().as_deref() == Some(OWNER)
            {
                Ok(true)
            } else {
                Err(BuildError::LibraryCacheUnowned(path.to_owned()))
            }
        }
        Ok(_) => Err(BuildError::LibraryCacheUnowned(path.to_owned())),
    }
}

pub(super) fn create_owned(path: &Path) -> Result<(), BuildError> {
    fs::create_dir(path).map_err(|source| error(path, source))?;
    let marker = path.join("owner");
    fs::write(&marker, OWNER).map_err(|source| error(&marker, source))
}

pub(super) fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    if !fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_file() && metadata.len() <= 64 * 1024)
    {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(super) fn write(path: &Path, value: &impl Serialize) -> Result<(), BuildError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|source| error(path, io::Error::other(source)))?;
    fs::write(path, bytes).map_err(|source| error(path, source))
}

pub(super) fn error(path: &Path, source: io::Error) -> BuildError {
    BuildError::BaseConfigurationCache {
        path: path.to_owned(),
        source,
    }
}

/// Both directories stay on the cache filesystem; readers retain the namespace lease.
pub(super) fn publish_directory(
    staged: &Path,
    target: &Path,
    backup: &Path,
) -> Result<(), BuildError> {
    let previous = owned(target)?;
    if previous {
        fs::rename(target, backup).map_err(|source| error(target, source))?;
    }
    if let Err(source) = fs::rename(staged, target) {
        if previous && let Err(source) = fs::rename(backup, target) {
            return Err(BuildError::Restore {
                path: target.to_owned(),
                source,
            });
        }
        return Err(error(target, source));
    }
    Ok(())
}
