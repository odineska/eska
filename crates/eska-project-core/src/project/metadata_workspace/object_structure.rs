//! Structural operations resolve live ownership and support through the existing project session.

mod context;
mod copying;
mod creating;
mod deleting;
mod discovery;
mod modules;
mod properties;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use super::{ProjectSession, WorkspaceError};
use crate::project::{
    configurator::ConfiguratorSchema,
    metadata_edit::EditError,
    metadata_model::{MetadataKind, ObjectId},
    metadata_objects::{
        ObjectError, ObjectPlan, ObjectRequest, ObjectResult, ObjectSchema, PreparedObject,
        inventory::{InputChecks, Inputs, output_snapshot},
    },
    metadata_rename::transaction::Guard,
    source_lock::SourceLease,
};

impl ProjectSession {
    /// Discover valid owner/kind pairs from the existing configurator schema.
    ///
    /// # Errors
    /// Rejects a missing parent instead of inventing a descriptor path.
    pub fn object_schema(
        &mut self,
        parent: Option<&ObjectId>,
    ) -> Result<ObjectSchema, ObjectError> {
        if self.project().configuration().project_type() == crate::project::ProjectType::Extension {
            return Err(ObjectError::Unsupported(MetadataKind::Configuration));
        }
        let parent = self.canonical_parent(parent);
        let module_owner = parent
            .as_ref()
            .unwrap_or_else(|| self.source.root().id())
            .clone();
        let module_roles = self.missing_module_roles(&module_owner)?;
        let kinds = if let Some(parent) = parent.as_ref() {
            let object = self.reveal_declared_object(parent)?;
            ConfiguratorSchema::collections(object.kind).to_vec()
        } else {
            MetadataKind::ALL
                .iter()
                .copied()
                .filter(|kind| kind.collection_folder().is_some())
                .collect()
        };
        Ok(ObjectSchema {
            schema_version: 1,
            required_properties: kinds
                .iter()
                .copied()
                .filter_map(|kind| {
                    let required =
                        crate::project::metadata_objects::templates::required_properties(kind);
                    (!required.is_empty()).then(|| {
                        (
                            kind.as_str().to_owned(),
                            required.iter().map(|key| (*key).to_owned()).collect(),
                        )
                    })
                })
                .collect(),
            reference_properties: kinds
                .iter()
                .copied()
                .filter_map(|kind| {
                    let keys: Vec<_> =
                        crate::project::metadata_edit::reference_lists::rules(kind.designer_tag())
                            .map(|rule| rule.property.to_owned())
                            .collect();
                    (!keys.is_empty()).then(|| (kind.as_str().to_owned(), keys))
                })
                .collect(),
            type_properties: kinds
                .iter()
                .copied()
                .filter_map(|kind| {
                    let id = ObjectId::from_parts(parent.as_ref(), kind.as_str(), "Object");
                    let keys: Vec<_> = crate::project::metadata_edit::type_sets::rules(&id)
                        .map(|rule| rule.property.to_owned())
                        .collect();
                    (!keys.is_empty()).then(|| (kind.as_str().to_owned(), keys))
                })
                .collect(),
            unique_properties: kinds
                .iter()
                .filter_map(|kind| {
                    crate::project::metadata_objects::templates::unique_property(*kind)
                        .map(|key| (kind.as_str().to_owned(), vec![key.to_owned()]))
                })
                .collect(),
            kinds,
            parent,
            module_roles,
            operations: vec!["create", "copy", "delete"],
            prepared_plans: true,
        })
    }

    /// Allocate all new identities once and compute a source-only structural preview.
    ///
    /// # Errors
    /// Rejects unsupported ownership, invalid identities, collisions and protected source paths.
    pub fn preview_object(&mut self, request: &ObjectRequest) -> Result<ObjectPlan, ObjectError> {
        self.object_plan(request, None)
    }

    /// Rebuild an exact saved plan without trusting its serialized display-only file diffs.
    ///
    /// # Errors
    /// Rejects changed inputs, changed identity allocations or changed computed output.
    pub fn preview_prepared_object(
        &mut self,
        prepared: &PreparedObject,
    ) -> Result<ObjectPlan, ObjectError> {
        let plan = self.object_plan(&prepared.request, Some(&prepared.identities))?;
        if plan.prepared.source_snapshot != prepared.source_snapshot
            || plan.prepared.expected_output_snapshot != prepared.expected_output_snapshot
        {
            return Err(EditError::Conflict.into());
        }
        Ok(plan)
    }

    /// Publish the reviewed multi-file operation under the shared lock and recovery journal.
    ///
    /// # Errors
    /// Incoming references require explicit force; conflicts or protected files prevent any write.
    pub fn apply_object(&mut self, prepared: &PreparedObject) -> Result<ObjectResult, ObjectError> {
        let guard = Guard::acquire(self.project().root())?;
        guard.check_clear(self.project().source())?;
        let plan = self.preview_prepared_object(prepared)?;
        if matches!(prepared.request, ObjectRequest::Delete { force: false, .. })
            && !plan.references.is_empty()
        {
            return Err(ObjectError::ReviewRequired(plan.references));
        }
        let paths: Vec<_> = plan
            .inputs
            .iter()
            .map(|path| self.project().source().join(path))
            .collect();
        let _lease =
            SourceLease::acquire_batch(self.project().root(), &paths).map_err(lock_error)?;
        plan.input_checks.verify(self.project().source())?;
        self.generation
            .checked_add(1)
            .ok_or(WorkspaceError::GenerationExhausted)?;
        let histories: Vec<_> = self
            .property_history
            .keys()
            .filter(|id| {
                self.source
                    .object_descriptor(id)
                    .ok()
                    .flatten()
                    .is_some_and(|location| {
                        plan.files.iter().any(|file| file.path == location.path)
                    })
            })
            .cloned()
            .collect();
        guard.publish_files(self.project().source(), &plan.files, &plan.directories)?;
        for id in histories {
            self.property_history.remove(&id);
        }
        let paths: Vec<_> = plan.files.iter().map(|file| file.path.clone()).collect();
        let refresh = self
            .changed_paths(&paths)
            .map_err(|error| ObjectError::Committed(Box::new(error)))?;
        if let Some(id) = &plan.object_id {
            self.reveal_declared_object(id)
                .map_err(|error| ObjectError::Committed(Box::new(error)))?;
        }
        Ok(ObjectResult {
            object_id: plan.object_id,
            snapshot: plan.prepared.expected_output_snapshot,
            refresh,
        })
    }

