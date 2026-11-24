//! Reconcile filesystem invalidations against authoritative, read-only Git snapshots.

use std::{
    path::{Path, PathBuf},
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
    pub scm: super::scm_details::Details,
    autocrlf: Option<BString>,
}

/// Owns the current coverage strategy and the latest reconciled view.
pub struct Monitor {
    coverage: Coverage,
    snapshot: Snapshot,
}

enum Coverage {
    Native {
        watcher: Box<gix::notify::RepositoryMonitor>,
        recovering_since: Option<Instant>,
    },
    Polling {
        next_scan: Instant,
    },
}

const POLL_INTERVAL: Duration = Duration::from_secs(5);

impl Repository {
    /// Register native coverage before taking the baseline, or poll if it is unavailable.
    ///
    /// # Errors
    /// Returns an error if the repository snapshot cannot be read.
    pub fn monitor(&self) -> Result<Monitor, Error> {
        let coverage = Coverage::register(self);
        Ok(Monitor {
            coverage,
            snapshot: Snapshot::read(self)?,
        })
    }
}

impl Coverage {
    fn register(repository: &Repository) -> Self {
        if let Ok(mut watcher) = repository.inner.monitor(gix::notify::Options {
            metadata_debounce: Duration::from_millis(45),
            retry_interval: Duration::from_millis(100),
            max_latency: Duration::from_millis(100),
            worktree_coverage: Some(gix::notify::WorktreeCoverage::IgnoreAware),
            ..Default::default()
        }) {
            // Register both domains first so events during the baseline remain queued.
            let outcome = watcher.service(Instant::now(), || open(repository.work_dir()));
            if outcome.errors.is_empty() && !outcome.recovery_pending {
                return Self::Native {
                    watcher: Box::new(watcher),
                    recovering_since: None,
                };
            }
        }
        // Drop even a partially installed watcher before reading the baseline.
        Self::Polling {
            next_scan: Instant::now() + POLL_INTERVAL,
        }
    }

    fn next_changes(&mut self, root: &Path) -> Changes {
        let (watcher, recovering_since) = match self {
            Self::Polling { next_scan } => {
                let delay = next_scan.saturating_duration_since(Instant::now());
                if !delay.is_zero() {
                    std::thread::sleep(delay.min(Duration::from_millis(200)));
                    return Changes::default();
                }
                return full_refresh();
            }
            Self::Native {
                watcher,
                recovering_since,
            } => (watcher, recovering_since),
        };
        let delay = watcher
            .next_timeout(Instant::now())
            .unwrap_or(Duration::from_millis(40))
            .min(Duration::from_millis(40));
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let outcome = watcher.service(Instant::now(), || open(root));
        if !outcome.errors.is_empty() || outcome.recovery_pending {
            if recovering_since.get_or_insert_with(Instant::now).elapsed() < Duration::from_secs(2)
            {
                return Changes::default();
            }
            // A partial native registration cannot supply reliable path hints.
            *self = Self::Polling {
                next_scan: Instant::now(),
            };
            return full_refresh();
        }
        if recovering_since.take().is_some() {
            return full_refresh();
        }
        outcome.changes
    }
}

fn full_refresh() -> Changes {
    Changes {
        worktree: Some(Scope::All),
        ..Changes::default()
    }
}

impl Monitor {
    #[must_use]
    pub const fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub(crate) const fn is_polling(&self) -> bool {
        matches!(self.coverage, Coverage::Polling { .. })
    }

    /// Wait for a different complete snapshot while the caller still needs the stream.
    ///
    /// # Errors
    /// Returns an error if the repository disappears or its authoritative status cannot be read.
    pub fn next_snapshot_while(
        &mut self,
        mut keep_running: impl FnMut() -> bool,
    ) -> Result<Option<&Snapshot>, Error> {
        while keep_running() {
            let changes = self.coverage.next_changes(&self.snapshot.root);
            if changes.is_empty() {
                continue;
            }
            let publish = self.snapshot.reconcile(&changes)?;
            if let Coverage::Polling { next_scan } = &mut self.coverage {
                // Start the pause after the scan even on slow and remote filesystems.
                *next_scan = Instant::now() + POLL_INTERVAL;
            }
            if publish {
                return Ok(Some(&self.snapshot));
            }
        }
        Ok(None)
    }
}

impl Snapshot {
    /// Read a baseline without installing filesystem watches.
    ///
    /// # Errors
    /// Returns an error if HEAD or status cannot be read.
    pub(crate) fn read(repository: &Repository) -> Result<Self, Error> {
        let autocrlf = repository.status_autocrlf()?;
        let status = repository.status_paths(&[], autocrlf.as_ref())?;
        let scm = super::scm_details::Details::read(repository, &status)?;
        Ok(Self {
            root: repository.work_dir().to_owned(),
            head: repository.head()?,
            status,
            scm,
            autocrlf,
        })
    }

    /// Reconcile invalidations against a fresh authoritative repository handle.
    ///
    /// # Errors
    /// Returns an error if the repository disappeared or its snapshot cannot be read.
    pub(crate) fn reconcile(&mut self, changes: &Changes) -> Result<bool, Error> {
        let root = &self.root;
        // Fresh handles discard index, reference, ignore and configuration caches.
        let repository = Repository::discover(root)?;
        if repository.work_dir() != root {
            return Err(Error::NotFound {
                start: root.clone(),
            });
        }
        let head = repository.head()?;
        let autocrlf = repository.status_autocrlf()?;
        let paths = if head == self.head && autocrlf == self.autocrlf {
            narrow_paths(&repository, changes)?
        } else {
            None
        };
        let status = match paths {
            Some(paths) => {
                let partial = repository.status_paths(&paths, autocrlf.as_ref())?;
                let mut entries = self.status.entries.clone();
                entries.retain(|entry| !paths.iter().any(|path| within(&entry.path, path)));
                entries.extend(partial.entries);
                entries.sort_by(|a, b| a.path.cmp(&b.path));
                Status { entries }
            }
            None => repository.status_paths(&[], autocrlf.as_ref())?,
        };
        let scm = super::scm_details::Details::read(&repository, &status)?;
        self.autocrlf = autocrlf;
        if head != self.head || status != self.status || scm != self.scm {
            self.scm = scm;
            self.head = head;
            self.status = status;
            return Ok(true);
        }
        Ok(false)
    }
}

fn open(root: &std::path::Path) -> gix::Result<gix::Repository> {
    gix::open_opts(root, gix::open::Options::isolated())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_support::TestDir, vcs::git};

    #[test]
    fn polling_reconciles_missed_paths_and_keeps_the_pause_after_a_scan() {
        let dir = TestDir::new();
        git::initialize(&dir.0).expect("initialize test repository");
        let repository = Repository::discover(&dir.0).expect("open test repository");
        let mut monitor = Monitor {
            coverage: Coverage::Polling {
                next_scan: Instant::now(),
            },
            snapshot: Snapshot::read(&repository).expect("baseline"),
        };
        std::fs::write(dir.0.join("outside-editor.bsl"), "changed").expect("write file");
        let snapshot = monitor
            .next_snapshot_while(|| true)
            .expect("poll")
            .expect("change");
        assert_eq!(snapshot.status.entries.len(), 1);
        assert_eq!(
            snapshot.status.entries[0].path.as_slice(),
            b"outside-editor.bsl"
        );
        assert!(
            matches!(monitor.coverage, Coverage::Polling { next_scan } if next_scan > Instant::now())
        );
        assert!(
            monitor
                .next_snapshot_while(|| false)
                .expect("disconnect")
                .is_none()
        );
    }
}
