//! Child creation resolves fresh declarations, permissions and references through the shared session.

use super::{ProjectSession, WorkspaceError};
use crate::project::{
    ProjectType,
    metadata_create::{
        self, CreateRequest, CreateType, CreationError, CreationIdentity, CreationLanguage,
        CreationPlan, CreationReference, CreationResult, CreationSchema, PreparedCreate,
    },
    metadata_edit::{EditError, snapshot, types},
    metadata_model::{MetadataKind, MetadataValue, ObjectId, PropertyKey},
    metadata_parser::{self, PropertiesMode},
    metadata_rename::transaction::Guard,
    source_lock::SourceLease,
};

impl ProjectSession {
    /// Inspect creation choices without allocating IDs, writing caches or trusting stale watcher state.
    ///
    /// # Errors
    /// Rejects unavailable owners, unsupported project contexts and invalid source descriptors.
    pub fn creation_schema(&mut self, owner: &ObjectId) -> Result<CreationSchema, CreationError> {
        self.creation_current()?.creation_schema_current(owner)
    }

    /// Allocate native identities and compute a complete source plan from semantic input.
    ///
    /// # Errors
    /// Rejects stale snapshots, duplicate or reserved names, invalid types and restricted owners.
    pub fn preview_create(
        &mut self,
        request: &CreateRequest,
    ) -> Result<CreationPlan, CreationError> {
        let prepared = PreparedCreate {
            request: request.clone(),
            identity: CreationIdentity::generate(request.kind)?,
            expected_output_snapshot: String::new(),
        };
        let mut plan = self.plan_creation(&prepared)?;
        plan.prepared.expected_output_snapshot = snapshot(plan.output());
        Ok(plan)
    }

    /// Reconstruct a saved preview without replacing its native identities or accepting patch bytes.
    ///
    /// # Errors
    /// Rejects changed source or any prepared input inconsistent with its reviewed output hash.
    pub fn preview_prepared_create(
        &mut self,
        prepared: &PreparedCreate,
    ) -> Result<CreationPlan, CreationError> {
        let plan = self.plan_creation(prepared)?;
        if snapshot(plan.output()) != prepared.expected_output_snapshot {
            return Err(EditError::Conflict.into());
        }
        Ok(plan)
    }

    /// Publish the exact reviewed plan under the shared metadata write guard.
    ///
    /// # Errors
    /// Returns validation or publication failures without replacing unrelated bytes. `Committed`
    /// means creation succeeded but presentation refresh failed: inspect before any further action.
    pub fn apply_create(
        &mut self,
        prepared: &PreparedCreate,
    ) -> Result<CreationResult, CreationError> {
        let guard = Guard::acquire(self.project().root())?;
        guard.check_clear(self.project().source())?;
        let plan = self.preview_prepared_create(prepared)?;
        let _lease = SourceLease::acquire(
            self.project().root(),
            &[self.project().source().join(&plan.path)],
        )
        .map_err(creation_lock_error)?;
        self.generation
            .checked_add(1)
            .ok_or(WorkspaceError::GenerationExhausted)?;
        let after = snapshot(plan.output());
        plan.edit.publish(self.project().source(), &plan.path)?;
        // Replaying an older scalar snapshot after a structural insertion would erase the new child.
        // Creation itself has no scalar undo: deleting an object needs separate dependency checks.
        self.forget_descriptor_history(&plan.path);
        let refresh = self
            .changed_paths(std::slice::from_ref(&plan.path))
            .map_err(|error| CreationError::Committed(Box::new(error)))?;
        self.reveal_declared_object(&plan.object_id)
            .map_err(|error| CreationError::Committed(Box::new(error)))?;
        Ok(CreationResult {
            object_id: plan.object_id,
            uuid: plan.prepared.identity.uuid,
            snapshot: after,
            refresh,
        })
    }

    /// One fresh root and the explicitly requested descriptors avoid both stale sessions and full scans.
    fn creation_current(&self) -> Result<Self, CreationError> {
        let source = self.source.reopen().map_err(WorkspaceError::Source)?;
        let mut current = Self::open(source)?;
        current.source_protections = self.source_protections.clone();
        Ok(current)
    }

