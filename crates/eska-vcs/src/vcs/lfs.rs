//! Recognize materialized Git LFS content without invoking external filters.
use std::{
    fs,
    io::{ErrorKind, Read},
};

use gix::bstr::ByteSlice;
use gix::error::ResultExt;
use sha2::{Digest, Sha256};

pub(super) fn standard_driver(config: &gix::config::File) -> bool {
    // Extensions can transform the payload even when the process command is standard.
    if config.sections_by_name("lfs").is_some_and(|mut sections| {
        sections.any(|section| {
            section
                .header()
                .subsection_name()
                .is_some_and(|name| name.starts_with(b"extension."))
        })
    }) {
        return false;
    }
    match config.string("filter.lfs.process") {
        Some(value) if !value.trim().is_empty() => value.trim() == b"git-lfs filter-process",
        _ => config
            .string("filter.lfs.clean")
            .is_some_and(|value| value.trim() == b"git-lfs clean -- %f"),
    }
}

pub(super) fn is_materialized(
    repository: &gix::Repository,
    attributes: &mut gix::AttributeStack<'_>,
    change: &gix::status::index_worktree::Item,
) -> gix::Result<bool> {
    use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
    let gix::status::index_worktree::Item::Modification {
        entry,
        rela_path,
        status:
            EntryStatus::Change(Change::Modification {
                executable_bit_changed: false,
                ..
            }),
        ..
    } = change
    else {
        return Ok(false);
    };
    if !matches!(
        entry.mode,
        gix::index::entry::Mode::FILE | gix::index::entry::Mode::FILE_EXECUTABLE
    ) {
        return Ok(false);
    }
    let mut outcome = attributes.selected_attribute_matches(["filter"]);
    attributes
        .at_entry(rela_path.as_bstr(), Some(entry.mode))?
        .matching_attributes(&mut outcome);
    if !outcome
        .iter_selected()
        .any(|value| value.assignment.state.as_bstr() == Some("lfs".into()))
    {
        return Ok(false);
    }
    let header = repository.find_header(entry.id)?;
    if header.kind() != gix::objs::Kind::Blob || header.size() >= 1024 {
        return Ok(false);
    }
    let blob = repository.find_blob(entry.id)?;
    let Some((digest, size)) = pointer(&blob.data) else {
        return Ok(false);
    };
    let path = repository
        .workdir()
        .unwrap_or_else(|| repository.git_dir())
        .join(gix::path::from_bstr(rela_path.as_bstr())?);
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        result => result.or_error()?,
    };
    if !metadata.is_file() || metadata.len() != size {
        return Ok(false);
    }
    let mut file = match fs::File::open(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        result => result.or_error()?.take(size.saturating_add(1)),
    };
    let mut hasher = Sha256::new();
    let mut buffer = [0; 16384];
    let mut actual_size = 0;
    loop {
        let count = file.read(&mut buffer).or_error()?;
        if count == 0 {
            break;
        }
        // Git LFS preserves small pointer files rather than hashing their bytes.
        // Be conservative for pointer-looking payloads, including noncanonical
        // pointers understood by Git LFS but outside our supported index form.
        if actual_size == 0
            && size < 1024
            && buffer[..count].starts_with(b"version https://git-lfs.github.com/spec/v1")
        {
            return Ok(false);
        }
        actual_size += count as u64;
        hasher.update(&buffer[..count]);
    }
    Ok(actual_size == size && hasher.finalize().as_slice() == digest)
}

