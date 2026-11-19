//! Unix discovery owns both the adapter and its recorded container-side version probe.

use super::{Runner, ToolOptions, interruptible_runner_command, recorded_arguments};
use eska_tool_runtime::{Component, Error, Operation, QueryPolicy};
use std::{
    fs, io,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Output,
    time::{SystemTime, UNIX_EPOCH},
};

struct Directory(PathBuf);

impl Drop for Directory {
    /// Remove only the exclusively claimed control namespace after its process has been reaped.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A version probe has finite output/duration and records cancellation before native setup.
pub(super) fn output(
    runner: &Runner,
    arguments: &[std::ffi::OsString],
    outer: Option<&Operation>,
    deadline: Option<std::time::Instant>,
) -> io::Result<Output> {
    let scope = outer.map_or_else(
        || Operation::new().map_err(io_error),
        |scope| Ok(scope.clone()),
    )?;
    let directory = if matches!(runner, Runner::Distrobox { .. }) {
        Some(directory()?)
    } else {
        None
    };
    let pid = directory
        .as_ref()
        .map_or_else(PathBuf::new, |directory| directory.0.join("process.pid"));
    let mut command = match runner {
        Runner::Host(_) => interruptible_runner_command(runner, arguments, &pid),
        Runner::Distrobox { container, path } => {
            let arguments = recorded_arguments(path, arguments, &pid);
            ToolOptions::distrobox_query(container, "sh", &arguments[1..])
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?
        }
    };
    // Discovery must never answer a missing-container creation prompt.
    command
        .env("DBX_NON_INTERACTIVE", "0")
        .stdin(std::process::Stdio::null());
    let policy = QueryPolicy {
        timeout: deadline.map_or_else(
            || QueryPolicy::default().timeout,
            |deadline| deadline.saturating_duration_since(std::time::Instant::now()),
        ),
        ..QueryPolicy::default()
    };
    if policy.timeout.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    eska_tool_runtime::capture(Component::Build, &mut command, &scope, policy, || {
        terminate_probe(runner, &pid);
    })
    .map_err(io_error)
}

/// Cleanup must remain bounded even when the container adapter itself stops responding.
fn terminate_probe(runner: &Runner, pid_file: &std::path::Path) {
    let Runner::Distrobox { container, .. } = runner else {
        return;
    };
    let Ok(pid) = fs::read_to_string(pid_file) else {
        return;
    };
    let pid = pid.trim();
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return;
    }
    let Ok(mut command) = ToolOptions::distrobox_query(
        container,
        "sh",
        &[
            "-c".into(),
            "kill -TERM \"$1\"".into(),
            "sh".into(),
            pid.into(),
        ],
    ) else {
        return;
    };
    let Ok(scope) = Operation::new() else {
        return;
    };
    let _ = eska_tool_runtime::capture(
        Component::Build,
        &mut command,
        &scope,
        QueryPolicy {
            timeout: std::time::Duration::from_secs(1),
            ..QueryPolicy::default()
        },
        || {},
    );
}

/// Keep Distrobox control files in shared user storage; private container /tmp is not shared.
fn directory() -> io::Result<Directory> {
    let base = control_root()?;
    fs::create_dir_all(&base)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    for attempt in 0..32 {
        let path = base.join(format!(
            "eska-probe-{}-{stamp}-{attempt}",
            std::process::id()
        ));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                let mut directory = Directory(path);
                directory.0 = directory.0.canonicalize()?;
                return Ok(directory);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

/// An isolated machine profile keeps its transient control files in the same shared test stand.
fn control_root() -> io::Result<PathBuf> {
    if let Some(config) = std::env::var_os("ESKA_CONFIG_DIR") {
        let config = PathBuf::from(config);
        return Ok(config
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(".eska-probes"));
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache/eska/probes"))
        .ok_or_else(|| io::ErrorKind::NotFound.into())
}

/// Preserve OS causes while cancellation, timeout and size failures retain stable IO categories.
fn io_error(error: Error) -> io::Error {
    match error {
        Error::Signal(source) | Error::Process { source, .. } => source,
        Error::Cancelled => io::ErrorKind::Interrupted.into(),
        Error::Timeout => io::ErrorKind::TimedOut.into(),
        Error::OutputTooLarge | Error::InvalidResponse => io::ErrorKind::InvalidData.into(),
        error => io::Error::other(error.code()),
    }
}
