//! Shared source leases explicitly unlock persistent inodes; OS close remains the crash fallback.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
};

use crate::file_lock::FileLock;

/// An owned handle is the write capability; filenames and numeric session IDs never grant it.
#[derive(Debug)]
pub struct SourceLease {
    _files: Vec<FileLock>,
    _gate: Option<FileLock>,
    root: PathBuf,
    names: Vec<String>,
}

impl SourceLease {
    /// Acquire every source in canonical order, releasing the partial set on any failure.
    ///
    /// # Errors
    /// Returns `WouldBlock` for a competing writer and IO errors for unsafe or unavailable lock storage.
    pub fn acquire(root: &Path, paths: &[PathBuf]) -> io::Result<Self> {
        let directory = directory(root, true)?;
        // Registration is serialized with batch readers; ordinary leases retain only their files.
        let _gate = gate(&directory, true)?;
        let names = names(paths)?;
        let mut files = Vec::new();
        for name in &names {
            let path = directory.join(name);
            let file = open(&path, true)?;
            files.push(lock(file)?);
        }
        Ok(Self {
            _files: files,
            _gate: None,
            root: fs::canonicalize(root)?,
            names,
        })
    }

    /// Protect a large dependency set without retaining one descriptor or creating a lock per input.
    ///
    /// # Errors
    /// Rejects an active input writer or a competing registration. Existing unrelated leases remain
    /// owned by their callers; new registrations are rejected until this batch capability is released.
    pub fn acquire_batch(root: &Path, paths: &[PathBuf]) -> io::Result<Self> {
        let directory = directory(root, true)?;
        let gate = gate(&directory, true)?;
        let names = names(paths)?;
        check_names(&directory, &names)?;
        Ok(Self {
            _files: Vec::new(),
            _gate: gate,
            root: fs::canonicalize(root)?,
            names,
        })
    }

    /// Check a dependency set under one short gate without creating persistent lock files.
    ///
    /// # Errors
    /// Returns the same ownership and storage errors as checking one source.
    pub fn check_many(root: &Path, paths: &[PathBuf]) -> io::Result<()> {
        let directory = directory(root, false)?;
        let _gate = gate(&directory, false)?;
        check_names(&directory, &names(paths)?)
    }

    /// A checked publisher may bypass only the exact source lease it already owns in this project.
    ///
    /// # Errors
    /// Returns filesystem failures while resolving either supplied identity.
    pub fn owns(&self, root: &Path, path: &Path) -> io::Result<bool> {
        Ok(fs::canonicalize(root)? == self.root && self.names.binary_search(&key(path)?).is_ok())
    }

    /// Check a known lock without creating a directory or persistent file for read-only previews.
    ///
    /// # Errors
    /// Returns `WouldBlock` while a competing query/writer owns this source; unsafe storage fails closed.
    pub fn check(root: &Path, path: &Path) -> io::Result<()> {
        let directory = directory(root, false)?;
        let _gate = gate(&directory, false)?;
        check_names(&directory, &[key(path)?])
    }
}

fn names(paths: &[PathBuf]) -> io::Result<Vec<String>> {
    let mut names = paths
        .iter()
        .map(|path| key(path))
        .collect::<io::Result<Vec<_>>>()?;
    names.sort();
    names.dedup();
    Ok(names)
}

fn check_names(directory: &Path, names: &[String]) -> io::Result<()> {
    for name in names {
        match open(&directory.join(name), false) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
            Ok(file) => drop(lock(file)?),
        }
    }
    Ok(())
}

fn gate(directory: &Path, create: bool) -> io::Result<Option<FileLock>> {
    match open(&directory.join("batch.lock"), create) {
        Err(error) if !create && error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(file) => lock(file).map(Some),
    }
}

/// Only canonical source identities share a lock; different path spellings cannot bypass an owner.
fn key(path: &Path) -> io::Result<String> {
    const HEX: &[u8] = b"0123456789abcdef";
    let canonical = fs::canonicalize(path)?;
    let digest = Sha256::digest(canonical.as_os_str().as_encoded_bytes());
    let hash = digest
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect::<String>();
    Ok(format!("{hash}.lock"))
}

