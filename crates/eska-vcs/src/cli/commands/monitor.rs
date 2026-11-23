//! Versioned newline-delimited repository snapshots for long-lived consumers.

use crate::vcs::{
    monitor::Snapshot,
    repository::{Error, Head, Repository},
    status::Change,
};
pub use eska_presentation::arguments::vcs::monitor::{MonitorArgs, localize};
use eska_presentation::{
    arguments::vcs::status::OutputFormat,
    encoding::{json_git_text, json_path},
    localization::{LocalizationValue, Localizer},
};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

pub fn run(args: &MonitorArgs, root: &Path, localizer: &Localizer) -> ExitCode {
    let mut output = io::stdout().lock();
    let mut output_connected = stdout_connected();
    let result = Repository::discover(root).and_then(|repository| repository.monitor());
    let mut monitor = match result {
        Ok(monitor) => monitor,
        Err(error) => return fail(&mut output, args.format, &error, localizer),
    };
    loop {
        if let Err(error) = write_snapshot(&mut output, args.format, monitor.snapshot(), localizer)
        {
            if error.kind() == io::ErrorKind::BrokenPipe {
                return ExitCode::SUCCESS;
            }
            eprintln!("{}", localizer.text("status-json-error"));
            return ExitCode::FAILURE;
        }
        match monitor.next_snapshot_while(&mut output_connected) {
            Ok(Some(_)) => {}
            Ok(None) => return ExitCode::SUCCESS,
            Err(error) => return fail(&mut output, args.format, &error, localizer),
        }
    }
}

#[cfg(unix)]
fn stdout_connected() -> impl FnMut() -> bool {
    use rustix::{
        event::{PollFd, PollFlags, Timespec, poll},
        fs::{FileType, fstat},
    };

    let output = io::stdout();
    let watch = fstat(&output).is_ok_and(|stat| {
        matches!(
            FileType::from_raw_mode(stat.st_mode),
            FileType::Fifo | FileType::Socket
        )
    });
    move || {
        if !watch {
            return true;
        }
        // Node child_process uses sockets on Unix; shell pipelines generally use FIFOs.
        // Only terminal conditions matter: a full transport is still connected.
        let mut descriptors = [PollFd::new(&output, PollFlags::empty())];
        poll(&mut descriptors, Some(&Timespec::default())).is_err()
            || !descriptors[0]
                .revents()
                .intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL)
    }
}

// std has no safe non-writing pipe-disconnection query on Windows. Keep detecting
// BrokenPipe on actual output there, without treating stdin EOF as cancellation.
#[cfg(not(unix))]
fn stdout_connected() -> impl FnMut() -> bool {
    || true
}

fn fail(
    output: &mut impl Write,
    format: OutputFormat,
    error: &Error,
    localizer: &Localizer,
) -> ExitCode {
    if matches!(format, OutputFormat::Json) {
        let code = match error {
            Error::NotFound { .. } => "repository_not_found",
            Error::InvalidIndex { .. } => "invalid_index",
            _ => "monitor_failed",
        };
        let _ = writeln!(
            output,
            "{}",
            json!({"schema_version":1,"kind":"error","code":code})
        );
        let _ = output.flush();
    }
    let reason = match error {
        Error::Operation { source, .. } => source.to_string(),
        Error::Io { source, .. } => source.to_string(),
        _ => localizer.text("status-repository-error"),
    };
    eprintln!(
        "{}",
        localizer.format(
            "monitor-error",
            &[("reason", LocalizationValue::Text(&reason))]
        )
    );
    ExitCode::FAILURE
}

fn write_snapshot(
    output: &mut impl Write,
    format: OutputFormat,
    snapshot: &Snapshot,
    localizer: &Localizer,
) -> io::Result<()> {
    match format {
        OutputFormat::Json => writeln!(output, "{}", document(snapshot))?,
        OutputFormat::Human => {
            let root = snapshot.root.display().to_string();
            let files = snapshot.status.entries.len().to_string();
            writeln!(
                output,
                "{}",
                localizer.format(
                    "monitor-status",
                    &[
                        ("root", LocalizationValue::Text(&root)),
                        ("files", LocalizationValue::Text(&files))
                    ]
                )
            )?;
        }
    }
    output.flush()
}

fn document(snapshot: &Snapshot) -> Value {
    let (root, root_encoding) = json_path(snapshot.root.as_os_str());
    let (head, reference, commit) = match &snapshot.head {
        Head::Attached { reference, id } => ("attached", Some(reference), Some(id.to_string())),
        Head::Detached { id } => ("detached", None, Some(id.to_string())),
        Head::Unborn { reference } => ("unborn", Some(reference), None),
    };
    let branch = reference.map(|reference| {
        json_git_text(reference.strip_prefix(b"refs/heads/").unwrap_or(reference))
    });
    let changes: Vec<_> = snapshot.status.entries.iter().map(|entry| {
        let (path, path_encoding) = json_git_text(&entry.path);
        json!({"path":path,"path_encoding":path_encoding,"index":entry.index.map(kind),"worktree":entry.worktree.map(kind)})
    }).collect();
    json!({"schema_version":1,"kind":"snapshot","repository":{"root":root,"root_encoding":root_encoding,"head":head,"branch":branch.as_ref().map(|(value,_)|value),"branch_encoding":branch.as_ref().map(|(_,encoding)|encoding),"commit":commit},"changes":changes})
}

const fn kind(change: Change) -> &'static str {
    match change {
        Change::Added => "added",
        Change::Modified => "modified",
        Change::Deleted => "deleted",
        Change::TypeChanged => "type_changed",
        Change::Untracked => "untracked",
        Change::IntentToAdd => "intent_to_add",
        Change::Conflict => "conflict",
    }
}
