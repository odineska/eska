//! Verify the configured production source branch before launching or attaching a debugger.

use eska_project_core::{
    application::project_application,
    project::{
        Project,
        discovery::{self, DiscoveryContext},
    },
};
use gix::ObjectId;
use serde::Serialize;

use crate::vcs::{
    command::Executor,
    network,
    repository::{Divergence, Head, ReferenceTarget, Repository},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Current,
    Different,
    Unverified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Update {
    FastForward,
    SwitchMain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Workflow,
    Repository,
    Remote,
    MainMissing,
    RemoteBranchMissing,
}

#[derive(Debug, Serialize)]
pub struct SourceCheck {
    pub status: Status,
    pub reason: Option<Reason>,
    pub main_branch: Option<String>,
    pub remote: Option<String>,
    pub active_branch: Option<String>,
    pub local_main_ahead: Option<usize>,
    pub local_main_behind: Option<usize>,
    pub active_ahead: Option<usize>,
    pub active_behind: Option<usize>,
    pub dirty: Option<bool>,
    pub unsaved: bool,
    pub update: Option<Update>,
}

#[derive(Debug)]
pub enum UpdateError {
    NoLongerSafe,
    Failed,
}

/// Fetch the configured remote and compare active and local main sources with its advertised main.
#[must_use]
pub fn check(project: &Project, unsaved: bool) -> SourceCheck {
    inspect(project, unsaved).0
}

/// Recheck and perform only the update explicitly chosen by the developer.
/// # Errors
/// Rejects an unsafe or stale update or a failure to switch or fast-forward the branch.
pub fn update(project: &Project, unsaved: bool, requested: Update) -> Result<(), UpdateError> {
    let (check, verified) = inspect(project, unsaved);
    if check.update != Some(requested) {
        return Err(UpdateError::NoLongerSafe);
    }
    let (repository, expected_head, main_id, remote_id) =
        verified.ok_or(UpdateError::NoLongerSafe)?;
    let main = check
        .main_branch
        .as_deref()
        .ok_or(UpdateError::NoLongerSafe)?;
    let executor = Executor::new(repository.work_dir());
    if repository
        .status()
        .map_err(|_| UpdateError::NoLongerSafe)?
        .is_dirty()
        || repository.head().map_err(|_| UpdateError::NoLongerSafe)? != expected_head
    {
        return Err(UpdateError::NoLongerSafe);
    }
    if requested == Update::SwitchMain {
        executor
            .switch_existing_branch(main)
            .map_err(|_| UpdateError::Failed)?;
    }
    let repository = Repository::discover(project.root()).map_err(|_| UpdateError::Failed)?;
    if !matches!(repository.head(), Ok(Head::Attached { reference, id })
        if reference.as_slice() == format!("refs/heads/{main}").as_bytes() && id == main_id)
        || repository
            .status()
            .map_err(|_| UpdateError::Failed)?
            .is_dirty()
    {
        return Err(UpdateError::NoLongerSafe);
    }
    if main_id != remote_id {
        executor
            .fast_forward_current(&remote_id.to_string())
            .map_err(|_| UpdateError::Failed)?;
    }
    let repository = Repository::discover(project.root()).map_err(|_| UpdateError::Failed)?;
    if repository.head().map_err(|_| UpdateError::Failed)?.id() != Some(remote_id)
        || repository
            .status()
            .map_err(|_| UpdateError::Failed)?
            .is_dirty()
    {
        return Err(UpdateError::Failed);
    }
    Ok(())
}

type Verified = (Repository, Head, ObjectId, ObjectId);

fn inspect(project: &Project, unsaved: bool) -> (SourceCheck, Option<Verified>) {
    let mut check = SourceCheck {
        status: Status::Unverified,
        reason: None,
        main_branch: None,
        remote: None,
        active_branch: None,
        local_main_ahead: None,
        local_main_behind: None,
        active_ahead: None,
        active_behind: None,
        dirty: None,
        unsaved,
        update: None,
    };
    match compare(project, &mut check) {
        Ok(verified) => (check, Some(verified)),
        Err(reason) => {
            check.reason = Some(reason);
            (check, None)
        }
    }
}

fn compare(project: &Project, check: &mut SourceCheck) -> Result<Verified, Reason> {
    let context = discovery::discover_context(project.root()).map_err(|_| Reason::Workflow)?;
    let (project, workspace) = match &context {
        DiscoveryContext::Standalone(project) => (project, None),
        DiscoveryContext::Workspace { workspace, .. } => {
            let member = workspace
                .members()
                .iter()
                .find(|member| member.root() == project.root())
                .ok_or(Reason::Workflow)?;
            (member.project(), Some(workspace))
        }
    };
    let policy = project
        .configuration()
        .workflow_settings()
        .and_then(|settings| settings.resolve(None).ok())
        .ok_or(Reason::Workflow)?;
    let main = policy.main_branch();
    let remote = policy.remote();
    check.main_branch = Some(main.to_owned());
    check.remote = Some(remote.to_owned());
    let local_ref = format!("refs/heads/{main}");
    let repo = Repository::discover(project.root()).map_err(|_| Reason::Repository)?;
    if let Some(workspace) = workspace
        && let Some((_, application)) =
            project_application(project, workspace).map_err(|_| Reason::Workflow)?
    {
        for member in workspace.members().iter().filter(|member| {
            application.configuration() == Some(member.name())
                || application.extensions().contains(member.name())
        }) {
            let source_repo =
                Repository::discover(member.root()).map_err(|_| Reason::Repository)?;
            if source_repo.work_dir() != repo.work_dir() {
                return Err(Reason::Repository);
            }
        }
    }
    let remote_id = network::fetch_branch(&repo, remote, &local_ref).map_err(|_| Reason::Remote);
    let repo = Repository::discover(project.root()).map_err(|_| Reason::Repository)?;
    let head = repo.head().map_err(|_| Reason::Repository)?;
    let attached = if let Head::Attached { reference, .. } = &head {
        check.active_branch = Some(
            String::from_utf8_lossy(reference.strip_prefix(b"refs/heads/").unwrap_or(reference))
                .into_owned(),
        );
        reference.as_slice() == local_ref.as_bytes()
    } else {
        false
    };
    let dirty = repo.status().map_err(|_| Reason::Repository)?.is_dirty();
    check.dirty = Some(dirty);
    let remote_id = remote_id?.ok_or(Reason::RemoteBranchMissing)?;
    let main_id = repo
        .references()
        .map_err(|_| Reason::Repository)?
        .into_iter()
        .find_map(|reference| match reference.target {
            ReferenceTarget::Object(id) if reference.name.as_slice() == local_ref.as_bytes() => {
                Some(id)
            }
            _ => None,
        })
        .ok_or(Reason::MainMissing)?;
    let Divergence { ahead, behind } = repo
        .divergence_between(main_id, remote_id)
        .map_err(|_| Reason::Repository)?;
    check.local_main_ahead = Some(ahead);
    check.local_main_behind = Some(behind);
    if let Some(id) = head.id() {
        let current = repo
            .divergence_between(id, remote_id)
            .map_err(|_| Reason::Repository)?;
        check.active_ahead = Some(current.ahead);
        check.active_behind = Some(current.behind);
    }
    check.status = if attached && head.id() == Some(remote_id) && !dirty && !check.unsaved {
        Status::Current
    } else {
        Status::Different
    };
    if !dirty && !check.unsaved && ahead == 0 {
        if attached && behind > 0 {
            check.update = Some(Update::FastForward);
        } else if !attached && matches!(head, Head::Attached { .. }) {
            check.update = Some(Update::SwitchMain);
        }
    }
    Ok((repo, head, main_id, remote_id))
}
