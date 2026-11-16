//! CLI and IDE authoring share the same ownership, source protections and checked publication.

use std::{collections::BTreeMap, fmt::Write, path::PathBuf};

use super::{
    ProjectSession, RefreshReport, WorkspaceError,
    editing_history::{EditedSource, HistoryStep},
};
use crate::project::{
    designer_source::SourceRole,
    metadata_edit::{EditError, EditPlan, snapshot},
    metadata_form_edit::{
        self, FormChanges, FormEditError, FormEditHistory, FormEditPlan, FormEditRequest,
        FormEditResult, FormEditSchema, FormTypeOption, PreparedFormEdit, RemovedFormTarget,
    },
    metadata_model::{MetadataKind, ObjectId},
    metadata_rename::{
        inventory::{MAX_TEXT_BYTES, read_text},
        transaction::Guard,
    },
    query::bsl,
    source_lock::SourceLease,
};

impl ProjectSession {
    /// Inspect owned managed-form settings and structural choices without writing any source.
    ///
    /// # Errors
    /// Returns ownership, format, containment or malformed form errors.
    pub fn form_edit_schema(&mut self, form: &ObjectId) -> Result<FormEditSchema, FormEditError> {
        let mut current = self.form_authoring_current()?;
        let (path, input) = current.form_authoring_source(form)?;
        let writable = current.form_authoring_writable(form, &path)?;
        metadata_form_edit::inspect(form, path, &input, writable)
    }

    /// Use the current project declarations without opening unrelated source files.
    #[must_use]
    pub fn form_type_options(&self) -> Vec<FormTypeOption> {
        metadata_form_edit::type_options(&self.objects)
    }

    /// Compute a semantic preview with deterministic local identity allocation.
    ///
    /// # Errors
    /// Rejects stale snapshots, unsafe operations or unavailable source write permissions.
    pub fn preview_form_edit(
        &mut self,
        request: &FormEditRequest,
    ) -> Result<FormEditPlan, FormEditError> {
        let mut current = self.form_authoring_current()?;
        let (path, input) = current.form_authoring_source(&request.form)?;
        if !current.form_authoring_writable(&request.form, &path)? {
            return Err(EditError::ReadOnly.into());
        }
        let FormChanges {
            edit,
            assigned_ids,
            removed,
        } = metadata_form_edit::plan_complete(&input, request)?;
        current.form_new_type_references(&input, edit.output())?;
        let mut inputs = BTreeMap::from([(path.clone(), snapshot(&input))]);
        current.form_removed_references(&request.form, &removed, &mut inputs)?;
        Ok(FormEditPlan {
            path,
            inputs: inputs.keys().cloned().collect(),
            prepared: PreparedFormEdit {
                request: request.clone(),
                source_snapshot: inputs_snapshot(&inputs),
                expected_output_snapshot: snapshot(edit.output()),
            },
            assigned_ids,
            edit,
            input_snapshots: inputs,
        })
    }

    /// Reconstruct an approved preview; client-supplied XML is never accepted as a write capability.
    ///
    /// # Errors
    /// Rejects source changes or a result hash that differs from the reviewed semantic request.
    pub fn preview_prepared_form_edit(
        &mut self,
        prepared: &PreparedFormEdit,
    ) -> Result<FormEditPlan, FormEditError> {
        let plan = self.preview_form_edit(&prepared.request)?;
        if plan.prepared.expected_output_snapshot != prepared.expected_output_snapshot
            || plan.prepared.source_snapshot != prepared.source_snapshot
        {
            return Err(EditError::Conflict.into());
        }
        Ok(plan)
    }

    /// Publish exactly one reviewed managed-form payload under the shared metadata write guard.
    ///
    /// # Errors
    /// Returns conflicts, protections and IO errors. `Committed` means the source was written but
    /// refreshing the workspace failed; callers must inspect instead of replaying that request.
    pub fn apply_form_edit(
        &mut self,
        prepared: &PreparedFormEdit,
    ) -> Result<FormEditResult, FormEditError> {
        let guard = Guard::acquire(self.project().root())?;
        guard.check_clear(self.project().source())?;
        let plan = self.preview_prepared_form_edit(prepared)?;
        if plan.is_empty() {
            return Ok(FormEditResult {
                form: prepared.request.form.clone(),
                snapshot: snapshot(plan.output()),
                assigned_ids: plan.assigned_ids,
                refresh: None,
            });
        }
        let after = snapshot(plan.output());
        let history = plan.edit.history();
        let refresh = self.publish_form_plan(&plan.path, &plan.input_snapshots, &plan.edit);
        if refresh.is_ok() || matches!(&refresh, Err(FormEditError::Committed(_))) {
            self.record_edit(
                EditedSource::Form(prepared.request.form.clone()),
                HistoryStep::Scalar(history),
            );
        }
        Ok(FormEditResult {
            form: prepared.request.form.clone(),
            snapshot: after,
            assigned_ids: plan.assigned_ids,
            refresh: Some(refresh?),
        })
    }

