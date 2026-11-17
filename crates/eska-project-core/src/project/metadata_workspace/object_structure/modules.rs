//! Module roles are schema-owned; files are created through the shared exact-plan journal.

use std::{collections::BTreeMap, path::PathBuf};

use super::{ProjectSession, WorkspaceError, empty_plan};
use crate::project::{
    configurator::ConfiguratorSchema,
    metadata_edit::EditError,
    metadata_model::{MetadataKind, ModuleRole, ObjectId},
    metadata_objects::{
        ObjectError, ObjectPlan, ObjectRequest, invalid,
        inventory::{Inputs, target_directories},
    },
    metadata_write::FileChange,
};

impl ProjectSession {
    pub(super) fn missing_module_roles(
        &mut self,
        owner: &ObjectId,
    ) -> Result<Vec<ModuleRole>, ObjectError> {
        let kind = self.reveal_declared_object(owner)?.kind;
        let mut inputs = Inputs::default();
        let mut roles = Vec::new();
        for &role in ConfiguratorSchema::for_source(&self.source).modules(owner, kind) {
            if self
                .missing_module_path(owner, kind, role, &mut inputs)?
                .is_some()
            {
                roles.push(role);
            }
        }
        Ok(roles)
    }

    pub(super) fn create_module(
        &mut self,
        owner: &ObjectId,
        role: ModuleRole,
        request: &ObjectRequest,
        inputs: &mut Inputs,
    ) -> Result<ObjectPlan, ObjectError> {
        let context = self.object_context(owner, inputs)?;
        if !ConfiguratorSchema::for_source(&self.source)
            .modules(owner, context.kind)
            .contains(&role)
        {
            return Err(invalid("role", "unsupported_module"));
        }
        let path = self
            .missing_module_path(owner, context.kind, role, inputs)?
            .ok_or_else(|| invalid("role", "module_unavailable"))?;
        let mut plan = empty_plan(request, Some(owner.clone()), BTreeMap::new());
        plan.files.push(FileChange {
            path,
            before: None,
            after: Some(Vec::new()),
        });
        plan.directories =
            target_directories(self.project().source(), inputs, &plan.files, Vec::new())?;
        Ok(plan)
    }

    /// Fingerprint every alternative, including binary modules: none may be shadowed or overwritten.
    fn missing_module_path(
        &self,
        owner: &ObjectId,
        kind: MetadataKind,
        role: ModuleRole,
        inputs: &mut Inputs,
    ) -> Result<Option<PathBuf>, ObjectError> {
        let paths = self
            .source
            .module_candidates(owner, role)
            .map_err(WorkspaceError::Source)?;
        let root = self.project().source();
        for path in &paths {
            for candidate in [path.clone(), path.with_extension("bin")] {
                if self
                    .source
                    .checked_file(&candidate)
                    .map_err(WorkspaceError::Source)?
                    .is_some()
                {
                    return Ok(None);
                }
                if inputs.read(root, &candidate)?.is_some() {
                    return Err(EditError::Conflict.into());
                }
            }
        }
        let path = match paths.as_slice() {
            [path] => path.clone(),
            [direct, wrapped] => {
                let descriptor = self
                    .source
                    .object_descriptor(owner)
                    .map_err(WorkspaceError::Source)?
                    .ok_or(EditError::Conflict)?;
                if inputs.directory(root, &descriptor.path.with_extension(""))? {
                    wrapped.clone()
                } else {
                    direct.clone()
                }
            }
            _ => return Err(invalid("owner", "ambiguous_layout")),
        };
        if matches!(kind, MetadataKind::Form | MetadataKind::CommonForm) {
            // Ordinary forms have a binary payload; only a managed XML form has an editable BSL module.
            let payload = path
                .parent()
                .ok_or(EditError::UnsafePath)?
                .with_extension("xml");
            if inputs.read(root, &payload)?.is_none() {
                return Ok(None);
            }
        }
        Ok(Some(path))
    }
}
