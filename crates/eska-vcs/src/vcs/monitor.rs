//! Native invalidations reconciled against authoritative, read-only Git snapshots.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gix::{
    bstr::{BStr, BString},
    notify::{Changes, Scope},
};

use super::{
    repository::{Error, Head, Operation, Repository},
    status::Status,
};

/// A repository-wide view; metadata project selection does not constrain this stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub root: PathBuf,
    pub head: Head,
    pub status: Status,
}

/// Owns native watches and the latest reconciled view. Dropping it releases the watches.
pub struct Monitor {
    watcher: gix::notify::RepositoryMonitor,
    snapshot: Snapshot,
    recovery_started: Option<Instant>,
    recovery_error: Option<Error>,
}

impl Repository {
    /// Register ignore-aware native coverage before reading the initial snapshot.
    ///
    /// # Errors
    /// Returns a status error if registration, repository opening or snapshot reading fails.
    pub fn monitor(&self) -> Result<Monitor, Error> {
        let mut watcher = self
            .inner
            .monitor(gix::notify::Options {
                metadata_debounce: Duration::from_millis(45),
                retry_interval: Duration::from_millis(100),
                max_latency: Duration::from_millis(100),
                worktree_coverage: Some(gix::notify::WorktreeCoverage::IgnoreAware),
                ..Default::default()
            })
            .map_err(|source| Error::operation(Operation::Status, source))?;
        let root = self.work_dir().to_owned();
        // service() registers both domains. Events arriving during the baseline remain queued.
        let outcome = watcher.service(Instant::now(), || open(&root));
        check_outcome(outcome)?;
        Ok(Monitor {
            watcher,
            recovery_started: None,
            recovery_error: None,
            snapshot: Snapshot {
                root,
                head: self.head()?,
                status: self.status()?,
            },
        })
    }
}

impl Monitor {
    #[must_use]
    pub const fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Wait for a different complete snapshot while the caller still needs the stream.
    /// The predicate runs between bounded native-service intervals, without extra status scans.
    ///
    /// # Errors
    /// Coverage failures are terminal: an incomplete watch must never look like a quiet repository.
    pub fn next_snapshot_while(
        &mut self,
        mut keep_running: impl FnMut() -> bool,
    ) -> Result<Option<&Snapshot>, Error> {
        loop {
            if !keep_running() {
                return Ok(None);
            }
            let delay = self
                .watcher
                .next_timeout(Instant::now())
                .unwrap_or(Duration::from_millis(40))
                .min(Duration::from_millis(40));
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let root = &self.snapshot.root;
            let outcome = self.watcher.service(Instant::now(), || open(root));
            let mut changes = outcome.changes;
            let failed = !outcome.errors.is_empty() || outcome.recovery_pending;
            if failed {
                if let Some(error) = outcome.errors.into_iter().next() {
                    self.recovery_error = Some(Error::operation(Operation::Status, error));
                }
                let started = self.recovery_started.get_or_insert_with(Instant::now);
                if started.elapsed() >= Duration::from_secs(2) {
                    return Err(self.recovery_error.take().unwrap_or_else(|| {
                        Error::operation(
                            Operation::Status,
                            std::io::Error::other("native repository coverage unavailable"),
                        )
                    }));
                }
                continue;
            }
            if self.recovery_started.take().is_some() {
                self.recovery_error = None;
                changes.configuration = true;
                changes.worktree = Some(Scope::All);
            }
            if changes.is_empty() {
                continue;
            }
            // Fresh handles discard index, reference, ignore and configuration caches.
            let repository = Repository::discover(root)?;
            if repository.work_dir() != root {
                return Err(Error::NotFound {
                    start: root.clone(),
                });
            }
            let head = repository.head()?;
            let paths = if head == self.snapshot.head {
                narrow_paths(&repository, &changes)?
            } else {
                None
            };
            let status = match paths {
                Some(paths) => {
                    let partial = repository.status_paths(&paths)?;
                    let mut entries = self.snapshot.status.entries.clone();
                    entries.retain(|entry| !paths.iter().any(|path| within(&entry.path, path)));
                    entries.extend(partial.entries);
                    entries.sort_by(|a, b| a.path.cmp(&b.path));
                    Status { entries }
                }
                None => repository.status()?,
            };
            if head != self.snapshot.head || status != self.snapshot.status {
                self.snapshot.head = head;
                self.snapshot.status = status;
                return Ok(Some(&self.snapshot));
            }
        }
    }
}

fn open(root: &std::path::Path) -> gix::Result<gix::Repository> {
    gix::open_opts(root, gix::open::Options::isolated())
}

fn check_outcome(outcome: gix::notify::Outcome) -> Result<Changes, Error> {
    if let Some(error) = outcome.errors.into_iter().next() {
        return Err(Error::operation(Operation::Status, error));
    }
    if outcome.recovery_pending {
        return Err(Error::operation(
            Operation::Status,
            std::io::Error::other("native repository coverage unavailable"),
        ));
    }
    Ok(outcome.changes)
}

fn within(path: &[u8], scope: &[u8]) -> bool {
    path == scope || (path.starts_with(scope) && path.get(scope.len()) == Some(&b'/'))
}

fn narrow_paths(repository: &Repository, changes: &Changes) -> Result<Option<Vec<BString>>, Error> {
    if changes.references
        || changes.index
        || changes.configuration
        || changes.ignores
        || changes.attributes
        || changes.layout
        || changes.operations
    {
        return Ok(None);
    }
    let Some(Scope::Paths(paths)) = &changes.worktree else {
        return Ok(None);
    };
    if paths.is_empty() || paths.iter().any(|entry| entry.path.is_empty()) {
        return Ok(None);
    }
    // Byte-prefix reconciliation cannot safely model case folding or Unicode decomposition.
    let config = repository.inner.config_snapshot();
    if config.boolean("core.ignoreCase").unwrap_or(false)
        || config.boolean("core.precomposeUnicode").unwrap_or(false)
    {
        return Ok(None);
    }
    let index = repository
        .inner
        .index_or_empty()
        .map_err(|source| Error::operation(Operation::Status, source))?;
    // A nested event belongs to the containing gitlink, not to a pathspec below it.
    let mut scopes: Vec<BString> = paths.iter().map(|entry| entry.path.clone()).collect();
    for scope in &mut scopes {
        for end in (0..scope.len()).rev() {
            if scope[end] == b'/'
                && index
                    .entry_by_path(BStr::new(&scope[..end]))
                    .is_some_and(|entry| entry.mode.is_submodule())
            {
                scope.truncate(end);
                break;
            }
        }
    }
    scopes.sort();
    scopes.dedup();
    Ok(Some(scopes))
}
