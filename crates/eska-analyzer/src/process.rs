//! Bounded subprocess I/O, cancellation and capability checks for the published CLI.

use std::{
    ffi::OsStr,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::Error;

const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

/// The selected executable and operation limits do not locate a wrapper or a platform.
pub struct Analyzer<'a> {
    pub binary: PathBuf,
    pub timeout: Duration,
    pub cancelled: &'a AtomicBool,
    pub connection_closed: Option<&'a AtomicBool>,
}

/// Captured output is bounded; a successful exit never hides truncation or a failed reader.
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Streaming consumers choose their own bounded representation instead of buffering raw JSONL.
pub(crate) struct StreamOutput<T> {
    pub status: ExitStatus,
    pub stdout: T,
    pub stderr: Vec<u8>,
}

impl Analyzer<'_> {
    /// A dropped editor connection cancels validation even before a queued shutdown can run.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self
                .connection_closed
                .is_some_and(|closed| closed.load(Ordering::Acquire))
    }
    /// Verify the executable identity and commands, returning its actual release version.
    ///
    /// # Errors
    /// Rejects invalid version output, malformed contracts, process failures and cancellation.
    pub fn validate(&self, root: &Path) -> Result<String, Error> {
        let output = self.run(root, [OsStr::new("--version")])?;
        let actual = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let version = actual.strip_prefix("bsl-analyzer ").filter(|version| {
            let parts: Vec<_> = version.split('.').collect();
            parts.len() == 3
                && parts.iter().all(|part| {
                    !part.is_empty()
                        && (part.len() == 1 || !part.starts_with('0'))
                        && part.bytes().all(|byte| byte.is_ascii_digit())
                })
        });
        let Some(version) = version.filter(|_| output.status.success()) else {
            return Err(Error::AnalyzerVersion { actual });
        };
        let output = self.run(root, [OsStr::new("contract")])?;
        let contract: Value =
            serde_json::from_slice(&output.stdout).map_err(|_| Error::AnalyzerContract)?;
        if !output.status.success() || contract["build_version"].as_str() != Some(version) {
            return Err(Error::AnalyzerContract);
        }
        let commands = contract["cli"]["commands"]
            .as_array()
            .ok_or(Error::AnalyzerContract)?;
        for name in ["check-config", "analyze", "lsp"] {
            let command = commands
                .iter()
                .find(|command| command["name"] == name)
                .ok_or(Error::AnalyzerContract)?;
            if name != "lsp"
                && !command["args"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg["name"] == "config"))
            {
                return Err(Error::AnalyzerContract);
            }
        }
        Ok(version.to_owned())
    }

    /// Validate a config in its final parent directory so every relative policy path keeps its meaning.
    ///
    /// # Errors
    /// Retains upstream validation output and rejects nonzero exits or interrupted validation.
    pub fn check_config(&self, root: &Path, config: &Path) -> Result<(), Error> {
        let output = self.run(
            root,
            [
                OsStr::new("check-config"),
                OsStr::new("--config"),
                config.as_os_str(),
            ],
        )?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::CheckConfig {
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    /// Drain both streams concurrently, then reap the child on success, failure, timeout or cancellation.
    ///
    /// # Errors
    /// Rejects spawn/read/wait failures, oversized output, timeout and explicit cancellation.
    pub fn run<'a>(
        &self,
        root: &Path,
        args: impl IntoIterator<Item = &'a OsStr>,
    ) -> Result<Output, Error> {
        let output = self.run_with_stdout(root, args, |stdout| read_bounded(stdout))?;
        Ok(Output {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Share cancellation, bounded stderr and reaping with the incremental JSONL reader.
    pub(crate) fn run_with_stdout<'a, T: Send + 'static>(
        &self,
        root: &Path,
        args: impl IntoIterator<Item = &'a OsStr>,
        read_stdout: impl FnOnce(&mut dyn Read) -> Result<T, Error> + Send + 'static,
    ) -> Result<StreamOutput<T>, Error> {
        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut child = Command::new(&self.binary)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::ProcessStart {
                binary: self.binary.clone(),
                source,
            })?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::ProcessIo(io::Error::other(
                "capture pipes unavailable",
            )));
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let mut stdout = CapturePipe {
            pipe: stdout,
            stopped: Arc::clone(&stopped),
        };
        let stderr = CapturePipe {
            pipe: stderr,
            stopped: Arc::clone(&stopped),
        };
        let stdout = match thread::Builder::new()
            .name("eska-analyzer-stdout".into())
            .spawn(move || read_stdout(&mut stdout))
        {
            Ok(reader) => reader,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::ProcessIo(source));
            }
        };
        let stderr = match thread::Builder::new()
            .name("eska-analyzer-stderr".into())
            .spawn(move || read_bounded(stderr))
        {
            Ok(reader) => reader,
            Err(source) => {
                stopped.store(true, Ordering::Release);
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::ProcessIo(source));
            }
        };
        let started = Instant::now();
        let mut status = None;
        let result = loop {
            if self.is_cancelled() {
                break Err(Error::Cancelled);
            }
            let remaining = self.timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break Err(Error::Timeout);
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(exit) => status = exit,
                    Err(source) => break Err(Error::ProcessIo(source)),
                }
            }
            if let Some(status) = status
                && stdout.is_finished()
                && stderr.is_finished()
            {
                break Ok(status);
            }
            thread::sleep(remaining.min(Duration::from_millis(10)));
        };
        if result.is_err() {
            stopped.store(true, Ordering::Release);
            if status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        // An unowned descendant can hold a pipe open after the direct child was reaped.
        // On interruption, dropping unfinished handles lets the caller return while reads unwind.
        let status = result?;
        let stdout = stdout.join();
        let stderr = stderr.join();
        let stdout =
            stdout.map_err(|_| Error::ProcessIo(io::Error::other("stdout reader stopped")))?;
        let stderr =
            stderr.map_err(|_| Error::ProcessIo(io::Error::other("stderr reader stopped")))?;
        Ok(StreamOutput {
            status,
            stdout: stdout?,
            stderr: stderr?,
        })
    }
}

