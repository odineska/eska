//! Immutable Designer XML copying and digesting without project or artifact policy.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

type Files = BTreeMap<PathBuf, Vec<u8>>;

#[derive(Debug)]
pub enum Error {
    Io { path: PathBuf, source: io::Error },
    UnsupportedEntry(PathBuf),
}

/// Copy exact source bytes and retain the existing version-one snapshot digest encoding.
///
/// # Errors
/// Rejects failed I/O and non-regular entries instead of following links outside the source.
pub(super) fn capture(source: &Path, target: &Path, excluded: &[&Path]) -> Result<Vec<u8>, Error> {
    let mut hash = Sha256::new();
    visit_directory(
        source,
        Some(target),
        source,
        excluded,
        &mut hash,
        &mut |_, _| {},
    )?;
    Ok(hash.finalize().to_vec())
}

/// Identify all current bytes without creating a copy or relying on metadata timestamps.
pub(super) fn digest(source: &Path, excluded: &[&Path]) -> Result<Vec<u8>, Error> {
    let mut hash = Sha256::new();
    visit_directory(source, None, source, excluded, &mut hash, &mut |_, _| {})?;
    Ok(hash.finalize().to_vec())
}

/// Capture individual files for runtime delta calculation, optionally from an immutable copy.
pub fn catalog(source: &Path, target: Option<&Path>, excluded: &[&Path]) -> Result<Files, Error> {
    catalog_with_digest(source, target, excluded).map(|(files, _)| files)
}

/// Materialize selected catalog entries and check the bytes that will be imported.
pub fn copy_selected<'a>(
    source: &Path,
    target: &Path,
    entries: impl IntoIterator<Item = (&'a Path, &'a [u8])>,
) -> Result<bool, Error> {
    create_directory(target)?;
    for (relative, expected) in entries {
        for directory in relative
            .ancestors()
            .skip(1)
            .take_while(|path| !path.as_os_str().is_empty())
        {
            if !is_entry(&source.join(directory), fs::Metadata::is_dir)? {
                return Ok(false);
            }
        }
        let input = source.join(relative);
        if !is_entry(&input, fs::Metadata::is_file)? {
            return Ok(false);
        }
        let output = target.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        match file_digest(&input, Some(&output)) {
            Ok(actual) if actual == expected => {}
            Ok(_) => return Ok(false),
            Err(Error::Io { path, source })
                if path == input && source.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn is_entry(path: &Path, is_kind: fn(&fs::Metadata) -> bool) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(is_kind(&metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn create_directory(path: &Path) -> Result<(), Error> {
    fs::create_dir(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

pub(super) fn catalog_with_digest(
    source: &Path,
    target: Option<&Path>,
    excluded: &[&Path],
) -> Result<(Files, Vec<u8>), Error> {
    let mut files = BTreeMap::new();
    let mut hash = Sha256::new();
    visit_directory(
        source,
        target,
        source,
        excluded,
        &mut hash,
        &mut |path, digest| {
            files.insert(path.to_owned(), digest.to_vec());
        },
    )?;
    Ok((files, hash.finalize().to_vec()))
}

/// Copy one directory without following links and hash its exact relative structure and bytes.
fn visit_directory(
    source: &Path,
    target: Option<&Path>,
    root: &Path,
    excluded: &[&Path],
    hash: &mut Sha256,
    on_file: &mut impl FnMut(&Path, &[u8]),
) -> Result<(), Error> {
    if let Some(target) = target {
        create_directory(target)?;
    }
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
        let destination = target.map(|target| target.join(entry.file_name()));
        let file_type = entry.file_type().map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            hash_entry(hash, b'd', relative, &[]);
            visit_directory(&path, destination.as_deref(), root, excluded, hash, on_file)?;
        } else if file_type.is_file() {
            let digest = file_digest(&path, destination.as_deref())?;
            hash_entry(hash, b'f', relative, &digest);
            on_file(relative, &digest);
        } else {
            return Err(Error::UnsupportedEntry(path));
        }
    }
    if let Some(target) = target {
        copy_permissions(source, target)?;
    }
    Ok(())
}

/// Stream one file once so copied bytes and their checksum cannot diverge.
pub(super) fn file_digest(source: &Path, target: Option<&Path>) -> Result<Vec<u8>, Error> {
    let mut input = File::open(source).map_err(|error| Error::Io {
        path: source.to_owned(),
        source: error,
    })?;
    let mut output = target
        .map(|path| {
            File::create(path).map_err(|source| Error::Io {
                path: path.to_owned(),
                source,
            })
        })
        .transpose()?;
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
        if let Some((output, path)) = output.as_mut().zip(target) {
            output
                .write_all(&buffer[..read])
                .map_err(|source| Error::Io {
                    path: path.to_owned(),
                    source,
                })?;
        }
        hash.update(&buffer[..read]);
    }
    if let Some(target) = target {
        copy_permissions(source, target)?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_copy_rejects_bytes_changed_since_catalog_capture() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let name = Path::new("Module.bsl");
        let module = source.join(name);
        fs::write(&module, "A = 1;").unwrap();
        let expected = file_digest(&module, None).unwrap();
        fs::write(&module, "A = 2;").unwrap();

        let selected = [(name, expected.as_slice())];
        assert!(!copy_selected(&source, &temp.path().join("stale"), selected).unwrap());

        fs::write(&module, "A = 1;").unwrap();
        let staged = temp.path().join("staged");
        assert!(copy_selected(&source, &staged, selected).unwrap());
        assert_eq!(fs::read(staged.join(name)).unwrap(), b"A = 1;");
    }

    #[cfg(unix)]
    #[test]
    fn selected_copy_rejects_a_link_replacing_the_source_directory() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let modules = source.join("modules");
        fs::create_dir_all(&modules).unwrap();
        let relative = Path::new("modules/Module.bsl");
        let module = source.join(relative);
        fs::write(&module, "A = 1;").unwrap();
        let expected = file_digest(&module, None).unwrap();
        let moved = temp.path().join("moved");
        fs::rename(&modules, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &modules).unwrap();

        let staged = temp.path().join("staged");
        assert!(!copy_selected(&source, &staged, [(relative, expected.as_slice())]).unwrap());
        assert!(!staged.join(relative).exists());
    }
}