    /// A webview may inspect session history without receiving its replacement bytes.
    #[must_use]
    pub fn form_edit_history(&self, form: &ObjectId, expected: &str) -> FormEditHistory {
        let Some(history) = self.edit_history.get(&EditedSource::Form(form.clone())) else {
            return FormEditHistory::default();
        };
        let ready = |step: Option<&HistoryStep>, undo| matches!(step, Some(HistoryStep::Scalar(edit)) if edit.matches_snapshot(expected, undo));
        FormEditHistory {
            undo: ready(history.undo.last(), true),
            redo: ready(history.redo.last(), false),
        }
    }

    /// Replay a backend-owned form edit under the same guard and dependency checks as a new edit.
    ///
    /// # Errors
    /// Rejects stale snapshots, changed permissions, external references and missing history.
    pub fn replay_form_edit(
        &mut self,
        form: &ObjectId,
        expected: &str,
        undo: bool,
    ) -> Result<FormEditResult, FormEditError> {
        let guard = Guard::acquire(self.project().root())?;
        guard.check_clear(self.project().source())?;
        let source = EditedSource::Form(form.clone());
        let history = self
            .edit_history
            .get(&source)
            .ok_or(EditError::HistoryUnavailable)?;
        let step = if undo {
            history.undo.last()
        } else {
            history.redo.last()
        };
        let Some(HistoryStep::Scalar(saved)) = step else {
            return Err(EditError::HistoryUnavailable.into());
        };
        let mut current = self.form_authoring_current()?;
        let (path, input) = current.form_authoring_source(form)?;
        if !current.form_authoring_writable(form, &path)? {
            return Err(EditError::ReadOnly.into());
        }
        if snapshot(&input) != expected {
            return Err(EditError::Conflict.into());
        }
        let edit = saved.replay(&input, undo)?;
        let step = HistoryStep::Scalar(saved.clone());
        current.form_new_type_references(&input, edit.output())?;
        let removals = metadata_form_edit::removed_targets(&input, edit.output())?;
        let mut inputs = BTreeMap::from([(path.clone(), expected.to_owned())]);
        current.form_removed_references(form, &removals, &mut inputs)?;
        let after = snapshot(edit.output());
        let refresh = self.publish_form_plan(&path, &inputs, &edit);
        if refresh.is_ok() || matches!(&refresh, Err(FormEditError::Committed(_))) {
            self.advance_edit(&source, step, undo);
        }
        Ok(FormEditResult {
            form: form.clone(),
            snapshot: after,
            assigned_ids: Vec::new(),
            refresh: Some(refresh?),
        })
    }

    fn publish_form_plan(
        &mut self,
        path: &PathBuf,
        inputs: &BTreeMap<PathBuf, String>,
        edit: &EditPlan,
    ) -> Result<RefreshReport, FormEditError> {
        let _lease = SourceLease::acquire(
            self.project().root(),
            &inputs
                .keys()
                .map(|path| self.project().source().join(path))
                .collect::<Vec<_>>(),
        )
        .map_err(lock_error)?;
        // A dependency may change between preflight and lease acquisition; protect the reviewed set.
        for (path, expected) in inputs {
            let absolute = self
                .source
                .checked_file(path)
                .map_err(WorkspaceError::Source)?
                .ok_or(EditError::Conflict)?;
            let input = read_text(&absolute, MAX_TEXT_BYTES).map_err(EditError::Io)?;
            if snapshot(&input) != *expected {
                return Err(EditError::Conflict.into());
            }
        }
        self.generation
            .checked_add(1)
            .ok_or(WorkspaceError::GenerationExhausted)?;
        edit.publish(self.project().source(), path)?;
        self.changed_paths(std::slice::from_ref(path))
            .map_err(|error| FormEditError::Committed(Box::new(error)))
    }

    fn form_authoring_current(&self) -> Result<Self, FormEditError> {
        let source = self.source.reopen().map_err(WorkspaceError::Source)?;
        let mut current = Self::open(source)?;
        current.source_protections = self.source_protections.clone();
        Ok(current)
    }

