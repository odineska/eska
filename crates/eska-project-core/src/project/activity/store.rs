//! Atomic local activity files use an OS-released lock and exact-byte compare-and-swap.

use super::{Error, MAX_STATE_BYTES, State, digest, path_key};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: State,
    pub revision: String,
}

/// The caller selects private machine storage; loading missing state never creates files.
#[derive(Debug)]
pub struct Store {
    directory: PathBuf,
}

impl Store {
    /// Namespace a canonical workspace independently from source/project/group names.
    ///
    /// # Errors
    /// Rejects relative machine directories and workspace roots.
    pub fn new(base: &Path, workspace: &Path) -> Result<Self, Error> {
        if !base.is_absolute() || !workspace.is_absolute() {
            return Err(Error::UnsafePath);
        }
        Ok(Self {
            directory: base.join(path_key(workspace)),
        })
    }

    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.directory.join("activity.json")
    }

    /// Read one bounded snapshot, preserving damaged state rather than silently enabling projects.
    ///
    /// # Errors
    /// Rejects links, invalid schemas and IO or size failures.
    pub fn load(&self) -> Result<Snapshot, Error> {
        if !checked_directory(&self.directory, false)? {
            return Ok(empty());
        }
        let path = self.path();
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
            Err(error) => return Err(error.into()),
            Ok(metadata) if metadata.is_symlink() || !metadata.is_file() => {
                return Err(Error::UnsafePath);
            }
            Ok(metadata) if metadata.len() > MAX_STATE_BYTES => return Err(Error::ResourceLimit),
            Ok(_) => {}
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_STATE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Error::ResourceLimit);
        }
        let state: State = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        if !state.valid() {
            return Err(Error::Invalid);
        }
        Ok(Snapshot {
            state,
            revision: digest(&bytes),
        })
    }

    /// Save only against the exact acknowledged snapshot; all fields change as one file.
    ///
    /// # Errors
    /// Preserves the previous state on conflicts, contention, invalid input and write failures.
    pub fn save(&self, expected: &str, state: &State) -> Result<Snapshot, Error> {
        if !state.valid() {
            return Err(Error::Invalid);
        }
        let mut bytes = serde_json::to_vec(state).map_err(|_| Error::Invalid)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Error::ResourceLimit);
        }
        checked_directory(&self.directory, true)?;
        let lock_path = self.directory.join("activity.lock");
        if fs::symlink_metadata(&lock_path)
            .is_ok_and(|metadata| metadata.is_symlink() || !metadata.is_file())
        {
            return Err(Error::UnsafePath);
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = Lock(options.open(lock_path)?);
        lock.0.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Busy,
            TryLockError::Error(error) => Error::Io(error),
        })?;
        let current = self.load()?;
        if current.revision != expected {
            return Err(Error::Conflict);
        }
        if current.state == *state {
            return Ok(current);
        }
        let temporary = Temporary::new(&self.directory, &bytes)?;
        fs::rename(&temporary.0, self.path())?;
        Ok(Snapshot {
            state: state.clone(),
            revision: digest(&bytes),
        })
    }
}

fn empty() -> Snapshot {
    Snapshot {
        state: State::default(),
        revision: digest(&[]),
    }
}

fn checked_directory(path: &Path, create: bool) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_symlink() && metadata.is_dir() => Ok(true),
        Ok(_) => Err(Error::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path.parent().ok_or(Error::UnsafePath)?)?;
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            checked_directory(path, false)
        }
        Err(error) => Err(error.into()),
    }
}

struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
struct Temporary(PathBuf);
impl Temporary {
    fn new(directory: &Path, bytes: &[u8]) -> Result<Self, Error> {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0..128 {
            let path = directory.join(format!(
                ".activity-{}-{time}-{attempt}.tmp",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            let temporary = Self(path);
            file.write_all(bytes)?;
            file.sync_all()?;
            return Ok(temporary);
        }
        Err(Error::Busy)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
