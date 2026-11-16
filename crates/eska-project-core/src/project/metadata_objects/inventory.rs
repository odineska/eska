//! Source fingerprints include absence and empty directories, not mtimes or client-supplied patches.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{ObjectError, ObjectPlan, ObjectRequest, invalid};
use crate::project::{
    metadata_edit::EditError,
    metadata_rename::inventory::{Inventory, MAX_XML_BYTES, file_hash},
    metadata_write::{DirectoryChange, FileChange},
};

#[derive(Default)]
pub struct Inputs {
    pub files: BTreeMap<PathBuf, Option<Vec<u8>>>,
    pub directories: BTreeMap<PathBuf, bool>,
    pub observed: BTreeMap<PathBuf, String>,
    inventories: BTreeMap<PathBuf, InventoryCheck>,
}

#[derive(Clone, Debug)]
struct InventoryCheck {
    excluded: Vec<PathBuf>,
    files: Vec<PathBuf>,
    directories: BTreeSet<PathBuf>,
}

#[derive(Debug, Default)]
pub struct InputChecks {
    files: BTreeMap<PathBuf, Option<String>>,
    directories: BTreeMap<PathBuf, bool>,
    inventories: BTreeMap<PathBuf, InventoryCheck>,
}

impl InputChecks {
    /// Retain hashes instead of a second copy of potentially large binary payloads.
    pub fn capture(inputs: &Inputs) -> Result<Self, ObjectError> {
        let mut files: BTreeMap<_, _> = inputs
            .files
            .iter()
            .map(|(path, contents)| {
                (
                    path.clone(),
                    contents
                        .as_deref()
                        .map(crate::project::metadata_edit::snapshot_bytes),
                )
            })
            .collect();
        for (path, hash) in &inputs.observed {
            if files
                .get(path)
                .is_some_and(|previous| previous.as_ref() != Some(hash))
            {
                return Err(EditError::Conflict.into());
            }
            files.insert(path.clone(), Some(hash.clone()));
        }
        Ok(Self {
            files,
            directories: inputs.directories.clone(),
            inventories: inputs.inventories.clone(),
        })
    }

