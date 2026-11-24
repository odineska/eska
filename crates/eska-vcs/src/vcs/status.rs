//! File status relative to the repository root, without parsing Designer XML.

use std::{collections::BTreeMap, fs, io};

use gix::{
    bstr::{BStr, BString, ByteSlice},
    config::{file::includes, tree::Core},
};

use super::repository::{Error, Operation, Repository};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Modified,
    Deleted,
    TypeChanged,
    Untracked,
    IntentToAdd,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStatus {
    /// Git-relative byte path with `/` separators, without lossy UTF-8 conversion.
    pub path: BString,
    /// HEAD-to-index change. Unmerged entries are always `Conflict` here.
    pub index: Option<Change>,
    /// Index-to-worktree change, including untracked and intent-to-add entries.
    pub worktree: Option<Change>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// One entry per changed path, sorted by byte path. Ignored files are excluded.
    pub entries: Vec<PathStatus>,
}

/// Blob snapshots used to refine a changed metadata XML file without invoking Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVersions {
    pub head: Option<Vec<u8>>,
    pub index: Option<Vec<u8>>,
    pub worktree: Option<Vec<u8>>,
}

impl Status {
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        !self.entries.is_empty()
    }

    /// All changed paths, including both endpoints of moves, without scanning again.
    pub fn changed_paths(&self) -> impl Iterator<Item = &BStr> {
        self.entries.iter().map(|entry| entry.path.as_ref())
    }
}

impl Repository {
    /// Compare HEAD, index and worktree; list individual untracked files across the repository.
    /// Renames are represented as deletion/addition pairs, avoiding similarity scans.
    /// Stat refresh suggestions are discarded, so this operation never writes the index.
    /// Repository-local ignore rules and attributes are handled by `gix`.
    ///
    /// # Errors
    /// Returns an index preflight error or `Operation::Status` for unreadable data or unrepresentable paths.
    pub fn status(&self) -> Result<Status, Error> {
        self.status_paths(&[], &self.status_config()?)
    }

    pub(super) fn status_config(&self) -> Result<gix::config::File, Error> {
        effective_git_config(&self.inner)
            .map_err(|source| Error::operation(Operation::Status, source))
    }

    pub(super) fn status_paths(
        &self,
        paths: &[BString],
        effective: &gix::config::File,
    ) -> Result<Status, Error> {
        self.check_index_size()?;
        let patterns = paths
            .iter()
            .map(|path| {
                let mut pattern = BString::from(":(top,literal)");
                pattern.extend_from_slice(path);
                pattern
            })
            .collect::<Vec<_>>();
        let autocrlf = effective
            .string_implicit(Core::AUTO_CRLF)
            .map(|value| value.unwrap_or_else(|| "true".into()));
        let mut configured;
        let repository = if let Some(value) = autocrlf.as_ref()
            && self
                .inner
                .config_snapshot()
                .string(Core::AUTO_CRLF)
                .as_ref()
                != Some(value)
        {
            configured = self.inner.clone();
            let mut config = configured.config_snapshot_mut();
            config
                .set_value(&Core::AUTO_CRLF, value)
                .and_then(|_| config.commit())
                .map_err(|source| Error::operation(Operation::Status, source))?
        } else {
            &self.inner
        };
        let mut attributes_repository;
        let mut lfs_attributes = if super::lfs::standard_driver(effective) {
            let attributes_source = if let Some(value) = effective.string(Core::ATTRIBUTES_FILE) {
                attributes_repository = repository.clone();
                let mut config = attributes_repository.config_snapshot_mut();
                config
                    .set_value(&Core::ATTRIBUTES_FILE, value.as_bstr())
                    .and_then(|_| config.commit())
                    .map_err(|source| Error::operation(Operation::Status, source))?;
                &attributes_repository
            } else {
                repository
            };
            let index = repository
                .index_or_empty()
                .map_err(|source| Error::operation(Operation::Status, source))?;
            Some(
                attributes_source
                    .attributes_only(
                        &index,
                        gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
                    )
                    .map_err(|source| Error::operation(Operation::Status, source))?,
            )
        } else {
            None
        };
        let platform = repository
            .status(gix::progress::Discard)
            .map_err(|source| Error::operation(Operation::Status, source))?
            .untracked_files(gix::status::UntrackedFiles::Files)
            .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
            .index_worktree_rewrites(None)
            .dirwalk_options(|options| options.empty_patterns_match_prefix(false));
        let iter = platform
            .into_iter(patterns)
            .map_err(|source| Error::operation(Operation::Status, source))?;
        let mut paths = BTreeMap::new();
        for item in iter {
            match item.map_err(|source| Error::operation(Operation::Status, source))? {
                gix::status::Item::TreeIndex(change) => record_index(&mut paths, &change),
                gix::status::Item::IndexWorktree(change) => {
                    if let Some(attributes) = lfs_attributes.as_mut()
                        && super::lfs::is_materialized(repository, attributes, &change)
                            .map_err(|source| Error::operation(Operation::Status, source))?
                    {
                        continue;
                    }
                    record_worktree(&mut paths, &change);
                }
            }
        }
        for entry in paths.values() {
            gix::path::from_bstr(entry.path.as_bstr())
                .map_err(|source| Error::operation(Operation::Status, source))?;
        }
        Ok(Status {
            entries: paths.into_values().collect(),
        })
    }

