//! Ordinary form sidecars are diagnostic inputs; binary dialogs only decide freshness.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::{Failure, Snapshot, fingerprint};
use crate::process::Analyzer;
use sha2::{Digest, Sha256};

/// A sidecar has no independent JSONL event: it belongs to the adjacent form module.
pub(super) fn capture(snapshot: &mut Snapshot, analyzer: &Analyzer<'_>) -> Result<(), Failure> {
    for module in &snapshot.modules {
        let Some(form) = module.parent().filter(|path| named(path, "Form")) else {
            continue;
        };
        let Some(ext) = form.parent().filter(|path| named(path, "Ext")) else {
            continue;
        };
        if !named(module, "Module.bsl") {
            continue;
        }
        // Metadata loaders can read a dialog even when source exclusions omit it
        // from the module walk. Probe the form directory independently of that walk.
        let dialog = find_ci(ext, "Form.xml")?;
        if let Some(path) = &dialog
            && !snapshot.files.contains_key(path)
        {
            snapshot
                .files
                .insert(path.clone(), fingerprint(path, analyzer)?);
        }
        let Some(sidecar) = find_ci(form, "Form.json")? else {
            continue;
        };
        if dialog.is_some_and(|path| fs::read_to_string(path).is_ok()) {
            continue;
        }
        let binary = find_ci(ext, "Form.bin")?;
        let stale = is_stale(&sidecar, binary.as_deref());
        let mut digest = Sha256::new();
        digest.update(fingerprint(&sidecar, analyzer)?);
        digest.update([u8::from(stale)]);
        snapshot.files.insert(sidecar, digest.finalize().into());
    }
    Ok(())
}

fn named(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
}

/// Prefer the canonical spelling just as upstream does, then accept a case variant.
fn find_ci(directory: &Path, name: &str) -> Result<Option<PathBuf>, Failure> {
    let exact = directory.join(name);
    match fs::symlink_metadata(&exact) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(Failure::UnsupportedPath { path: exact });
            }
            return Ok(Some(exact));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Failure::Io {
                path: exact,
                source,
            });
        }
    }
    for entry in fs::read_dir(directory).map_err(|source| Failure::Io {
        path: directory.into(),
        source,
    })? {
        let entry = entry.map_err(|source| Failure::Io {
            path: directory.into(),
            source,
        })?;
        if named(&entry.path(), name) {
            if entry
                .file_type()
                .map_err(|source| Failure::Io {
                    path: entry.path(),
                    source,
                })?
                .is_symlink()
            {
                return Err(Failure::UnsupportedPath { path: entry.path() });
            }
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// Upstream uses only this ordering, not raw times or binary contents. Missing dialogs,
/// unavailable times and equal times are fresh; touching either file may change the result.
fn is_stale(sidecar: &Path, binary: Option<&Path>) -> bool {
    let modified = |path: &Path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    };
    binary
        .and_then(modified)
        .zip(modified(sidecar))
        .is_some_and(|(binary, sidecar)| binary > sidecar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::atomic::AtomicBool,
        time::{Duration, SystemTime},
    };

    fn snapshot(module: PathBuf) -> Snapshot {
        Snapshot {
            modules: BTreeSet::from([module]),
            files: BTreeMap::new(),
        }
    }

    fn capture_test(snapshot: &mut Snapshot) {
        let cancelled = AtomicBool::new(false);
        capture(
            snapshot,
            &Analyzer {
                binary: PathBuf::new(),
                timeout: Duration::from_secs(1),
                cancelled: &cancelled,
                connection_closed: None,
            },
        )
        .unwrap();
    }

    fn set_time(path: &Path, seconds: u64) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn tracks_bytes_and_effective_freshness_without_binary_content_or_absolute_times() {
        let owned = TestDir::new();
        let ext = owned.0.join("eXt");
        let form = ext.join("fOrM");
        fs::create_dir_all(&form).unwrap();
        let module = form.join("mOdUlE.BsL");
        fs::write(&module, "").unwrap();
        let json = form.join("fOrM.JsOn");
        fs::write(&json, "{}").unwrap();
        let bin = ext.join("fOrM.BiN");
        fs::write(&bin, [0]).unwrap();
        set_time(&json, 20);
        set_time(&bin, 10);
        let mut initial = snapshot(module.clone());
        capture_test(&mut initial);
        fs::write(&bin, [1, 2]).unwrap();
        set_time(&json, 40);
        set_time(&bin, 30);
        let mut shifted = snapshot(module.clone());
        capture_test(&mut shifted);
        assert!(initial == shifted);
        set_time(&bin, 50);
        let mut stale = snapshot(module.clone());
        capture_test(&mut stale);
        assert!(initial != stale);
        set_time(&bin, 40);
        let mut equal = snapshot(module.clone());
        capture_test(&mut equal);
        assert!(equal == initial);
        fs::remove_file(&bin).unwrap();
        let mut absent = snapshot(module.clone());
        capture_test(&mut absent);
        assert!(absent == initial);
        fs::write(&json, "{\"schema\":2}").unwrap();
        let mut changed = snapshot(module.clone());
        capture_test(&mut changed);
        assert!(changed != initial);
        fs::remove_file(&json).unwrap();
        let mut removed = snapshot(module);
        capture_test(&mut removed);
        assert!(removed != initial);
    }

    #[test]
    fn ignores_unrelated_json_binary_and_sidecars_without_a_form_module() {
        let owned = TestDir::new();
        let ext = owned.0.join("Ext");
        let form = ext.join("Form");
        fs::create_dir_all(&form).unwrap();
        fs::write(ext.join("Form.bin"), [0]).unwrap();
        fs::write(form.join("Other.json"), "{}").unwrap();
        let mut empty = snapshot(form.join("Module.bsl"));
        capture_test(&mut empty);
        assert_eq!(empty.files, BTreeMap::new());
        fs::write(form.join("Form.json"), "{}").unwrap();
        let mut unrelated = snapshot(ext.join("ObjectModule.bsl"));
        capture_test(&mut unrelated);
        assert_eq!(unrelated.files, BTreeMap::new());
        let xml = ext.join("fOrM.xMl");
        fs::write(&xml, "<Form/>").unwrap();
        let mut managed = snapshot(form.join("Module.bsl"));
        capture_test(&mut managed);
        assert_eq!(managed.files.len(), 1);
        assert!(managed.files.contains_key(&xml));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_sidecar_aliases_instead_of_following_links() {
        let owned = TestDir::new();
        let form = owned.0.join("Ext/Form");
        fs::create_dir_all(&form).unwrap();
        let target = owned.0.join("Other.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(target, form.join("Form.json")).unwrap();
        let mut input = snapshot(form.join("Module.bsl"));
        let cancelled = AtomicBool::new(false);
        let analyzer = Analyzer {
            binary: PathBuf::new(),
            timeout: Duration::from_secs(1),
            cancelled: &cancelled,
            connection_closed: None,
        };
        assert!(matches!(
            capture(&mut input, &analyzer),
            Err(Failure::UnsupportedPath { .. })
        ));
    }
}
