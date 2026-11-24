//! Private, locale-independent SCM adapter boundary.
use crate::vcs::{
    command::Executor,
    monitor::Snapshot,
    repository::{Head, Repository},
    status::Change,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use eska_presentation::encoding::{json_git_text, json_path};
use gix::bstr::ByteSlice;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

type Reply = Result<Value, Value>;
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Revision {
    Head,
    Index,
    Base,
    Ours,
    Theirs,
}
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Action {
    Stage,
    Unstage,
    Discard,
}
fn repository(root: &Path) -> Result<Repository, Value> {
    Repository::discover(root).map_err(|_| json!("repository"))
}

pub fn root(start: &Path) -> Reply {
    let repo = repository(start)?;
    let (root, root_encoding) = json_path(repo.work_dir().as_os_str());
    let git_available = Executor::new(repo.work_dir()).git_available();
    Ok(json!({"root":root,"root_encoding":root_encoding,
        "mutations_available":git_available,"branches_available":git_available,"line_staging_available":true}))
}
/// List existing local branch names without remote guessing or a second repository watcher.
pub fn branches(start: &Path) -> Reply {
    let repo = repository(start)?;
    if !Executor::new(repo.work_dir()).git_available() {
        return Err(json!("git_unavailable"));
    }
    let branches = repo
        .references()
        .map_err(|_| json!("references"))?
        .into_iter()
        .filter_map(|reference| {
            reference.name.strip_prefix(b"refs/heads/").map(|name| {
                let (name, name_encoding) = json_git_text(name);
                json!({"name":name,"name_encoding":name_encoding})
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"branches":branches}))
}

/// Switch only to an exact local branch; Git retains its worktree safety checks.
pub fn checkout(start: &Path, branch: &str) -> Reply {
    let reference = format!("refs/heads/{branch}");
    if branch.is_empty()
        || branch.starts_with('-')
        || branch == "@"
        || gix::validate::reference::branch_name(reference.as_bytes().as_bstr()).is_err()
    {
        return Err(json!("branch_invalid"));
    }
    let repo = repository(start)?;
    if !repo
        .references()
        .map_err(|_| json!("references"))?
        .iter()
        .any(|candidate| candidate.name.as_slice() == reference.as_bytes())
    {
        return Err(json!("branch_not_found"));
    }
    let executor = Executor::new(repo.work_dir());
    if !executor.git_available() {
        return Err(json!("git_unavailable"));
    }
    executor
        .switch_existing_branch(branch)
        .map_err(|_| json!("checkout_failed"))?;
    Ok(json!({}))
}

pub fn status(start: &Path) -> Reply {
    let repo = repository(start)?;
    let snapshot = Snapshot::read(&repo).map_err(|_| json!("status"))?;
    Ok(crate::cli::commands::monitor::document(&snapshot))
}

/// Reject recursive pathspecs, metadata and traversal through foreign worktrees or symlink directories.
pub(super) fn validate(repo: &Repository, path: &Path) -> Result<(), Value> {
    if path.as_os_str().is_empty()
        || path.components().any(
            |part| !matches!(part, Component::Normal(name) if !name.eq_ignore_ascii_case(".git")),
        )
    {
        return Err(json!("path"));
    }
    let index = repo.inner.index_or_empty().map_err(|_| json!("index"))?;
    let mut relative = PathBuf::new();
    let mut full = repo.work_dir().to_owned();
    let count = path.components().count();
    for (number, part) in path.components().enumerate() {
        full.push(part);
        relative.push(part);
        let key = gix::path::into_bstr(relative.as_path()).map_err(|_| json!("path"))?;
        let key = gix::path::to_unix_separators_on_windows(key);
        if index
            .entry_by_path(key.as_ref())
            .is_some_and(|entry| entry.mode.is_submodule())
        {
            return Err(json!("path"));
        }
        if number + 1 < count {
            match fs::symlink_metadata(&full) {
                Ok(meta)
                    if meta.is_dir()
                        && !meta.is_symlink()
                        && fs::symlink_metadata(full.join(".git"))
                            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(json!("path")),
            }
        } else if fs::symlink_metadata(&full).is_ok_and(|meta| meta.is_dir()) {
            return Err(json!("path"));
        }
    }
    Ok(())
}
pub fn blob(start: &Path, revision: Revision, path: &Path) -> Reply {
    let repo = repository(start)?;
    validate(&repo, path)?;
    let bytes = gix::path::into_bstr(path).map_err(|_| json!("path"))?;
    let bytes = gix::path::to_unix_separators_on_windows(bytes);
    let content = repo
        .file_version_reader()
        .map_err(|_| json!("index"))?
        .scm_blob(
            bytes.as_ref(),
            match revision {
                Revision::Head => None,
                Revision::Index => Some(gix::index::entry::Stage::Unconflicted),
                Revision::Base => Some(gix::index::entry::Stage::Base),
                Revision::Ours => Some(gix::index::entry::Stage::Ours),
                Revision::Theirs => Some(gix::index::entry::Stage::Theirs),
            },
        )
        .map_err(|_| json!("blob"))?;
    Ok(json!({"content":content.map(|bytes| STANDARD.encode(&bytes))}))
}

pub fn files_input(start: &Path, action: Action, paths: &[PathBuf], stdin: bool) -> Reply {
    use std::io::Read;
    if !stdin {
        return files(start, action, paths);
    }
    if !paths.is_empty() {
        return Err(json!("paths"));
    }
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .take(262_145)
        .read_to_end(&mut input)
        .map_err(|_| json!("input"))?;
    if input.len() > 262_144 {
        return Err(json!("paths_limit"));
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(&input).map_err(|_| json!("paths"))?;
    files(start, action, &paths)
}
fn files(start: &Path, action: Action, paths: &[PathBuf]) -> Reply {
    if paths.is_empty()
        || paths.len() > 2048
        || paths
            .iter()
            .map(|path| path.as_os_str().len())
            .sum::<usize>()
            > 262_144
    {
        return Err(json!("paths_limit"));
    }
    let repo = repository(start)?;
    let status = repo.status().map_err(|_| json!("status"))?;
    let mut expanded = paths.to_vec();
    expanded.sort();
    expanded.dedup();
    if matches!(action, Action::Unstage) {
        for (from, to) in
            super::scm_details::renames(&repo, &status).map_err(|_| json!("status"))?
        {
            let from = gix::path::from_bstr(from.as_bstr())
                .map_err(|_| json!("path"))?
                .into_owned();
            let to = gix::path::from_bstr(to.as_bstr())
                .map_err(|_| json!("path"))?
                .into_owned();
            if expanded.contains(&from) || expanded.contains(&to) {
                if !expanded.contains(&from) {
                    expanded.push(from);
                }
                if !expanded.contains(&to) {
                    expanded.push(to);
                }
            }
        }
    }
    let index = repo.inner.index_or_empty().map_err(|_| json!("index"))?;
    let mut tracked = Vec::new();
    let mut untracked = Vec::new();
    for path in &expanded {
        validate(&repo, path)?;
        let bytes = gix::path::into_bstr(path.as_path()).map_err(|_| json!("path"))?;
        let bytes = gix::path::to_unix_separators_on_windows(bytes);
        let entry = status
            .entries
            .iter()
            .find(|entry| entry.path.as_slice() == bytes.as_ref())
            .ok_or_else(|| json!("stale_status"))?;
        if matches!(action, Action::Discard) {
            if entry.index == Some(Change::Conflict) {
                return Err(json!("conflict"));
            }
            if index.entry_by_path(bytes.as_ref()).is_some() {
                tracked.push(path.clone());
            } else if entry.worktree == Some(Change::Untracked)
                && fs::symlink_metadata(repo.work_dir().join(path))
                    .is_ok_and(|meta| meta.is_file() || meta.is_symlink())
            {
                untracked.push(path);
            } else {
                return Err(json!("stale_status"));
            }
        } else {
            tracked.push(path.clone());
        }
    }
    let action_name = match action {
        Action::Stage => "stage",
        Action::Unstage => "unstage",
        Action::Discard => "discard",
    };
    if !tracked.is_empty() {
        Executor::new(repo.work_dir())
            .scm_files(
                action_name,
                matches!(repo.head().map_err(|_| json!("head"))?, Head::Unborn { .. }),
                &tracked,
            )
            .map_err(|_| json!("mutation"))?;
    }
    for path in untracked {
        fs::remove_file(repo.work_dir().join(path)).map_err(|_| json!("mutation"))?;
    }
    Ok(json!({}))
}

pub fn stage_lines_input(start: &Path, path: &Path) -> Reply {
    super::scm_stage_lines::input(start, path)
}