/// Never traverse a foreign symlink when creating project-owned lock storage.
fn directory(root: &Path, create: bool) -> io::Result<PathBuf> {
    let mut current = fs::canonicalize(root)?;
    for name in [".eska", "source-locks"] {
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {}
            Ok(_) => return Err(io::Error::from(io::ErrorKind::InvalidInput)),
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        if !fs::symlink_metadata(&current)
                            .is_ok_and(|metadata| metadata.is_dir() && !metadata.is_symlink())
                        {
                            return Err(error);
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(current)
}

/// Empty regular lock files are durable coordination state, never disposable temporary paths.
fn open(path: &Path, create: bool) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.is_symlink() || metadata.len() != 0 => {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)
}

/// Only a successful acquisition owns an unlock; failed and competing handles never release another lease.
fn lock(file: File) -> io::Result<FileLock> {
    FileLock::try_lock(file).map_err(|error| match error {
        TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
        TryLockError::Error(error) => error,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::SourceLease;
    use crate::test_support::TestDir;
    use std::{fs, io};

    /// A duplicate open-file description models a descriptor inherited by a concurrent subprocess spawn.
    #[test]
    fn released_lease_does_not_wait_for_a_duplicate_descriptor_to_close() {
        let directory = TestDir::new();
        let source = directory.0.join("Module.bsl");
        fs::write(&source, "A=1;").unwrap();
        let lease = SourceLease::acquire(&directory.0, std::slice::from_ref(&source)).unwrap();
        let SourceLease { _files: files, .. } = &lease;
        let duplicate = files[0].duplicate().unwrap();
        drop(lease);
        let next = SourceLease::acquire(&directory.0, std::slice::from_ref(&source)).unwrap();
        drop(duplicate);
        assert_eq!(
            SourceLease::check(&directory.0, &source)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(next);
        SourceLease::check(&directory.0, &source).unwrap();
    }

    #[test]
    fn batch_rejects_an_active_input_and_releases_its_gate_after_failure() {
        let directory = TestDir::new();
        let source = directory.0.join("Module.bsl");
        fs::write(&source, "A=1;").unwrap();
        let active = SourceLease::acquire(&directory.0, std::slice::from_ref(&source)).unwrap();
        assert_eq!(
            SourceLease::acquire_batch(&directory.0, std::slice::from_ref(&source))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            SourceLease::check_many(&directory.0, std::slice::from_ref(&source))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(active);
        let batch =
            SourceLease::acquire_batch(&directory.0, std::slice::from_ref(&source)).unwrap();
        assert!(batch.owns(&directory.0, &source).unwrap());
        assert_eq!(
            SourceLease::acquire(&directory.0, std::slice::from_ref(&source))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            SourceLease::check(&directory.0, &source)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(batch);
        SourceLease::acquire(&directory.0, std::slice::from_ref(&source)).unwrap();
    }

    #[test]
    fn batch_retains_unrelated_owner_and_rejects_new_registrations() {
        let directory = TestDir::new();
        let input = directory.0.join("Input.bsl");
        let other = directory.0.join("Other.bsl");
        let new = directory.0.join("New.bsl");
        for source in [&input, &other, &new] {
            fs::write(source, "").unwrap();
        }
        let owner = SourceLease::acquire(&directory.0, std::slice::from_ref(&other)).unwrap();
        let batch = SourceLease::acquire_batch(&directory.0, std::slice::from_ref(&input)).unwrap();
        assert!(!batch.owns(&directory.0, &other).unwrap());
        assert!(owner.owns(&directory.0, &other).unwrap());
        assert_eq!(
            SourceLease::acquire_batch(&directory.0, std::slice::from_ref(&new))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            SourceLease::acquire(&directory.0, std::slice::from_ref(&new))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(batch);
        assert_eq!(
            SourceLease::check(&directory.0, &other).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(owner);
        SourceLease::check(&directory.0, &other).unwrap();
    }

    #[test]
    fn large_batch_creates_only_one_lock_file_and_retains_one_handle() {
        let directory = TestDir::new();
        let paths: Vec<_> = (0..1500)
            .map(|index| {
                let path = directory.0.join(format!("{index}.bsl"));
                fs::write(&path, "").unwrap();
                path
            })
            .collect();
        SourceLease::check_many(&directory.0, &paths).unwrap();
        assert!(!directory.0.join(".eska").exists());
        let batch = SourceLease::acquire_batch(&directory.0, &paths).unwrap();
        let SourceLease {
            _files: files,
            _gate: gate,
            ..
        } = &batch;
        assert!(
            files.is_empty(),
            "a large batch must not consume one FD per source"
        );
        assert!(gate.is_some());
        assert_eq!(
            fs::read_dir(directory.0.join(".eska/source-locks"))
                .unwrap()
                .count(),
            1
        );
        assert!(batch.owns(&directory.0, &paths[1499]).unwrap());
        drop(batch);
        SourceLease::check_many(&directory.0, &paths).unwrap();
    }
}