    /// Check exactly the preview dependencies after acquiring their lease, without parsing XML/BSL.
    pub fn verify(&self, root: &Path) -> Result<(), ObjectError> {
        for (directory, expected) in &self.inventories {
            if !directory.as_os_str().is_empty() {
                checked_ancestry(root, directory)?;
            }
            let actual = Inventory::read(&root.join(directory), &expected.excluded)
                .map_err(|error| io(directory, error))?;
            if actual.files != expected.files || actual.directories != expected.directories {
                return Err(EditError::Conflict.into());
            }
        }
        for (path, expected) in &self.files {
            checked_ancestry(root, path)?;
            let hash = match file_hash(&root.join(path)) {
                Ok(hash) => Some(crate::project::metadata_edit::snapshot_digest(&hash)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(io(path, error)),
            };
            if hash != *expected {
                return Err(EditError::Conflict.into());
            }
        }
        for (path, expected) in &self.directories {
            checked_ancestry(root, path)?;
            let actual = match fs::symlink_metadata(root.join(path)) {
                Ok(info) if info.is_dir() => true,
                Ok(_) => return Err(EditError::Conflict.into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(io(path, error)),
            };
            if actual != *expected {
                return Err(EditError::Conflict.into());
            }
        }
        Ok(())
    }
}

impl Inputs {
    pub fn read(&mut self, root: &Path, path: &Path) -> Result<Option<Vec<u8>>, ObjectError> {
        if let Some(value) = self.files.get(path) {
            return Ok(value.clone());
        }
        checked_ancestry(root, path)?;
        let physical = root.join(path);
        let bytes = match fs::File::open(&physical) {
            Ok(file) => {
                if !file
                    .metadata()
                    .map_err(|source| io(path, source))?
                    .is_file()
                {
                    return Err(EditError::UnsafePath.into());
                }
                let mut bytes = Vec::new();
                file.take(MAX_XML_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|source| io(path, source))?;
                if bytes.len() as u64 > MAX_XML_BYTES {
                    return Err(invalid("source", "too_large"));
                }
                Some(bytes)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(io(path, error)),
        };
        self.files.insert(path.to_path_buf(), bytes.clone());
        Ok(bytes)
    }

    pub fn required(&mut self, root: &Path, path: &Path) -> Result<Vec<u8>, ObjectError> {
        self.read(root, path)?
            .ok_or_else(|| EditError::Conflict.into())
    }

    pub fn directory(&mut self, root: &Path, path: &Path) -> Result<bool, ObjectError> {
        checked_ancestry(root, path)?;
        let exists = match fs::symlink_metadata(root.join(path)) {
            Ok(info) if info.is_dir() => true,
            Ok(_) => return Err(EditError::UnsafePath.into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(io(path, error)),
        };
        self.directories.insert(path.to_path_buf(), exists);
        Ok(exists)
    }

    pub fn subtree(&mut self, root: &Path, directory: &Path) -> Result<Vec<PathBuf>, ObjectError> {
        if !self.directory(root, directory)? {
            return Ok(Vec::new());
        }
        let inventory =
            Inventory::read(&root.join(directory), &[]).map_err(|source| io(directory, source))?;
        self.observe_inventory(directory, &[], &inventory);
        for path in inventory.directories {
            self.directories.insert(directory.join(path), true);
        }
        let paths: Vec<_> = inventory
            .files
            .into_iter()
            .map(|path| directory.join(path))
            .collect();
        for path in &paths {
            self.required(root, path)?;
        }
        Ok(paths)
    }

    pub fn observe_inventory(
        &mut self,
        directory: &Path,
        excluded: &[PathBuf],
        inventory: &Inventory,
    ) {
        self.inventories.insert(
            directory.to_path_buf(),
            InventoryCheck {
                excluded: excluded.to_vec(),
                files: inventory.files.clone(),
                directories: inventory.directories.clone(),
            },
        );
    }

    pub fn snapshot(&self, request: &ObjectRequest) -> Result<String, ObjectError> {
        let mut digest = Sha256::new();
        digest
            .update(serde_json::to_vec(request).map_err(|_| invalid("request", "serialization"))?);
        for (path, contents) in &self.files {
            hash_file(&mut digest, path, contents.as_deref());
        }
        for (path, exists) in &self.directories {
            hash_directory(&mut digest, path, *exists);
        }
        for (path, hash) in &self.observed {
            digest.update(b"observed\0");
            digest.update(path.to_string_lossy().as_bytes());
            digest.update([0]);
            digest.update(hash.as_bytes());
        }
        for (root, inventory) in &self.inventories {
            digest.update(b"inventory\0");
            digest.update(root.to_string_lossy().as_bytes());
            for file in &inventory.files {
                digest.update([0]);
                digest.update(file.to_string_lossy().as_bytes());
            }
            for directory in &inventory.directories {
                digest.update([1]);
                digest.update(directory.to_string_lossy().as_bytes());
            }
        }
        Ok(crate::project::metadata_edit::snapshot_bytes(
            &digest.finalize(),
        ))
    }
}

/// The exact output token covers every affected byte, path and empty-directory transition.
pub fn output_snapshot(plan: &ObjectPlan) -> String {
    let mut digest = Sha256::new();
    for file in &plan.files {
        hash_file(&mut digest, &file.path, file.after.as_deref());
    }
    for directory in &plan.directories {
        hash_directory(&mut digest, &directory.path, directory.after);
    }
    crate::project::metadata_edit::snapshot_bytes(&digest.finalize())
}

fn hash_file(digest: &mut Sha256, path: &Path, contents: Option<&[u8]>) {
    digest.update(b"file\0");
    digest.update(path.to_string_lossy().as_bytes());
    digest.update([0]);
    if let Some(contents) = contents {
        digest.update([1]);
        digest.update(Sha256::digest(contents));
    } else {
        digest.update([0]);
    }
}

fn hash_directory(digest: &mut Sha256, path: &Path, exists: bool) {
    digest.update(b"directory\0");
    digest.update(path.to_string_lossy().as_bytes());
    digest.update([u8::from(exists)]);
}

/// Only lexical source-relative paths are accepted, including currently absent destinations.
pub fn checked_ancestry(root: &Path, path: &Path) -> Result<(), ObjectError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(EditError::UnsafePath.into());
    }
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(info) if info.file_type().is_symlink() => return Err(EditError::UnsafePath.into()),
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(io(path, error)),
        }
    }
    Ok(())
}

pub fn io(path: &Path, source: std::io::Error) -> ObjectError {
    ObjectError::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub fn text(bytes: &[u8]) -> Result<&str, ObjectError> {
    std::str::from_utf8(bytes).map_err(|_| EditError::InvalidXml.into())
}

/// Every destination parent is explicit, so an empty Ext directory survives copy and deletion.
pub fn target_directories(
    root: &Path,
    inputs: &mut Inputs,
    files: &[FileChange],
    additional: impl IntoIterator<Item = PathBuf>,
) -> Result<Vec<DirectoryChange>, ObjectError> {
    let additional: Vec<_> = additional.into_iter().collect();
    let mut paths = BTreeSet::new();
    for path in files
        .iter()
        .filter(|file| file.after.is_some())
        .map(|file| &file.path)
        .chain(additional.iter())
    {
        let mut current = path.parent();
        while let Some(path) = current.filter(|path| !path.as_os_str().is_empty()) {
            paths.insert(path.to_path_buf());
            current = path.parent();
        }
    }
    paths.extend(additional);
    paths
        .into_iter()
        .map(|path| {
            let before = inputs.directory(root, &path)?;
            Ok(DirectoryChange {
                path,
                before,
                after: true,
            })
        })
        .collect()
}