    fn object_plan(
        &self,
        request: &ObjectRequest,
        identities: Option<&BTreeMap<String, String>>,
    ) -> Result<ObjectPlan, ObjectError> {
        let source = self.source.reopen().map_err(WorkspaceError::Source)?;
        let mut current = Self::open(source)?;
        current.source_protections = self.source_protections.clone();
        if current.project().configuration().project_type()
            == crate::project::ProjectType::Extension
        {
            return Err(ObjectError::Unsupported(MetadataKind::Configuration));
        }
        let mut inputs = Inputs::default();
        let mut plan = match request {
            ObjectRequest::CreateModule { owner, role } => {
                current.create_module(owner, *role, request, &mut inputs)?
            }
            ObjectRequest::Copy {
                source,
                name,
                parent,
                ..
            } => current.copy_object(
                source,
                name,
                parent.as_ref(),
                request,
                identities,
                &mut inputs,
            )?,
            ObjectRequest::Create {
                kind,
                name,
                parent,
                template,
                ..
            } => {
                if let Some(template) = template {
                    if current.reveal_declared_object(template)?.kind != *kind {
                        return Err(crate::project::metadata_objects::invalid(
                            "template",
                            "kind_mismatch",
                        ));
                    }
                    current.copy_object(
                        template,
                        name,
                        parent.as_ref(),
                        request,
                        identities,
                        &mut inputs,
                    )?
                } else {
                    current.create_object(
                        *kind,
                        name,
                        parent.as_ref(),
                        request,
                        identities,
                        &mut inputs,
                    )?
                }
            }
            ObjectRequest::Delete { object, .. } => {
                current.delete_object(object, request, identities, &mut inputs)?
            }
        };
        current.object_properties(&mut plan, &mut inputs)?;
        plan.inputs = inputs
            .files
            .iter()
            .filter(|(_, bytes)| bytes.is_some())
            .map(|(path, _)| path.clone())
            .chain(inputs.observed.keys().cloned())
            .collect();
        plan.inputs.sort();
        plan.inputs.dedup();
        plan.input_checks = InputChecks::capture(&inputs)?;
        plan.files.sort_by(|left, right| left.path.cmp(&right.path));
        plan.directories
            .sort_by(|left, right| left.path.cmp(&right.path));
        plan.prepared.source_snapshot = inputs.snapshot(request)?;
        plan.prepared.expected_output_snapshot = output_snapshot(&plan);
        current.object_support(&plan)?;
        Ok(plan)
    }

    fn object_support(&mut self, plan: &ObjectPlan) -> Result<(), ObjectError> {
        for path in &plan.inputs {
            if self.source_protections.blocks(path, None) {
                return Err(EditError::ReadOnly.into());
            }
        }
        SourceLease::check_many(
            self.project().root(),
            &plan
                .inputs
                .iter()
                .map(|path| self.project().source().join(path))
                .collect::<Vec<_>>(),
        )
        .map_err(lock_error)?;
        let existing: Vec<_> = plan
            .files
            .iter()
            .filter(|file| file.before.is_some())
            .map(|file| file.path.clone())
            .collect();
        for path in &existing {
            if self.source_protections.blocks(path, None) {
                return Err(EditError::ReadOnly.into());
            }
        }
        let mut permission_paths: Vec<_> = existing
            .into_iter()
            .filter(|path| path != std::path::Path::new("ConfigDumpInfo.xml"))
            .collect();
        if matches!(plan.prepared.request, ObjectRequest::CreateModule { .. }) {
            permission_paths.extend(plan.files.iter().map(|file| file.path.clone()));
        }
        let support = self.support_files(&permission_paths)?;
        if !support.diagnostics.is_empty()
            || support
                .files
                .iter()
                .any(|file| file.read_only || file.unknown)
        {
            return Err(EditError::ReadOnly.into());
        }
        for directory in &plan.directories {
            if self.source_protections.blocks_move(&directory.path) {
                return Err(EditError::ReadOnly.into());
            }
        }
        Ok(())
    }
}

fn empty_plan(
    request: &ObjectRequest,
    object_id: Option<ObjectId>,
    identities: BTreeMap<String, String>,
) -> ObjectPlan {
    ObjectPlan {
        prepared: PreparedObject {
            request: request.clone(),
            source_snapshot: String::new(),
            identities,
            expected_output_snapshot: String::new(),
        },
        object_id,
        files: Vec::new(),
        directories: Vec::new(),
        references: Vec::new(),
        inputs: Vec::new(),
        input_checks: InputChecks::default(),
    }
}

fn lock_error(error: std::io::Error) -> EditError {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        EditError::Busy
    } else {
        EditError::Io(error)
    }
}
