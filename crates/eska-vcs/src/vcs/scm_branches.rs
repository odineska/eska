//! Checked reference selection for the SCM branch picker.
use super::{
    command::Executor,
    repository::{Head, ReferenceTarget, Repository},
};
use gix::bstr::ByteSlice;
use serde_json::{Value, json};
use std::path::Path;

type Reply = Result<Value, Value>;
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Kind {
    Head,
    Local,
    Remote,
    Tag,
    Commit,
}
fn repository(start: &Path) -> Result<Repository, Value> {
    Repository::discover(start).map_err(|_| json!("repository"))
}
pub fn refs(start: &Path) -> Reply {
    let repo = repository(start)?;
    let mut refs = Vec::new();
    for reference in repo.references().map_err(|_| json!("references"))? {
        if matches!(reference.target, ReferenceTarget::Symbolic(_)) {
            continue;
        }
        for (prefix, kind) in [
            ("refs/heads/", "local"),
            ("refs/remotes/", "remote"),
            ("refs/tags/", "tag"),
        ] {
            if let Some(name) = reference.name.strip_prefix(prefix.as_bytes()) {
                let Ok(name) = std::str::from_utf8(name) else {
                    continue;
                };
                refs.push(json!({"name":name,"kind":kind}));
            }
        }
    }
    Ok(json!({"refs":refs}))
}
fn target(repo: &Repository, kind: Kind, name: &str) -> Result<String, Value> {
    let prefix = match kind {
        Kind::Head if name == "HEAD" => return Ok("HEAD".into()),
        Kind::Commit
            if (7..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            let prefix = gix::hash::Prefix::from_hex(name).map_err(|_| json!("branch_invalid"))?;
            let id = repo
                .inner
                .objects
                .lookup_prefix(prefix, None)
                .map_err(|_| json!("branch_not_found"))?
                .ok_or_else(|| json!("branch_not_found"))?
                .map_err(|()| json!("branch_not_found"))?;
            let commit = repo
                .inner
                .find_object(id)
                .and_then(gix::Object::try_into_commit)
                .map_err(|_| json!("branch_not_found"))?;
            return Ok(commit.id.to_string());
        }
        Kind::Local => "refs/heads/",
        Kind::Remote => "refs/remotes/",
        Kind::Tag => "refs/tags/",
        _ => return Err(json!("branch_invalid")),
    };
    let full = format!("{prefix}{name}");
    if !repo
        .references()
        .map_err(|_| json!("references"))?
        .iter()
        .any(|reference| {
            reference.name.as_slice() == full.as_bytes()
                && !matches!(reference.target, ReferenceTarget::Symbolic(_))
        })
    {
        return Err(json!("branch_not_found"));
    }
    Ok(full)
}
pub fn change(start: &Path, kind: Kind, name: &str, branch: Option<&str>, detached: bool) -> Reply {
    let repo = repository(start)?;
    let target = target(&repo, kind, name)?;
    if repo
        .inner
        .index_or_empty()
        .map_err(|_| json!("index"))?
        .entries()
        .iter()
        .any(|entry| entry.stage() != gix::index::entry::Stage::Unconflicted)
    {
        return Err(json!("checkout_failed"));
    }
    if let Some(branch) = branch {
        if branch.is_empty()
            || branch.starts_with('-')
            || branch == "@"
            || gix::validate::reference::branch_name(
                format!("refs/heads/{branch}").as_bytes().as_bstr(),
            )
            .is_err()
        {
            return Err(json!("branch_invalid"));
        }
        if repo
            .references()
            .map_err(|_| json!("references"))?
            .iter()
            .any(|reference| reference.name.as_slice() == format!("refs/heads/{branch}").as_bytes())
        {
            return Err(json!("branch_exists"));
        }
    }
    let executor = Executor::new(repo.work_dir());
    if !executor.git_available() {
        return Err(json!("git_unavailable"));
    }
    // Preserve branch creation before the first commit: no checkout or shelf is possible.
    if matches!(
        repo.head().map_err(|_| json!("repository"))?,
        Head::Unborn { .. }
    ) && matches!(kind, Kind::Head)
        && branch.is_some()
    {
        executor
            .scm_switch(kind, name, &target, branch, detached)
            .map_err(|_| json!("checkout_failed"))?;
        return Ok(json!({"saved":false,"restored":false}));
    }
    if matches!(kind, Kind::Local) && branch.is_none() && !detached {
        let result =
            crate::project::switch::execute_branch(start, name, false).map_err(switch_error)?;
        return Ok(json!({"saved":result.saved.is_some(),"restored":result.restored.is_some()}));
    }
    let saved =
        crate::project::switch::execute_selection(&repo, kind, name, &target, branch, detached)
            .map_err(switch_error)?;
    Ok(json!({"saved":saved,"restored":false}))
}

fn switch_error(error: crate::project::switch::SwitchError) -> Value {
    use crate::project::switch::SwitchError;
    json!(match error {
        SwitchError::Shelf(error) => super::shelves::error_code(&error),
        SwitchError::InvalidBranch => "branch_invalid",
        SwitchError::BranchMissing { .. } => "branch_not_found",
        SwitchError::TargetCheckedOut { .. } => "target-checked-out",
        SwitchError::Command(_) => "checkout_failed",
        _ => "repository",
    })
}