struct CapturePipe<R> {
    pipe: R,
    stopped: Arc<AtomicBool>,
}

impl<R: Read> Read for CapturePipe<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.stopped.load(Ordering::Acquire) {
            return Ok(0);
        }
        self.pipe.read(buffer)
    }
}

/// Retain a bounded prefix while draining the rest so output saturation cannot deadlock a child.
fn read_bounded(mut stream: impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    stream
        .by_ref()
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::ProcessIo)?;
    if bytes.len() as u64 > MAX_OUTPUT {
        io::copy(&mut stream, &mut io::sink()).map_err(Error::ProcessIo)?;
        return Err(Error::OutputTooLarge);
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    fn shell(cancelled: &AtomicBool, timeout: Duration) -> Analyzer<'_> {
        Analyzer {
            binary: "sh".into(),
            timeout,
            cancelled,
            connection_closed: None,
        }
    }

    #[test]
    fn deadline_covers_inherited_stderr_after_direct_child_exits() {
        let directory = TestDir::new();
        let cancelled = AtomicBool::new(false);
        let result = shell(&cancelled, Duration::from_millis(300)).run(
            &directory.0,
            ["-c", "sleep 2 >/dev/null & exit 0"].map(OsStr::new),
        );
        assert!(matches!(result, Err(Error::Timeout)));
    }

    #[test]
    fn cancellation_covers_inherited_stderr_after_stdout_reaches_eof() {
        let directory = TestDir::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        let result = shell(&cancelled, Duration::from_secs(1)).run_with_stdout(
            &directory.0,
            ["-c", "sleep 2 >/dev/null & exit 0"].map(OsStr::new),
            move |stdout| {
                let output = read_bounded(stdout)?;
                // Leave the worker waiting for the other stream, held open by sleep.
                thread::sleep(Duration::from_millis(50));
                trigger.store(true, Ordering::Release);
                Ok(output)
            },
        );
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[test]
    fn captures_both_streams_and_preserves_stdout_reader_failure() {
        let directory = TestDir::new();
        let cancelled = AtomicBool::new(false);
        let analyzer = shell(&cancelled, Duration::from_secs(5));
        let args = ["-c", "printf out; printf err >&2; exit 7"].map(OsStr::new);
        let output = analyzer.run(&directory.0, args).unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");

        let result =
            analyzer.run_with_stdout(&directory.0, ["-c", "exit 0"].map(OsStr::new), |_| {
                Err::<(), _>(Error::OutputTooLarge)
            });
        assert!(matches!(result, Err(Error::OutputTooLarge)));
    }
}
