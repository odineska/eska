//! Conditional configuration publication and stable leases for one add transaction.

use super::{Error, io_error};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Edit {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    after: Vec<u8>,
}

impl Edit {
    pub fn read(path: &Path) -> Result<Self, Error> {
        let before = read(path)?;
        let after = before.clone().unwrap_or_default();
        Ok(Self {
            path: path.to_owned(),
            before,
            after,
        })
    }
    pub fn text(&self) -> Result<&str, Error> {
        std::str::from_utf8(&self.after).map_err(|_| Error::NonUtf8(self.path.clone()))
    }
    pub fn document(&self) -> Result<toml_edit::DocumentMut, Error> {
        self.text()?.parse().map_err(|source| Error::Toml {
            path: self.path.clone(),
            source,
        })
    }
    pub fn set(&mut self, after: Vec<u8>) {
        self.after = after;
    }
    pub fn changed(&self) -> bool {
        self.before.as_deref() != Some(&self.after)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if read(&self.path)? == self.before {
            Ok(())
        } else {
            Err(Error::Changed(self.path.clone()))
        }
    }
    pub fn writable(&self) -> Result<(), Error> {
        if self.changed()
            && fs::metadata(&self.path).is_ok_and(|metadata| metadata.permissions().readonly())
        {
            return Err(io_error(
                &self.path,
                io::Error::from(io::ErrorKind::PermissionDenied),
            ));
        }
        Ok(())
    }
    pub fn publish(&self) -> Result<(), Error> {
        self.validate()?;
        if !self.changed() {
            return Ok(());
        }
        self.writable()?;
        publish(&self.path, self.before.as_deref(), &self.after)
    }
    pub fn restore(&self) -> Result<(), Error> {
        if !self.changed() || read(&self.path)? == self.before {
            return Ok(());
        }
        if read(&self.path)?.as_deref() != Some(&self.after) {
            return Err(Error::Changed(self.path.clone()));
        }
        self.before.as_ref().map_or_else(
            || fs::remove_file(&self.path).map_err(|source| io_error(&self.path, source)),
            |before| publish(&self.path, Some(&self.after), before),
        )
    }
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => fs::read(path)
            .map(Some)
            .map_err(|source| io_error(path, source)),
        Ok(_) => Err(Error::Changed(path.to_owned())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(io_error(path, source)),
    }
}

pub(super) fn lock(path: &Path) -> Result<File, Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Changed(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    if fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_file()) {
        return Err(Error::Changed(path.to_owned()));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|source| io_error(path, source))?;
    file.try_lock().map_err(|source| match source {
        fs::TryLockError::WouldBlock => Error::Locked(path.to_owned()),
        fs::TryLockError::Error(source) => io_error(path, source),
    })?;
    Ok(file)
}

fn publish(path: &Path, expected: Option<&[u8]>, bytes: &[u8]) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Changed(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    let (temporary, mut file) = temporary(path)?;
    let guard = Temporary(temporary);
    if let Ok(metadata) = fs::metadata(path) {
        file.set_permissions(metadata.permissions())
            .map_err(|source| io_error(path, source))?;
    }
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error(path, source))?;
    drop(file);
    if read(path)?.as_deref() != expected {
        return Err(Error::Changed(path.to_owned()));
    }
    if expected.is_none() {
        fs::hard_link(&guard.0, path).map_err(|source| io_error(path, source))?;
    } else {
        fs::rename(&guard.0, path).map_err(|source| io_error(path, source))?;
    }
    Ok(())
}

fn temporary(path: &Path) -> Result<(PathBuf, File), Error> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..100_u8 {
        let temporary =
            path.with_extension(format!("toml.add-{}-{nonce}-{attempt}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(io_error(&temporary, source)),
        }
    }
    Err(io_error(
        path,
        io::Error::from(io::ErrorKind::AlreadyExists),
    ))
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
