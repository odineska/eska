//! Descriptor and form editing share bounded history storage with separate source identities.

use super::{ProjectSession, RefreshReport};
use crate::project::{
    metadata_edit::{MAX_SOURCE_HISTORY_BYTES, MAX_SOURCE_HISTORY_STEPS, SavedEdit},
    metadata_model::ObjectId,
    metadata_rename::{RenamePlan, history::SavedRename},
};

/// Undo can change logical identity; callers must replace their old object address.
#[derive(Debug)]
pub struct PropertyReplay {
    pub object_id: ObjectId,
    pub refresh: RefreshReport,
}

/// The next replay operation determines the editor's dirty-document checks and identity handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryOperation {
    Scalar,
    Rename,
    Linked,
}

#[derive(Clone, Debug)]
pub(super) enum HistoryStep {
    Scalar(SavedEdit),
    Rename(Box<SavedRename>),
    Linked(Box<super::editing_linked::SavedLinkedEdit>),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum EditedSource {
    Descriptor(ObjectId),
    Form(ObjectId),
}

impl EditedSource {
    pub(super) const fn object(&self) -> &ObjectId {
        match self {
            Self::Descriptor(id) | Self::Form(id) => id,
        }
    }

    fn rename(self, plan: &RenamePlan) -> Self {
        let renamed = self.object().renamed(&plan.object_id, &plan.new_object_id);
        match self {
            Self::Descriptor(_) => Self::Descriptor(renamed),
            Self::Form(_) => Self::Form(renamed),
        }
    }
}

impl HistoryStep {
    /// Expose the operation category without giving a client any replacement bytes.
    pub const fn operation(&self) -> HistoryOperation {
        match self {
            Self::Scalar(_) => HistoryOperation::Scalar,
            Self::Rename(_) => HistoryOperation::Rename,
            Self::Linked(_) => HistoryOperation::Linked,
        }
    }
    /// Bound retained replacement text and structural paths independently of descriptor size.
    fn bytes(&self) -> usize {
        match self {
            Self::Scalar(step) => step.bytes(),
            Self::Rename(step) => step.bytes(),
            Self::Linked(step) => step.bytes(),
        }
    }
}

/// Histories are session-local and migrate with renamed objects and their descendants.
#[derive(Debug, Default)]
pub(super) struct EditHistory {
    pub undo: Vec<HistoryStep>,
    pub redo: Vec<HistoryStep>,
}

impl ProjectSession {
    /// Keep one ordered sequence, dropping old operations once the existing memory limits are reached.
    pub(super) fn record_property(&mut self, id: &ObjectId, step: HistoryStep) {
        self.record_edit(EditedSource::Descriptor(id.clone()), step);
    }

    pub(super) fn record_edit(&mut self, source: EditedSource, step: HistoryStep) {
        let entry = self.edit_history.entry(source).or_default();
        entry.redo.clear();
        entry.undo.push(step);
        while entry.undo.len() > MAX_SOURCE_HISTORY_STEPS
            || entry.undo.iter().map(HistoryStep::bytes).sum::<usize>() > MAX_SOURCE_HISTORY_BYTES
        {
            entry.undo.remove(0);
        }
        while self.edit_history.len() > 256
            || self
                .edit_history
                .values()
                .flat_map(|history| history.undo.iter().chain(&history.redo))
                .map(HistoryStep::bytes)
                .sum::<usize>()
                > 32 * 1024 * 1024
        {
            self.edit_history.pop_first();
        }
    }

    /// Preserve unrelated histories; descendant keys use a segment boundary, never a textual prefix.
    pub(super) fn rename_histories(&mut self, plan: &RenamePlan) {
        self.edit_history = std::mem::take(&mut self.edit_history)
            .into_iter()
            .map(|(source, history)| (source.rename(plan), history))
            .collect();
    }

    /// Move a successful replay between stacks; failed writes leave its original position intact.
    pub(super) fn advance_history(&mut self, id: &ObjectId, step: HistoryStep, undo: bool) {
        self.advance_edit(&EditedSource::Descriptor(id.clone()), step, undo);
    }

    pub(super) fn advance_edit(&mut self, source: &EditedSource, step: HistoryStep, undo: bool) {
        if let Some(history) = self.edit_history.get_mut(source) {
            let (from, to) = if undo {
                (&mut history.undo, &mut history.redo)
            } else {
                (&mut history.redo, &mut history.undo)
            };
            from.pop();
            to.push(step);
        }
    }
}
