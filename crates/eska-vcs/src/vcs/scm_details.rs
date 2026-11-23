//! Exact index metadata for SCM snapshots, without a second status traversal.
use super::{
    repository::{Error, Operation, Repository},
    status::{Change, Status},
};
use gix::{bstr::ByteSlice, index::entry::Stage};
use std::collections::BTreeMap;

pub type Rename = (Vec<u8>, Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Details {
    pub renames: Vec<Rename>,
    pub conflicts: Vec<Conflict>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: Vec<u8>,
    pub base: bool,
    pub ours: bool,
    pub theirs: bool,
}
impl Details {
    /// Read details only for relevant status entries.
    ///
    /// # Errors
    /// Returns an error if the required index or HEAD metadata cannot be read.
    pub fn read(repo: &Repository, status: &Status) -> Result<Self, Error> {
        let mut details = Self::default();
        if status
            .entries
            .iter()
            .any(|entry| entry.index == Some(Change::Conflict))
        {
            let index = repo
                .inner
                .index_or_empty()
                .map_err(|source| Error::operation(Operation::Status, source))?;
            for entry in status
                .entries
                .iter()
                .filter(|entry| entry.index == Some(Change::Conflict))
            {
                let has = |stage| {
                    index
                        .entry_by_path_and_stage(entry.path.as_bstr(), stage)
                        .is_some()
                };
                details.conflicts.push(Conflict {
                    path: entry.path.to_vec(),
                    base: has(Stage::Base),
                    ours: has(Stage::Ours),
                    theirs: has(Stage::Theirs),
                });
            }
        }
        if status
            .entries
            .iter()
            .any(|entry| entry.index == Some(Change::Added))
            && status
                .entries
                .iter()
                .any(|entry| entry.index == Some(Change::Deleted))
        {
            details.renames = renames(repo, status)?;
        }
        Ok(details)
    }
}
/// Pair only unique identical blobs; ambiguous or edited moves remain separate changes.
pub(super) fn renames(repo: &Repository, status: &Status) -> Result<Vec<Rename>, Error> {
    let Some(head) = repo.head()?.id() else {
        return Ok(Vec::new());
    };
    let tree = repo
        .inner
        .find_commit(head)
        .and_then(|commit| commit.tree())
        .map_err(|source| Error::operation(Operation::Status, source))?;
    let index = repo
        .inner
        .index_or_empty()
        .map_err(|source| Error::operation(Operation::Status, source))?;
    let mut removed = BTreeMap::<gix::ObjectId, Vec<Vec<u8>>>::new();
    let mut added = BTreeMap::<gix::ObjectId, Vec<Vec<u8>>>::new();
    for entry in &status.entries {
        match entry.index {
            Some(Change::Deleted) => {
                let path = gix::path::from_bstr(entry.path.as_bstr())
                    .map_err(|source| Error::operation(Operation::Status, source))?;
                if let Some(old) = tree
                    .lookup_entry_by_path(path.as_ref())
                    .map_err(|source| Error::operation(Operation::Status, source))?
                {
                    removed
                        .entry(old.object_id())
                        .or_default()
                        .push(entry.path.to_vec());
                }
            }
            Some(Change::Added) => {
                if let Some(new) = index.entry_by_path(entry.path.as_bstr()) {
                    added.entry(new.id).or_default().push(entry.path.to_vec());
                }
            }
            _ => {}
        }
    }
    Ok(removed
        .into_iter()
        .filter_map(|(id, old)| {
            let new = added.get(&id)?;
            (old.len() == 1 && new.len() == 1).then(|| (old[0].clone(), new[0].clone()))
        })
        .collect())
}