    fn creation_schema_current(
        &mut self,
        owner: &ObjectId,
    ) -> Result<CreationSchema, CreationError> {
        let object = self.reveal_declared_object(owner)?;
        let kinds = metadata_create::kinds_for_owner(object.kind);
        if kinds.is_empty()
            || self.project().configuration().project_type() == ProjectType::Extension
        {
            return Err(CreationError::UnsupportedOwner(object.kind));
        }
        if object.kind == MetadataKind::TabularSection {
            let parent = object.parent.as_ref().ok_or(EditError::UnsupportedValue)?;
            let parent = self.reveal_declared_object(parent)?;
            if !metadata_create::kinds_for_owner(parent.kind)
                .contains(&metadata_create::CreationKind::TablePart)
            {
                return Err(CreationError::UnsupportedOwner(object.kind));
            }
        }
        let (location, input) = self.editing_source(owner)?;
        let source_path = self.project().source().join(&location.path);
        let lease_available = match SourceLease::check(self.project().root(), &source_path) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(error) => return Err(EditError::Io(error).into()),
        };
        let writable = lease_available
            && self.can_edit(owner, &location.path)?
            && !std::fs::metadata(self.project().source().join(&location.path))
                .map_err(EditError::Io)?
                .permissions()
                .readonly();
        let references = self.creation_references();
        let languages = self.creation_languages()?;
        Ok(CreationSchema {
            owner: owner.clone(),
            snapshot: snapshot(&input),
            writable,
            capabilities: metadata_create::capabilities(),
            kinds: kinds.to_vec(),
            references,
            languages,
        })
    }

    fn creation_references(&self) -> Vec<CreationReference> {
        self.objects
            .values()
            .filter(|object| object.parent.is_none())
            .filter_map(|object| {
                let (family, _) = types::REFERENCE_TYPES
                    .iter()
                    .find(|(_, tag)| *tag == object.kind.designer_tag())?;
                Some(CreationReference {
                    object: object.id.clone(),
                    kind: object.kind,
                    name: object.name.clone(),
                    key: PropertyKey {
                        namespace: Some(types::CFG.to_owned()),
                        name: format!("{family}.{}", object.name),
                    },
                })
            })
            .collect()
    }

    fn creation_languages(&self) -> Result<Vec<CreationLanguage>, CreationError> {
        self.objects
            .values()
            .filter(|object| object.kind == MetadataKind::Language && object.parent.is_none())
            .map(|object| {
                let parsed = metadata_parser::load(&self.source, &object.id, PropertiesMode::All)
                    .map_err(WorkspaceError::Load)?;
                let code = parsed
                    .objects
                    .iter()
                    .find(|item| item.metadata.id() == &object.id)
                    .and_then(|item| item.properties.as_ref())
                    .and_then(|properties| {
                        properties.iter().find(|property| {
                            property.property.key.namespace.as_deref() == Some(metadata_create::MD)
                                && property.property.key.name == "LanguageCode"
                        })
                    })
                    .and_then(|property| match &property.property.value {
                        MetadataValue::Text(value) => Some(value),
                        _ => None,
                    })
                    .filter(|value| !value.is_empty())
                    .ok_or(EditError::UnsupportedValue)?;
                Ok(CreationLanguage {
                    object: object.id.clone(),
                    code: code.to_owned(),
                    name: object.name.clone(),
                })
            })
            .collect()
    }

    fn plan_creation(&self, prepared: &PreparedCreate) -> Result<CreationPlan, CreationError> {
        let mut current = self.creation_current()?;
        let schema = current.creation_schema_current(&prepared.request.owner)?;
        if !schema.writable {
            return Err(EditError::ReadOnly.into());
        }
        if schema.snapshot != prepared.request.snapshot {
            return Err(EditError::Conflict.into());
        }
        current.validate_creation_choices(prepared, &schema)?;
        let (location, input) = current.editing_source(&prepared.request.owner)?;
        let context = current.editing_document(&prepared.request.owner, &location, &input)?;
        let (object_id, edit) =
            metadata_create::plan(&input, context.parent, prepared, &schema.references)?;
        Ok(CreationPlan {
            prepared: prepared.clone(),
            object_id,
            path: location.path,
            edit,
        })
    }

    fn validate_creation_choices(
        &self,
        prepared: &PreparedCreate,
        schema: &CreationSchema,
    ) -> Result<(), CreationError> {
        for value in &prepared.request.types {
            if let CreateType::Reference { object } = value {
                if !schema
                    .references
                    .iter()
                    .any(|reference| &reference.object == object)
                {
                    return Err(CreationError::Invalid {
                        field: "types",
                        reason: "unknown_reference",
                    });
                }
                metadata_parser::load(&self.source, object, PropertiesMode::Summary)
                    .map_err(WorkspaceError::Load)?;
            }
        }
        if !schema.languages.is_empty()
            && prepared.request.synonyms.iter().any(|synonym| {
                !schema
                    .languages
                    .iter()
                    .any(|language| language.code == synonym.language)
            })
        {
            return Err(CreationError::Invalid {
                field: "synonyms",
                reason: "unknown_language",
            });
        }
        Ok(())
    }

    fn forget_descriptor_history(&mut self, path: &std::path::Path) {
        let ids: Vec<_> = self
            .edit_history
            .keys()
            .filter(|id| {
                self.source
                    .object_descriptor(id.object())
                    .ok()
                    .flatten()
                    .is_some_and(|location| location.path == path)
            })
            .cloned()
            .collect();
        for id in ids {
            self.edit_history.remove(&id);
        }
    }
}

fn creation_lock_error(error: std::io::Error) -> CreationError {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        EditError::Busy.into()
    } else {
        EditError::Io(error).into()
    }
}