    pub(super) fn check_index_size(&self) -> Result<(), Error> {
        let path = self.inner.index_path();
        match fs::metadata(&path) {
            // Reject files too short to contain an index header and object-hash checksum.
            // Normal Git updates use rename; concurrent in-place corruption is outside the
            // snapshot guarantees of gix.
            Ok(metadata)
                if metadata.len() < 12 + self.inner.object_hash().len_in_bytes() as u64 =>
            {
                Err(Error::InvalidIndex { path })
            }
            Ok(_) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::Io { path, source }),
        }
    }
}

/// Resolve Git's file scopes and includes for the worktree being compared.
/// Consumers apply only the settings needed for their specific operation.
pub(super) fn effective_git_config(repository: &gix::Repository) -> gix::Result<gix::config::File> {
    let mut options = gix::open::Options::default();
    options.permissions.config.git_binary = true;
    options.permissions.config.env = false;
    options.permissions.config.includes = false;
    let mut config = gix::config(None, &options)?;
    config.append(repository.config_snapshot().plumbing().clone())?;

    // Resolve conditional includes after all file scopes are present, with the
    // actual linked worktree, branch and remote URLs available to Gitoxide.
    let branch = repository.head_name()?;
    let home = gix::path::env::home_dir();
    let install_dir = repository.install_dir().ok();
    config.resolve_includes(gix::config::file::init::Options {
        includes: includes::Options::follow(
            gix::config::path::interpolate::Context {
                home_dir: home.as_deref(),
                git_install_dir: install_dir.as_deref(),
                home_for_user: Some(gix::config::path::interpolate::home_for_user),
            },
            includes::conditional::Context {
                git_dir: Some(repository.git_dir()),
                branch_name: branch.as_ref().map(AsRef::as_ref),
            },
        ),
        ..Default::default()
    })?;
    Ok(config)
}

fn entry<'a>(paths: &'a mut BTreeMap<BString, PathStatus>, path: &BStr) -> &'a mut PathStatus {
    paths.entry(path.to_owned()).or_insert_with(|| PathStatus {
        path: path.to_owned(),
        index: None,
        worktree: None,
    })
}

pub(super) fn record_index(
    paths: &mut BTreeMap<BString, PathStatus>,
    change: &gix::diff::index::Change,
) {
    use gix::diff::index::Change as GitChange;
    let kind = match change {
        GitChange::Addition { .. } => Change::Added,
        GitChange::Deletion { .. } => Change::Deleted,
        GitChange::Modification {
            previous_entry_mode,
            entry_mode,
            ..
        } => {
            // The executable bit is a modification, not a file type change.
            if (*previous_entry_mode == gix::index::entry::Mode::SYMLINK)
                != (*entry_mode == gix::index::entry::Mode::SYMLINK)
                || previous_entry_mode.is_submodule() != entry_mode.is_submodule()
            {
                Change::TypeChanged
            } else {
                Change::Modified
            }
        }
        GitChange::Rewrite {
            source_location,
            copy,
            ..
        } => {
            if !copy {
                entry(paths, source_location).index = Some(Change::Deleted);
            }
            Change::Added
        }
    };
    let value = entry(paths, change.location());
    if value.index != Some(Change::Conflict) {
        value.index = Some(kind);
    }
}

fn record_worktree(
    paths: &mut BTreeMap<BString, PathStatus>,
    change: &gix::status::index_worktree::Item,
) {
    use gix::status::index_worktree::{Item, iter::Summary};
    let Some(summary) = change.summary() else {
        return;
    };
    if let Item::Rewrite {
        source,
        copy: false,
        ..
    } = change
    {
        entry(paths, source.rela_path()).worktree = Some(Change::Deleted);
    }
    let value = entry(paths, change.rela_path());
    let kind = match summary {
        Summary::Added | Summary::Renamed | Summary::Copied => Change::Untracked,
        Summary::Removed => Change::Deleted,
        Summary::Modified => Change::Modified,
        Summary::TypeChange => Change::TypeChanged,
        Summary::IntentToAdd => Change::IntentToAdd,
        Summary::Conflict => {
            value.index = Some(Change::Conflict);
            return;
        }
    };
    value.worktree = Some(kind);
}