    fn form_new_type_references(&mut self, input: &str, output: &str) -> Result<(), FormEditError> {
        let targets = |text: &str| -> Result<Vec<ObjectId>, FormEditError> {
            Ok(crate::project::forms::parse("authoring", text.to_owned())?
                .references
                .into_iter()
                .filter(|reference| {
                    reference.property.namespace.as_deref() == Some("http://v8.1c.ru/8.1/data/core")
                        && matches!(reference.property.name.as_str(), "Type" | "TypeSet")
                })
                .filter_map(|reference| metadata_form_edit::type_reference(&reference.target))
                .collect())
        };
        let before = targets(input)?;
        for target in targets(output)? {
            if !before.contains(&target) {
                self.reveal_declared_object(&target)?;
            }
        }
        Ok(())
    }

    fn form_authoring_source(
        &mut self,
        form: &ObjectId,
    ) -> Result<(PathBuf, String), FormEditError> {
        let object = self.reveal_declared_object(form)?;
        if !matches!(object.kind, MetadataKind::Form | MetadataKind::CommonForm) {
            return Err(FormEditError::Invalid {
                field: "form",
                reason: "not_form",
            });
        }
        let path = self
            .source
            .sources(form)
            .map_err(WorkspaceError::Source)?
            .into_iter()
            .find(|location| {
                location.role == SourceRole::Payload
                    && location
                        .path
                        .file_name()
                        .is_some_and(|name| name == "Form.xml")
            })
            .ok_or(FormEditError::Invalid {
                field: "form",
                reason: "not_managed_form",
            })?
            .path;
        let input = self
            .source
            .read_xml(&path)
            .map_err(WorkspaceError::Source)?
            .ok_or(EditError::Conflict)?;
        Ok((path, input))
    }

    fn form_authoring_writable(
        &mut self,
        form: &ObjectId,
        path: &std::path::Path,
    ) -> Result<bool, FormEditError> {
        let absolute = self.project().source().join(path);
        let available = match SourceLease::check(self.project().root(), &absolute) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(error) => return Err(EditError::Io(error).into()),
        };
        Ok(available
            && self.can_edit(form, path)?
            && !std::fs::metadata(absolute)
                .map_err(EditError::Io)?
                .permissions()
                .readonly())
    }

    /// Identifier and string references are review candidates; dynamic BSL cannot be proven unused.
    fn form_removed_references(
        &self,
        form: &ObjectId,
        removals: &[RemovedFormTarget],
        inputs: &mut BTreeMap<PathBuf, String>,
    ) -> Result<(), FormEditError> {
        if removals.is_empty() {
            return Ok(());
        }
        for source in self.source.sources(form).map_err(WorkspaceError::Source)? {
            if !matches!(source.role, SourceRole::Module(_)) {
                continue;
            }
            if self.source_protections.blocks(&source.path, None) {
                return Err(EditError::ReadOnly.into());
            }
            let absolute = self
                .source
                .checked_file(&source.path)
                .map_err(WorkspaceError::Source)?
                .ok_or(EditError::Conflict)?;
            SourceLease::check(self.project().root(), &absolute).map_err(lock_error)?;
            let module = read_text(&absolute, MAX_TEXT_BYTES).map_err(EditError::Io)?;
            inputs.insert(source.path.clone(), snapshot(&module));
            if removals.iter().all(|removal| removal.force) {
                continue;
            }
            let tokens = bsl::tokens(&module).map_err(|_| FormEditError::Invalid {
                field: "module",
                reason: "unavailable_reference_analysis",
            })?;
            for removal in removals.iter().filter(|removal| !removal.force) {
                let paths: Vec<_> = tokens
                    .iter()
                    .filter(|token| matches!(token.kind, bsl::Kind::Word | bsl::Kind::String))
                    .filter(|token| {
                        let value = module[token.range.clone()].trim_matches('"').to_lowercase();
                        removal.names.contains(&value)
                    })
                    .map(|token| format!("{}:{}", source.path.display(), token.range.start))
                    .collect();
                if !paths.is_empty() {
                    return Err(FormEditError::References {
                        target: removal.target.clone(),
                        paths,
                    });
                }
            }
        }
        Ok(())
    }
}

fn inputs_snapshot(inputs: &BTreeMap<PathBuf, String>) -> String {
    let mut fingerprint = String::from("managed-form-inputs-1\n");
    for (path, hash) in inputs {
        let path = path.to_string_lossy();
        let _ = writeln!(fingerprint, "{}:{path}:{hash}", path.len());
    }
    snapshot(&fingerprint)
}

fn lock_error(error: std::io::Error) -> EditError {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        EditError::Busy
    } else {
        EditError::Io(error)
    }
}
