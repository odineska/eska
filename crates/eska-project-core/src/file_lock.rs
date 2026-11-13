//! Ownership of an acquired OS file lock, independent of other copies of its descriptor.

use std::fs::{File, TryLockError};

#[derive(Debug)]
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// A failed acquisition must never release another owner's lock.
    pub(crate) fn try_lock(file: File) -> Result<Self, TryLockError> {
        file.try_lock()?;
        Ok(Self { file })
    }

    #[cfg(all(test, unix))]
    pub(crate) fn duplicate(&self) -> std::io::Result<File> {
        self.file.try_clone()
    }
}

impl Drop for FileLock {
    /// A concurrent fork can retain this open-file description until exec; close alone is insufficient.
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::fs::OpenOptions;

    #[test]
    fn release_does_not_wait_for_an_inherited_descriptor() {
        let directory = TestDir::new();
        let path = directory.0.join("settings.lock");
        let open = || {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .unwrap()
        };
        let owner = FileLock::try_lock(open()).unwrap();
        let inherited = owner.duplicate().unwrap();
        assert!(matches!(
            FileLock::try_lock(open()),
            Err(TryLockError::WouldBlock)
        ));
        drop(owner);
        let next = FileLock::try_lock(open()).unwrap();
        drop(inherited);
        assert!(matches!(
            FileLock::try_lock(open()),
            Err(TryLockError::WouldBlock)
        ));
        drop(next);
        FileLock::try_lock(open()).unwrap();
    }
}
