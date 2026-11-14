//! Paths for consumers that do not accept Win32 verbatim syntax.
use std::{borrow::Cow, path::Path};

/// Convert canonical Win32 drive and UNC paths without changing other path kinds.
#[must_use]
#[cfg(windows)]
pub fn native_path(path: &Path) -> Cow<'_, Path> {
    use std::{
        ffi::OsString,
        path::{Component, PathBuf, Prefix},
    };

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Cow::Borrowed(path);
    };
    let mut plain = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            let mut value = OsString::from(r"\\");
            value.push(server);
            value.push(r"\");
            value.push(share);
            PathBuf::from(value)
        }
        _ => return Cow::Borrowed(path),
    };
    for component in components {
        plain.push(component.as_os_str());
    }
    Cow::Owned(plain)
}

/// Preserve paths on systems without Win32 verbatim prefixes.
#[must_use]
#[cfg(not(windows))]
pub const fn native_path(path: &Path) -> Cow<'_, Path> {
    Cow::Borrowed(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_paths_are_borrowed_without_changes() {
        for path in ["", ".", "../src/Module.bsl", "/tmp/Проект #1/Module.bsl"] {
            let path = Path::new(path);
            assert!(matches!(native_path(path), Cow::Borrowed(value) if value == path));
        }
    }

    #[cfg(windows)]
    #[test]
    fn drive_and_unc_paths_preserve_unicode_spaces_and_roots() {
        for (input, expected) in [
            (r"\\?\C:\Проект 😀 #1\src", r"C:\Проект 😀 #1\src"),
            (r"\\?\UNC\server\share\Проект", r"\\server\share\Проект"),
            (r"\\?\C:\", r"C:\"),
            (r"\\?\UNC\server\share\", r"\\server\share\"),
        ] {
            assert_eq!(native_path(Path::new(input)), Path::new(expected));
        }
    }

    #[cfg(windows)]
    #[test]
    fn non_verbatim_and_device_paths_are_borrowed() {
        for input in [
            r"C:\Проект\src",
            r"\\server\share\Проект",
            r"\\.\pipe\eska",
            r"\\?\Volume{1234}\src",
        ] {
            let path = Path::new(input);
            assert!(matches!(native_path(path), Cow::Borrowed(value) if value == path));
        }
    }
}
