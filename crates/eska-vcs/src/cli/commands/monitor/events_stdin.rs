//! Portable invalidation transport and bounded NDJSON decoding.

use super::{MonitorArgs, fail, publish_snapshot, stdout_connected};
use crate::vcs::{
    monitor::Snapshot,
    repository::{Error, Head, Repository},
};
use eska_presentation::localization::Localizer;
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Instant, SystemTime},
};

pub(super) fn run(args: &MonitorArgs, root: &Path, localizer: &Localizer) -> ExitCode {
    use std::{sync::mpsc, time::Duration};
    let mut output = io::stdout().lock();
    // Probe before the baseline so concurrent metadata updates are reconciled again.
    let mut metadata = match Metadata::read(root) {
        Ok(metadata) => metadata,
        Err(error) => return fail(&mut output, args.format, &error, localizer),
    };
    let mut last_probe = Instant::now();
    let mut snapshot =
        match Repository::discover(root).and_then(|repository| Snapshot::read(&repository)) {
            Ok(snapshot) => snapshot,
            Err(error) => return fail(&mut output, args.format, &error, localizer),
        };
    // Backpressure bounds memory without dropping invalidations. The reader owns no watches.
    let (sender, receiver) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            match read_event(&mut input) {
                Ok(Some(changes)) => {
                    if sender.send(Ok(changes)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        }
    });
    let mut connected = stdout_connected();
    loop {
        if let Err(exit) = publish_snapshot(&mut output, args.format, &snapshot, localizer) {
            return exit;
        }
        loop {
            if !connected() {
                return ExitCode::SUCCESS;
            }
            let metadata_changed = if last_probe.elapsed() >= Duration::from_secs(1) {
                let current = match Metadata::read(&snapshot.root) {
                    Ok(current) => current,
                    Err(error) => return fail(&mut output, args.format, &error, localizer),
                };
                last_probe = Instant::now();
                let changed = current != metadata;
                metadata = current;
                changed
            } else {
                false
            };
            let changes = if metadata_changed {
                gix::notify::Changes {
                    worktree: Some(gix::notify::Scope::All),
                    ..Default::default()
                }
            } else {
                match receiver.recv_timeout(Duration::from_millis(40)) {
                    Ok(Ok(changes)) => changes,
                    Ok(Err(source)) => {
                        return fail(
                            &mut output,
                            args.format,
                            &Error::operation(crate::vcs::repository::Operation::Status, source),
                            localizer,
                        );
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return ExitCode::SUCCESS,
                }
            };
            match snapshot.reconcile(&changes) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => return fail(&mut output, args.format, &error, localizer),
            }
        }
    }
}

// A missing index is valid for unborn repositories. Recheck it on every probe;
// permission and other I/O failures must never masquerade as an unchanged index.
#[derive(PartialEq, Eq)]
struct Metadata {
    head: Head,
    index_path: PathBuf,
    index_stamp: Option<(SystemTime, u64)>,
}

impl Metadata {
    fn read(root: &Path) -> Result<Self, Error> {
        let repository = Repository::discover(root)?;
        let head = repository.head()?;
        let index_path = repository.index_path();
        let index_stamp = match std::fs::metadata(&index_path) {
            Ok(metadata) => Some((
                metadata.modified().map_err(|source| Error::Io {
                    path: index_path.clone(),
                    source,
                })?,
                metadata.len(),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(Error::Io {
                    path: index_path,
                    source,
                });
            }
        };
        Ok(Self {
            head,
            index_path,
            index_stamp,
        })
    }
}

// Oversized or malformed records conservatively request a full refresh. Drain through
// the newline so a truncated tail can never be interpreted as a separate valid event.
fn read_event(input: &mut impl io::BufRead) -> io::Result<Option<gix::notify::Changes>> {
    const LIMIT: usize = 64 * 1024;
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            return Ok((!line.is_empty() || oversized).then(|| parse_event(&line)));
        }
        let count = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1);
        let complete = bytes[count - 1] == b'\n';
        if !oversized && line.len() + count <= LIMIT {
            line.extend_from_slice(&bytes[..count]);
        } else {
            oversized = true;
            line.clear();
        }
        input.consume(count);
        if complete {
            return Ok(Some(parse_event(&line)));
        }
    }
}

fn parse_event(line: &[u8]) -> gix::notify::Changes {
    use gix::notify::{
        Changes, PathChange, Scope,
        plumbing::{EventKind, PathKind},
    };
    let full = || Changes {
        worktree: Some(Scope::All),
        ..Changes::default()
    };
    let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(line) else {
        return full();
    };
    if record.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return full();
    }
    if record.contains_key("refresh") || record.len() != 2 {
        return full();
    }
    let Some(paths) = record.get("paths").and_then(Value::as_array) else {
        return full();
    };
    if paths.is_empty() {
        return full();
    }
    let mut changes = Vec::with_capacity(paths.len());
    for path in paths {
        let Some(path) = path.as_str() else {
            return full();
        };
        if path.is_empty()
            || path.contains(['\\', '\0', ':'])
            || path.split('/').any(|part| {
                part.is_empty()
                    || part == "."
                    || part == ".."
                    || part.eq_ignore_ascii_case(".git")
                    || matches!(part, ".gitignore" | ".gitattributes" | ".gitmodules")
            })
        {
            return full();
        }
        changes.push(PathChange {
            path: path.as_bytes().into(),
            kind: EventKind::Any,
            path_kind: PathKind::Any,
        });
    }
    Changes {
        worktree: Some(Scope::Paths(changes)),
        ..Changes::default()
    }
}

#[cfg(test)]
mod event_tests {
    use super::{parse_event, read_event};
    use gix::notify::Scope;
    #[test]
    fn paths_and_rename_endpoints_are_literal() {
        let changes = parse_event(br#"{"schema_version":1,"paths":["old/file","new/file"]}"#);
        assert!(matches!(changes.worktree, Some(Scope::Paths(paths)) if paths.len() == 2));
    }
    #[test]
    fn ambiguous_and_metadata_events_refresh_everything() {
        for line in [
            r#"{"schema_version":1,"refresh":true}"#,
            r#"{"schema_version":1,"paths":["../outside"]}"#,
            r#"{"schema_version":1,"paths":["dir/.gitignore"]}"#,
            r#"{"schema_version":1,"paths":[".git/index"]}"#,
            r#"{"schema_version":2,"paths":["file"]}"#,
            "broken",
        ] {
            assert!(matches!(
                parse_event(line.as_bytes()).worktree,
                Some(Scope::All)
            ));
        }
    }
    #[test]
    fn oversized_record_is_drained_and_eof_is_clean() {
        let mut data = vec![b'x'; 100_000];
        data.extend_from_slice(b"\n{\"schema_version\":1,\"paths\":[\"file\"]}\n");
        let mut input = data.as_slice();
        assert!(matches!(
            read_event(&mut input).unwrap().unwrap().worktree,
            Some(Scope::All)
        ));
        assert!(matches!(
            read_event(&mut input).unwrap().unwrap().worktree,
            Some(Scope::Paths(_))
        ));
        assert!(read_event(&mut input).unwrap().is_none());
    }
}