fn pointer(bytes: &[u8]) -> Option<([u8; 32], u64)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split('\n');
    if lines.next()? != "version https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let oid = lines.next()?.strip_prefix("oid sha256:")?;
    if oid.len() != 64
        || !oid
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&oid[index * 2..index * 2 + 2], 16).ok()?;
    }
    let size_text = lines.next()?.strip_prefix("size ")?;
    let size = size_text.parse::<u64>().ok()?;
    if size == 0
        || size.to_string() != size_text
        || !lines.next()?.is_empty()
        || lines.next().is_some()
    {
        return None;
    }
    Some((digest, size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_support::TestDir, vcs::repository::Repository};
    use std::{fmt::Write as _, path::Path, process::Command};

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.join("absent"))
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn only_canonical_pointers_are_recognized() {
        let canonical = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
            "a".repeat(64)
        );
        assert_eq!(pointer(canonical.as_bytes()).unwrap().1, 42);
        for invalid in [
            canonical.replace("42", "042"),
            canonical.replace("42", "0"),
            canonical.replace('\n', "\r\n"),
            format!("{canonical}\n"),
            canonical.replace("sha256:", "sha1:"),
        ] {
            assert!(pointer(invalid.as_bytes()).is_none());
        }
    }

    #[test]
    fn configured_lfs_extensions_disable_the_builtin_comparison() {
        let config =
            gix::config::File::try_from("[filter \"lfs\"]\nprocess = git-lfs filter-process\n")
                .unwrap();
        assert!(standard_driver(&config));
        let config = gix::config::File::try_from("[filter \"lfs\"]\nprocess = git-lfs filter-process\n[lfs \"extension.custom\"]\nclean = custom-transform\n").unwrap();
        assert!(!standard_driver(&config));
    }

    #[cfg(unix)]
    fn check_mode_and_symlink_changes(root: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let status = || Repository::discover(root).unwrap().status().unwrap();
        git(root, &["config", "core.filemode", "true"]);
        fs::set_permissions(root.join("asset.bin"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            status().entries[0].worktree,
            Some(crate::vcs::status::Change::Modified)
        );
        fs::set_permissions(root.join("asset.bin"), fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(root.join("asset.bin")).unwrap();
        std::os::unix::fs::symlink(".gitattributes", root.join("asset.bin")).unwrap();
        assert_eq!(
            status().entries[0].worktree,
            Some(crate::vcs::status::Change::TypeChanged)
        );
    }

    #[test]
    fn materialized_content_preserves_real_changes_staging_and_attributes() {
        let dir = TestDir::new();
        git(&dir.0, &["init", "-b", "main"]);
        let content = b"materialized binary payload\0\xff";
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            Sha256::digest(content)
                .iter()
                .fold(String::new(), |mut out, byte| {
                    write!(out, "{byte:02x}").unwrap();
                    out
                }),
            content.len()
        );
        fs::write(dir.0.join(".gitattributes"), "*.bin filter=lfs -text\n").unwrap();
        fs::write(dir.0.join("asset.bin"), &pointer).unwrap();
        git(&dir.0, &["add", "."]);
        git(&dir.0, &["commit", "-m", "baseline"]);
        git(
            &dir.0,
            &["config", "filter.lfs.clean", "git-lfs clean -- %f"],
        );
        git(&dir.0, &["config", "filter.lfs.process", ""]);
        fs::write(dir.0.join("asset.bin"), content).unwrap();
        let status = || Repository::discover(&dir.0).unwrap().status().unwrap();
        assert_eq!(status().entries.len(), 0);
        let mut snapshot =
            crate::vcs::monitor::Snapshot::read(&Repository::discover(&dir.0).unwrap()).unwrap();
        let invalidation = gix::notify::Changes {
            worktree: Some(gix::notify::Scope::Paths(vec![gix::notify::PathChange {
                path: "asset.bin".into(),
                kind: gix::notify::plumbing::EventKind::Any,
                path_kind: gix::notify::plumbing::PathKind::Any,
            }])),
            ..Default::default()
        };
        assert!(!snapshot.reconcile(&invalidation).unwrap());
        // Config changes invalidate the entire snapshot even when the next
        // filesystem invalidation concerns an unrelated path.
        let unrelated = gix::notify::Changes {
            worktree: Some(gix::notify::Scope::Paths(vec![gix::notify::PathChange {
                path: "elsewhere.bsl".into(),
                kind: gix::notify::plumbing::EventKind::Any,
                path_kind: gix::notify::plumbing::PathKind::Any,
            }])),
            ..Default::default()
        };
        git(&dir.0, &["config", "filter.lfs.clean", "untrusted-command"]);
        assert!(snapshot.reconcile(&unrelated).unwrap());
        assert_eq!(snapshot.status.entries.len(), 1);
        git(
            &dir.0,
            &["config", "filter.lfs.clean", "git-lfs clean -- %f"],
        );
        assert!(snapshot.reconcile(&unrelated).unwrap());
        assert_eq!(snapshot.status.entries.len(), 0);

        fs::write(dir.0.join("asset.bin"), b"changed binary payload\0\xff").unwrap();
        assert_eq!(status().entries.len(), 1);
        assert!(snapshot.reconcile(&invalidation).unwrap());
        assert_eq!(snapshot.status.entries.len(), 1);
        fs::write(dir.0.join("asset.bin"), content).unwrap();
        assert!(
            snapshot
                .reconcile(&gix::notify::Changes {
                    worktree: Some(gix::notify::Scope::All),
                    ..Default::default()
                })
                .unwrap()
        );
        assert_eq!(snapshot.status.entries.len(), 0);
        git(&dir.0, &["config", "filter.lfs.clean", "untrusted-command"]);
        assert_eq!(status().entries.len(), 1);
        git(
            &dir.0,
            &["config", "filter.lfs.clean", "git-lfs clean -- %f"],
        );
        git(&dir.0, &["config", "filter.lfs.process", ""]);
        fs::write(dir.0.join(".gitattributes"), "*.bin -filter -text\n").unwrap();
        assert!(
            status()
                .entries
                .iter()
                .any(|entry| entry.path == "asset.bin")
        );
        fs::write(dir.0.join(".gitattributes"), "*.bin filter=lfs -text\n").unwrap();
        #[cfg(unix)]
        check_mode_and_symlink_changes(&dir.0);
        fs::remove_file(dir.0.join("asset.bin")).unwrap();
        assert_eq!(
            status().entries[0].worktree,
            Some(crate::vcs::status::Change::Deleted)
        );
        fs::write(dir.0.join("asset.bin"), &pointer).unwrap();
        git(
            &dir.0,
            &["-c", "filter.lfs.clean=", "mv", "asset.bin", "renamed.bin"],
        );
        fs::write(dir.0.join("renamed.bin"), content).unwrap();
        let staged = status();
        assert!(staged.entries.iter().all(|entry| entry.worktree.is_none()));
        assert_eq!(staged.entries.len(), 2);
    }
}
