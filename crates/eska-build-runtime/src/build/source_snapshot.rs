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
    tree_digest(source, Some(target), excluded)
}

/// Identify all current bytes without creating a copy or relying on metadata timestamps.
pub(super) fn digest(source: &Path, excluded: &[&Path]) -> Result<Vec<u8>, Error> {
    tree_digest(source, None, excluded)
}

fn tree_digest(source: &Path, target: Option<&Path>, excluded: &[&Path]) -> Result<Vec<u8>, Error> {
    let mut hash = Sha256::new();
    let mut walker = Walker::new(source, excluded, Some(&mut hash));
    walker.visit(source, target, &mut |_, _, _| Ok(()))?;
    Ok(hash.finalize().to_vec())
}

/// Capture individual files for runtime delta calculation, optionally from an immutable copy.
pub fn catalog(source: &Path, target: Option<&Path>, excluded: &[&Path]) -> Result<Files, Error> {
    Walker::new(source, excluded, None).catalog(target)
}

/// Capture exact digests and freeze the inputs needed to plan an incremental import.
/// `None` means that a selected file changed while it was being staged.
pub fn stage_delta(
    source: &Path,
    target: &Path,
    excluded: &[&Path],
    previous: &Files,
    required: Option<&Path>,
) -> Result<Option<Files>, Error> {
    create_directory(target)?;
    let mut walker = Walker::new(source, excluded, None);
    let mut files = Files::new();
    let mut needs_xml = false;
    let mut stable = true;
    walker.visit(source, None, &mut |relative, digest, buffered| {
        let old = previous.get(relative);
        let changed = old.is_none_or(|old| *old != digest);
        let module = has_extension(relative, "bsl");
        needs_xml |= old.is_none() || (changed && !module);
        if stable && ((changed && module) || Some(relative) == required) {
            stable = stage_entry(source, target, relative, &digest, buffered)?;
        }
        files.insert(relative.to_owned(), digest);
        Ok(())
    })?;
    if !stable {
        return Ok(None);
    }
    // Structural plans can check references in unrelated XML. Freeze all XML
    // for those checks and leave other resources to the eventual import list.
    if needs_xml || previous.len() != files.len() {
        for (path, digest) in &files {
            if has_extension(path, "xml") && !stage_missing(source, target, path, digest)? {
                return Ok(None);
            }
        }
    }
    Ok(Some(files))
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

/// Materialize import companions from the same catalog before the first write.
pub fn stage_selected(
    source: &Path,
    target: &Path,
    files: &Files,
    paths: &[PathBuf],
) -> Result<bool, Error> {
    for path in paths {
        let Some(digest) = files.get(path) else {
            return Ok(false);
        };
        if !stage_missing(source, target, path, digest)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn stage_missing(source: &Path, target: &Path, path: &Path, digest: &[u8]) -> Result<bool, Error> {
    Ok(is_entry(&target.join(path), fs::Metadata::is_file)?
        || stage_entry(source, target, path, digest, None)?)
}

fn stage_entry(
    source: &Path,
    target: &Path,
    relative: &Path,
    expected: &[u8],
    buffered: Option<&[u8]>,
) -> Result<bool, Error> {
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
    if let Some(bytes) = buffered {
        fs::write(&output, bytes).map_err(|source| Error::Io {
            path: output.clone(),
            source,
        })?;
        copy_permissions(&input, &output)?;
        return Ok(true);
    }
    match file_digest(&input, Some(&output)) {
        Ok(actual) => Ok(actual == expected),
        Err(Error::Io { path, source })
            if path == input && source.kind() == io::ErrorKind::NotFound =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
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
    let mut hash = Sha256::new();
    let files = Walker::new(source, excluded, Some(&mut hash)).catalog(target)?;
    Ok((files, hash.finalize().to_vec()))
}

const BUFFER_SIZE: usize = 64 * 1024;

struct Walker<'a> {
    root: &'a Path,
    excluded: &'a [&'a Path],
    tree: Option<&'a mut Sha256>,
    buffer: Box<[u8]>,
}

impl<'a> Walker<'a> {
    fn new(root: &'a Path, excluded: &'a [&'a Path], tree: Option<&'a mut Sha256>) -> Self {
        Self {
            root,
            excluded,
            tree,
            buffer: vec![0; BUFFER_SIZE].into_boxed_slice(),
        }
    }

    fn catalog(&mut self, target: Option<&Path>) -> Result<Files, Error> {
        let mut files = Files::new();
        self.visit(self.root, target, &mut |path, digest, _| {
            files.insert(path.to_owned(), digest);
            Ok(())
        })?;
        Ok(files)
    }

    /// Preserve tree ordering and entry encoding for callers requesting a tree digest.
    fn visit(
        &mut self,
        source: &Path,
        target: Option<&Path>,
        on_file: &mut impl FnMut(&Path, Vec<u8>, Option<&[u8]>) -> Result<(), Error>,
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
                || self
                    .excluded
                    .iter()
                    .any(|excluded| path == **excluded || path.starts_with(excluded))
            {
                continue;
            }
            let relative = path
                .strip_prefix(self.root)
                .expect("directory entries remain below the source root");
            let destination = target.map(|target| target.join(entry.file_name()));
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_dir() {
                if let Some(hash) = self.tree.as_deref_mut() {
                    hash_entry(hash, b'd', relative, &[]);
                }
                self.visit(&path, destination.as_deref(), on_file)?;
            } else if file_type.is_file() {
                let (digest, length) = read_file(&path, destination.as_deref(), &mut self.buffer)?;
                if let Some(hash) = self.tree.as_deref_mut() {
                    hash_entry(hash, b'f', relative, &digest);
                }
                on_file(
                    relative,
                    digest,
                    length.map(|length| &self.buffer[..length]),
                )?;
            } else {
                return Err(Error::UnsupportedEntry(path));
            }
        }
        if let Some(target) = target {
            copy_permissions(source, target)?;
        }
        Ok(())
    }
}

/// Stream one file once so copied bytes and their checksum cannot diverge.
pub(super) fn file_digest(source: &Path, target: Option<&Path>) -> Result<Vec<u8>, Error> {
    let mut buffer = vec![0; BUFFER_SIZE].into_boxed_slice();
    read_file(source, target, &mut buffer).map(|(digest, _)| digest)
}

/// Return the complete bytes in the shared buffer when they fit in a single read.
fn read_file(
    source: &Path,
    target: Option<&Path>,
    buffer: &mut [u8],
) -> Result<(Vec<u8>, Option<usize>), Error> {
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
    let mut buffered = Some(0);
    loop {
        let read = input.read(buffer).map_err(|error| Error::Io {
            path: source.to_owned(),
            source: error,
        })?;
        if read == 0 {
            break;
        }
        buffered = buffered.filter(|length| *length == 0).map(|_| read);
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
    Ok((hash.finalize().to_vec(), buffered))
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
    fn module_snapshot_stages_exact_bytes_and_keeps_the_full_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let required = Path::new("Configuration.xml");
        let small = Path::new("Small.bsl");
        let large = Path::new("Large.bsl");
        let unchanged = Path::new("Unchanged.bsl");
        for path in [required, small, large, unchanged] {
            fs::write(source.join(path), b"original").unwrap();
        }
        let previous = catalog(&source, None, &[]).unwrap();
        let large_bytes = vec![b'a'; BUFFER_SIZE + 1];
        fs::write(source.join(small), b"").unwrap();
        fs::write(source.join(large), &large_bytes).unwrap();
        let staged = temp.path().join("staged");
        let current = stage_delta(&source, &staged, &[], &previous, Some(required))
            .unwrap()
            .unwrap();

        assert_eq!(current, catalog(&source, None, &[]).unwrap());
        assert_eq!(fs::read(staged.join(small)).unwrap(), b"");
        assert_eq!(fs::read(staged.join(large)).unwrap(), large_bytes);
        assert_eq!(fs::read(staged.join(required)).unwrap(), b"original");
        assert!(!staged.join(unchanged).exists());
    }

    #[test]
    fn xml_delta_freezes_references_and_stages_only_selected_resources() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let descriptor = PathBuf::from("Changed.xml");
        let references = PathBuf::from("References.xml");
        let resource = PathBuf::from("Picture.png");
        let unrelated = PathBuf::from("Unrelated.bsl");
        for path in [&descriptor, &references, &resource, &unrelated] {
            fs::write(source.join(path), b"old bytes").unwrap();
        }
        let previous = catalog(&source, None, &[]).unwrap();
        fs::write(source.join(&descriptor), b"new bytes").unwrap();
        let staged = temp.path().join("staged");
        let current = stage_delta(&source, &staged, &[], &previous, None)
            .unwrap()
            .unwrap();
        assert_eq!(current, catalog(&source, None, &[]).unwrap());
        assert_eq!(fs::read(staged.join(&descriptor)).unwrap(), b"new bytes");
        assert_eq!(fs::read(staged.join(&references)).unwrap(), b"old bytes");
        assert!(!staged.join(&resource).exists());

        fs::write(source.join(&descriptor), b"later edit").unwrap();
        let selected = [descriptor.clone(), resource.clone()];
        assert!(stage_selected(&source, &staged, &current, &selected).unwrap());
        assert_eq!(fs::read(staged.join(&descriptor)).unwrap(), b"new bytes");
        assert_eq!(fs::read(staged.join(resource)).unwrap(), b"old bytes");
        assert!(!staged.join(unrelated).exists());
    }

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

        assert!(!stage_entry(&source, &temp.path().join("stale"), name, &expected, None).unwrap());

        fs::write(&module, "A = 1;").unwrap();
        let staged = temp.path().join("staged");
        assert!(stage_entry(&source, &staged, name, &expected, None).unwrap());
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
        assert!(!stage_entry(&source, &staged, relative, &expected, None).unwrap());
        assert!(!staged.join(relative).exists());
    }
}
