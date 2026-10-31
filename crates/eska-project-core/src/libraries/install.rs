//! Publish validated managed sources only after an exclusively owned checkout completes.

use super::{Error, KnownLibrary, Revision, check_cancel, io_error, write};
use crate::{
    config::ProjectConfig,
    project::{ProjectType, build::BuildSettings},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const RECEIPT: &str = ".eska/library.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u8,
    name: String,
    repository: String,
    reference: String,
    revision: String,
}

pub(super) struct Prepared {
    destination: PathBuf,
    source: PathBuf,
    stage: Option<Stage>,
    revision: Revision,
    published: bool,
    keep: bool,
    _lock: File,
}

impl Prepared {
    pub fn source(&self) -> &Path {
        &self.source
    }
    pub const fn revision(&self) -> &Revision {
        &self.revision
    }
    pub const fn is_new(&self) -> bool {
        self.stage.is_some()
    }
    pub const fn keep(&mut self) {
        self.keep = true;
    }
    pub fn publish(&mut self) -> Result<(), Error> {
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        fs::create_dir(&self.destination).map_err(|source| io_error(&self.destination, source))?;
        self.published = true;
        // Exclusive directory creation prevents rename from replacing a concurrently created empty directory.
        for entry in
            fs::read_dir(&stage.checkout).map_err(|source| io_error(&stage.checkout, source))?
        {
            let entry = entry.map_err(|source| io_error(&stage.checkout, source))?;
            if entry.file_name() == ".eska" {
                continue;
            }
            fs::rename(entry.path(), self.destination.join(entry.file_name()))
                .map_err(|source| io_error(&self.destination, source))?;
        }
        // The receipt becomes visible only after the complete checkout, including .git, is in place.
        fs::rename(stage.checkout.join(".eska"), self.destination.join(".eska"))
            .map_err(|source| io_error(&self.destination, source))?;
        Ok(())
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if self.stage.is_some() && self.published && !self.keep {
            let _ = fs::remove_dir_all(&self.destination);
        }
    }
}

pub(super) fn prepare<F>(
    destination: &Path,
    library: KnownLibrary,
    platform: Option<&str>,
    fetch: F,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Prepared, Error>
where
    F: FnOnce(KnownLibrary, &Path) -> Result<Revision, Error>,
{
    let lock = write::lock(&destination.with_extension("eska-add.lock"))?;
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            let revision = receipt(destination, library)?;
            validate_manifest(destination, library, platform, false)?;
            return Ok(Prepared {
                destination: destination.to_owned(),
                source: destination.join("src/cf"),
                stage: None,
                revision,
                published: false,
                keep: true,
                _lock: lock,
            });
        }
        Ok(_) => return Err(Error::InvalidReceipt(destination.to_owned())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(io_error(destination, source)),
    }
    let stage = Stage::new(destination)?;
    let revision = fetch(library, &stage.checkout)?;
    check_cancel(cancelled)?;
    validate_manifest(&stage.checkout, library, platform, true)?;
    let metadata = stage.checkout.join(".eska");
    match fs::symlink_metadata(&metadata) {
        Ok(existing) if existing.file_type().is_dir() => {}
        Ok(_) => return Err(Error::InvalidReceipt(metadata)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&metadata).map_err(|source| io_error(&metadata, source))?;
        }
        Err(source) => return Err(io_error(&metadata, source)),
    }
    let receipt = Receipt {
        schema_version: 1,
        name: library.name().into(),
        repository: library.repository().into(),
        reference: "refs/heads/master".into(),
        revision: revision.as_str().into(),
    };
    let path = stage.checkout.join(RECEIPT);
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|source| io_error(&path, io::Error::other(source)))?;
    create_file(&path, &bytes)?;
    Ok(Prepared {
        destination: destination.to_owned(),
        source: stage.checkout.join("src/cf"),
        stage: Some(stage),
        revision,
        published: false,
        keep: false,
        _lock: lock,
    })
}

fn validate_manifest(
    root: &Path,
    library: KnownLibrary,
    platform: Option<&str>,
    create: bool,
) -> Result<(), Error> {
    let path = root.join("eska.toml");
    if !fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_dir()) {
        return Err(Error::InvalidReceipt(root.to_owned()));
    }
    let canonical = fs::canonicalize(root).map_err(|source| io_error(root, source))?;
    let expected = fs::canonicalize(root.join("src/cf"))
        .map_err(|source| io_error(&root.join("src/cf"), source))?;
    if !expected.starts_with(&canonical) || expected == canonical {
        return Err(Error::InvalidReceipt(path));
    }
    if fs::symlink_metadata(&path).is_ok() {
        if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) {
            return Err(Error::InvalidReceipt(path));
        }
        let config =
            ProjectConfig::load(&path).map_err(|source| Error::Project(Box::new(source)))?;
        let project = config
            .into_project(root.to_owned())
            .map_err(|source| Error::Project(Box::new(source)))?;
        let source = fs::canonicalize(project.source())
            .map_err(|source| io_error(project.source(), source))?;
        let expected = fs::canonicalize(root.join("src/cf"))
            .map_err(|source| io_error(&root.join("src/cf"), source))?;
        if project.configuration().project_type() != ProjectType::Configuration
            || source != expected
        {
            return Err(Error::Conflict {
                path,
                field: "project.type/source".into(),
            });
        }
    } else if create {
        let mut config = ProjectConfig::new(ProjectType::Configuration)
            .with_name(
                crate::project::ProjectName::parse(library.name().into())
                    .map_err(|_| Error::InvalidReceipt(path.clone()))?,
            )
            .with_source("src/cf".into())
            .map_err(|source| Error::Project(Box::new(source)))?;
        if let Some(platform) = platform {
            let build = BuildSettings::new(platform, "build".into())
                .map_err(|_| Error::InvalidReceipt(path.clone()))?;
            config = config.with_build_settings(build);
        }
        let contents = config
            .to_toml()
            .map_err(|source| io_error(&path, io::Error::other(source)))?;
        create_file(&path, contents.as_bytes())?;
    } else {
        return Err(Error::InvalidReceipt(path));
    }
    Ok(())
}

fn receipt(root: &Path, library: KnownLibrary) -> Result<Revision, Error> {
    let path = root.join(RECEIPT);
    if !fs::symlink_metadata(&path)
        .is_ok_and(|metadata| metadata.file_type().is_file() && metadata.len() < 64 * 1024)
    {
        return Err(Error::InvalidReceipt(path));
    }
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(&path).map_err(|source| io_error(&path, source))?)
            .map_err(|_| Error::InvalidReceipt(path.clone()))?;
    if receipt.schema_version != 1
        || receipt.name != library.name()
        || receipt.repository != library.repository()
        || receipt.reference != "refs/heads/master"
    {
        return Err(Error::InvalidReceipt(path));
    }
    Revision::parse(receipt.revision).map_err(|_| Error::InvalidReceipt(path))
}

fn create_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| io_error(path, source))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error(path, source))
}

struct Stage {
    root: PathBuf,
    checkout: PathBuf,
}
impl Stage {
    fn new(destination: &Path) -> Result<Self, Error> {
        let parent = destination
            .parent()
            .ok_or_else(|| Error::InvalidReceipt(destination.to_owned()))?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0..100_u8 {
            let root = parent.join(format!(
                ".eska-library-{}-{nonce}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => {
                    return Ok(Self {
                        checkout: root.join("checkout"),
                        root,
                    });
                }
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(io_error(&root, source)),
            }
        }
        Err(io_error(
            parent,
            io::Error::from(io::ErrorKind::AlreadyExists),
        ))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
