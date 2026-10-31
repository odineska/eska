//! Immutable Designer XML copying and digesting without project or artifact policy.

use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

#[derive(Debug)]
pub(super) enum Error {
    Io { path: PathBuf, source: io::Error },
    UnsupportedEntry(PathBuf),
}

/// Copy exact source bytes and retain the existing version-one snapshot digest encoding.
///
/// # Errors
/// Rejects failed I/O and non-regular entries instead of following links outside the source.
pub(super) fn capture(source: &Path, target: &Path, excluded: &[&Path]) -> Result<Vec<u8>, Error> {
    let mut hash = Sha256::new();
    copy_directory(source, target, source, excluded, &mut hash)?;
    Ok(hash.finalize().to_vec())
}

/// Copy one directory without following links and hash its exact relative structure and bytes.
fn copy_directory(
    source: &Path,
    target: &Path,
    root: &Path,
    excluded: &[&Path],
    hash: &mut Sha256,
) -> Result<(), Error> {
    fs::create_dir(target).map_err(|source| Error::Io {
        path: target.to_owned(),
        source,
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|source_error| Error::Io {
            path: source.to_owned(),
            source: source_error,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source_error| Error::Io {
            path: source.to_owned(),
            source: source_error,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if entry.file_name() == OsStr::new(".git")
            || excluded
                .iter()
                .any(|excluded| path == **excluded || path.starts_with(excluded))
        {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("directory entries remain below the source root");
        let destination = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            hash_entry(hash, b'd', relative, &[]);
            copy_directory(&path, &destination, root, excluded, hash)?;
            copy_permissions(&path, &destination)?;
        } else if file_type.is_file() {
            let digest = copy_file(&path, &destination)?;
            hash_entry(hash, b'f', relative, &digest);
        } else {
            return Err(Error::UnsupportedEntry(path));
        }
    }
    copy_permissions(source, target)
}

/// Stream one file once so copied bytes and their checksum cannot diverge.
fn copy_file(source: &Path, target: &Path) -> Result<Vec<u8>, Error> {
    let mut input = File::open(source).map_err(|error| Error::Io {
        path: source.to_owned(),
        source: error,
    })?;
    let mut output = File::create(target).map_err(|error| Error::Io {
        path: target.to_owned(),
        source: error,
    })?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = input.read(&mut buffer).map_err(|error| Error::Io {
            path: source.to_owned(),
            source: error,
        })?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| Error::Io {
                path: target.to_owned(),
                source: error,
            })?;
        hash.update(&buffer[..read]);
    }
    copy_permissions(source, target)?;
    Ok(hash.finalize().to_vec())
}

/// Preserve permissions only after a copied directory has been populated.
fn copy_permissions(source: &Path, target: &Path) -> Result<(), Error> {
    let permissions = fs::metadata(source)
        .map_err(|error| Error::Io {
            path: source.to_owned(),
            source: error,
        })?
        .permissions();
    fs::set_permissions(target, permissions).map_err(|error| Error::Io {
        path: target.to_owned(),
        source: error,
    })
}

/// Hash an unambiguous entry record independent of traversal buffering.
fn hash_entry(hash: &mut Sha256, kind: u8, path: &Path, digest: &[u8]) {
    let encoded = path_bytes(path);
    hash.update([kind]);
    hash.update((encoded.len() as u64).to_le_bytes());
    hash.update(encoded);
    hash.update((digest.len() as u64).to_le_bytes());
    hash.update(digest);
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}
