//! Empty objects and deep copies publish through the same ownership and transaction path.

use super::{ProjectSession, empty_plan};
use crate::project::{
    metadata_edit::EditError,
    metadata_model::{MetadataKind, ObjectId},
    metadata_objects::{
        ObjectError, ObjectPlan, ObjectRequest,
        inventory::{Inputs, target_directories},
        templates, xml,
    },
    metadata_write::FileChange,
};
use std::collections::BTreeMap;

impl ProjectSession {
    pub(super) fn create_object(
        &mut self,
        kind: MetadataKind,
        name: &str,
        parent: Option<&ObjectId>,
        request: &ObjectRequest,
        saved: Option<&BTreeMap<String, String>>,
        inputs: &mut Inputs,
    ) -> Result<ObjectPlan, ObjectError> {
        let (id, owner) = self.target_identity(kind, name, parent, inputs)?;
        let identities = templates::allocate(kind, saved)?;
        let descriptor =
            templates::descriptor(kind, name, &identities, owner.kind, &owner.ancestry)?;
        let mut plan = empty_plan(request, Some(id.clone()), identities);
        let root = self.project().source().to_path_buf();
        let after = if let Some(path) = self.new_descriptor(&id, &owner)? {
            super::copying::ensure_absent(&root, inputs, &path, &id)?;
            super::copying::ensure_absent_tree(&root, inputs, &path.with_extension(""), &id)?;
            plan.files.push(FileChange {
                path: path.clone(),
                before: None,
                after: Some(descriptor.into_bytes()),
            });
            if matches!(kind, MetadataKind::Form | MetadataKind::CommonForm) {
                let payload = path.with_extension("").join("Ext/Form.xml");
                super::copying::ensure_absent(&root, inputs, &payload, &id)?;
                plan.files.push(FileChange {
                    path: payload,
                    before: None,
                    after: Some(crate::project::metadata_form_edit::empty_form().into_bytes()),
                });
            }
            xml::add_reference(&owner.input, owner.range, kind.designer_tag(), name)?
        } else {
            let parsed = xml::document(&descriptor)?;
            let node = parsed
                .root_element()
                .children()
                .find(roxmltree::Node::is_element)
                .ok_or(EditError::InvalidXml)?;
            // Self-contained namespace declarations permit insertion into prefixed/legacy descriptors.
            let fragment = &descriptor[node.range()];
            let end = fragment.find('>').ok_or(EditError::InvalidXml)?;
            let fragment = format!(
                "{} xmlns=\"{}\" xmlns:xr=\"{}\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"{}",
                &fragment[..end],
                xml::MD,
                xml::XR,
                &fragment[end..]
            );
            xml::insert_child(&owner.input, owner.range, "ChildObjects", &fragment)?
        };
        plan.files.push(FileChange {
            path: owner.path,
            before: Some(owner.input.into_bytes()),
            after: Some(after.into_bytes()),
        });
        plan.directories = target_directories(&root, inputs, &plan.files, Vec::new())?;
        Ok(plan)
    }
}
