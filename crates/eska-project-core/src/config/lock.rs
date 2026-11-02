use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub fn catalog_path(config: &Path) -> PathBuf {
    config.with_extension("toml.eska-add.lock")
}

/// Keep the lock on a stable sibling while the data file is replaced.
pub fn acquire(path: &Path) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err(io::ErrorKind::InvalidInput.into()),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock()?;
    Ok(file)
}
