//! Property mutations share the checked source resolver, support rules and invalidation boundary.

use crate::project::metadata_edit::EditingProfile;
use std::{io::ErrorKind, path::PathBuf};

use super::{
    ProjectSession, PropertyEditPlan, PropertyReplay, RefreshReport, WorkspaceError,
    editing_history::HistoryStep,
};
use crate::project::{
    designer_source::SourceLocation,
    metadata_edit::{
        EditError, EditPlan, EditingDocument, FieldStep, PropertyChange, PropertyEditing,
        PropertyFileEdit, numbering,
    },
    metadata_model::{MetadataKind, ObjectId},
    metadata_rename::transaction::Guard,
    metadata_write::FileChange,
    support::State,
};

/// Source mapping accompanies a fresh editable snapshot, never client-provided paths.
#[derive(Debug)]
pub struct EditingSnapshot {
    pub properties: PropertyEditing,
    pub path: PathBuf,
    pub writable: bool,
    pub undo: Option<super::HistoryOperation>,
    pub redo: Option<super::HistoryOperation>,
    pub context_snapshot: Option<String>,
    pub linked_objects: usize,
}

/// A type choice carries semantic identity and optional metadata context for presentation.
#[derive(Debug)]
pub struct PropertyTypeChoice {
    pub key: crate::project::metadata_model::PropertyKey,
    pub object: Option<super::ObjectSummary>,
}

/// Source failures and editing failures retain their own stable machine categories.
#[derive(Debug)]
pub enum PropertyEditError {
    Workspace(Box<WorkspaceError>),
    /// The bytes were published, but refreshing the session failed; do not repeat the write.
    Committed(Box<WorkspaceError>),
    Edit(EditError),
    /// Preserve the declared owner of a failed dependent field for both human and AI clients.
    Related {
        object_id: ObjectId,
        name: String,
        error: EditError,
    },
}

impl From<WorkspaceError> for PropertyEditError {
    /// Preserve original source and generation diagnostics at the editing boundary.
    fn from(value: WorkspaceError) -> Self {
        Self::Workspace(Box::new(value))
    }
}

impl From<EditError> for PropertyEditError {
    /// Keep mutation diagnostics independent of localization and transport.
    fn from(value: EditError) -> Self {
        Self::Edit(value)
    }
}

impl ProjectSession {
    /// Read allowed existing data types without indexing or reading unrelated descriptors.
    ///
    /// # Errors
    /// Rejects stale object addresses and fields which do not support type selection.
    pub fn property_type_choices(
        &mut self,
        id: &ObjectId,
        path: &[FieldStep],
    ) -> Result<Vec<PropertyTypeChoice>, PropertyEditError> {
        use crate::project::metadata_edit::{ScalarSchema, ValueDomain, types};
        use crate::project::metadata_model::PropertyKey;
        let snapshot = self.property_fields(id)?;
        let field = snapshot
            .properties
            .fields
            .iter()
            .find(|field| field.path == path)
            .ok_or(EditError::UnsupportedValue)?;
        if let ScalarSchema::Types { domain, .. } = field.schema {
            return self.type_set_choices(domain);
        }
        let (reference_only, parameter) = match field.schema {
            ScalarSchema::DataType { reference_only, .. } => (reference_only, false),
            ScalarSchema::Value {
                domain: Some(ValueDomain::ChoiceParameter),
                ..
            } => (false, true),
            _ => return Err(EditError::UnsupportedValue.into()),
        };
        let mut choices: Vec<_> = types::PRIMITIVES
            .iter()
            .filter(|(namespace, _)| {
                !reference_only && (!parameter || *namespace == "http://www.w3.org/2001/XMLSchema")
            })
            .map(|(namespace, name)| PropertyTypeChoice {
                key: PropertyKey {
                    namespace: Some((*namespace).to_owned()),
                    name: (*name).to_owned(),
                },
                object: None,
            })
            .collect();
        let sibling_types: Vec<_> = snapshot
            .properties
            .fields
            .iter()
            .filter_map(|other| {
                if parameter
                    || other.path == path
                    || other.path.len() != path.len()
                    || other.path[..other.path.len() - 1] != path[..path.len() - 1]
                {
                    return None;
                }
                if let ScalarSchema::DataType { key, .. } = &other.schema {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        let root = self.source.root().id();
        for (prefix, tag) in types::REFERENCE_TYPES {
            let kind = MetadataKind::from_xml_tag(tag).map_err(|_| EditError::UnsupportedValue)?;
            for object in self.objects.values().filter(|object| {
                object.kind == kind && object.parent.is_none() && &object.id != root
            }) {
                choices.push(PropertyTypeChoice {
                    key: PropertyKey {
                        namespace: Some(types::CFG.to_owned()),
                        name: format!("{prefix}.{}", object.name),
                    },
                    object: Some(object.clone()),
                });
            }
        }
        choices.retain(|choice| !sibling_types.contains(&&choice.key));
        Ok(choices)
    }

    pub(super) fn type_set_choices(
        &mut self,
        domain: crate::project::metadata_edit::TypeDomain,
    ) -> Result<Vec<PropertyTypeChoice>, PropertyEditError> {
        use crate::project::metadata_edit::{TypeDomain, type_sets, types};
        use crate::project::metadata_model::PropertyKey;
        if domain == TypeDomain::EventSource {
            let owners: Vec<_> = self
                .objects
                .values()
                .filter(|object| {
                    object.kind == MetadataKind::CalculationRegister && object.parent.is_none()
                })
                .map(|object| object.id.clone())
                .collect();
            for owner in owners {
                self.children(
                    &super::NodeId::Object(owner),
                    crate::project::configurator::TreeOptions::default(),
                )?;
            }
        }
        let mut choices: Vec<_> = types::PRIMITIVES
            .iter()
            .filter(|_| domain == TypeDomain::Value)
            .map(|(namespace, name)| PropertyTypeChoice {
                key: PropertyKey {
                    namespace: Some((*namespace).to_owned()),
                    name: (*name).to_owned(),
                },
                object: None,
            })
            .collect();
        for (family, kind) in type_sets::families(domain) {
            if domain == TypeDomain::EventSource && kind != MetadataKind::DefinedType {
                choices.push(PropertyTypeChoice {
                    key: PropertyKey {
                        namespace: Some(types::CFG.to_owned()),
                        name: family.clone(),
                    },
                    object: None,
                });
            }
            choices.extend(
                self.objects
                    .values()
                    .filter(|object| {
                        object.kind == kind
                            && (object.parent.is_none() || kind == MetadataKind::Recalculation)
                            && &object.id != self.source.root().id()
                    })
                    .map(|object| PropertyTypeChoice {
                        key: PropertyKey {
                            namespace: Some(types::CFG.to_owned()),
                            name: object
                                .parent
                                .as_ref()
                                .and_then(|parent| self.objects.get(parent))
                                .map_or_else(
                                    || format!("{family}.{}", object.name),
                                    |parent| format!("{family}.{}.{}", parent.name, object.name),
                                ),
                        },
                        object: Some(object.clone()),
                    }),
            );
        }
        Ok(choices)
    }

    /// Validate and preview using precisely the same planner as mutation, with no writes.
    ///
    /// # Errors
    /// Returns the same validation, source and policy failures as applying the edit.
    pub fn preview_property(
        &mut self,
        id: &ObjectId,
        expected: &str,
        context: Option<&str>,
        path: &[FieldStep],
        change: &PropertyChange,
    ) -> Result<PropertyEditPlan, PropertyEditError> {
        self.plan_property(id, expected, context, path, change)
    }

    /// Preview and publication always reconstruct the same complete dependent-file plan.
    fn plan_property(
        &mut self,
        id: &ObjectId,
        expected: &str,
        context: Option<&str>,
        path: &[FieldStep],
        change: &PropertyChange,
    ) -> Result<PropertyEditPlan, PropertyEditError> {
        self.reveal_declared_object(id)?;
        let (location, input) = self.editing_source(id)?;
        if !self.can_edit(id, &location.path)? {
            return Err(EditError::ReadOnly.into());
        }
        if let PropertyChange::DataType { key } = change {
            self.validate_property_type(key)?;
        }
        self.validate_property_reference(id, path, change)?;
        self.validate_property_value(id, path, change)?;
        let document = self.editing_document(id, &location, &input)?;
        if let PropertyChange::Types { keys } = change {
            let fields = document.inspect()?;
            let field = fields
                .fields
                .iter()
                .find(|field| field.path == path)
                .ok_or(EditError::UnsupportedValue)?;
            let crate::project::metadata_edit::ScalarSchema::Types { domain, .. } = field.schema
            else {
                return Err(EditError::UnsupportedValue.into());
            };
            crate::project::metadata_edit::type_sets::validate(domain, keys)?;
            for key in keys {
                if let Some(parts) = crate::project::metadata_edit::type_sets::target(domain, key) {
                    self.property_reference(&parts)?;
                }
            }
        }
        let plan = document.update(expected, path, change)?;
        let profile = document.profile;
        self.validate_predefined_constraints(id, &plan)?;
        let kind = self.object(id)?.kind;
        let plan = if numbering::linked_field(kind, path) {
            self.plan_linked_property(
                PropertyFileEdit {
                    object_id: id.clone(),
                    path: location.path,
                    plan,
                },
                kind,
                context,
                &path[0].key.name,
                profile,
            )?
        } else {
            Self::scalar_property_plan(id, location.path, plan)
        };
        self.validate_linked_support(&plan)?;
        Ok(plan)
    }

    /// Read editing domains and the exact source fingerprint without changing XML.
    ///
    /// # Errors
    /// Returns unknown object, invalid source or unsupported descriptor shape.
    pub fn property_editing(
        &mut self,
        id: &ObjectId,
    ) -> Result<EditingSnapshot, PropertyEditError> {
        let mut state = self.property_fields(id)?;
        let kind = self.object(id)?.kind;
        match self.property_context(id, kind) {
            Ok(Some((snapshot, count))) => {
                state.context_snapshot = Some(snapshot);
                state.linked_objects = count;
            }
            Ok(None) => (),
            Err(_) => {
                // Keep history in memory but do not advertise a replay without a current dependency token.
                if state.undo == Some(super::HistoryOperation::Linked) {
                    state.undo = None;
                }
                if state.redo == Some(super::HistoryOperation::Linked) {
                    state.redo = None;
                }
                let mut hidden = Vec::new();
                state.properties.fields.retain(|field| {
                    let keep = !numbering::linked_field(kind, &field.path);
                    if !keep {
                        hidden.push(field.path[0].key.clone());
                    }
                    keep
                });
                state
                    .properties
                    .read_only_properties
                    .extend(hidden.into_iter().map(|key| {
                        crate::project::metadata_edit::ReadOnlyProperty {
                            key,
                            reason: "linked_context_unavailable",
                        }
                    }));
            }
        }
        Ok(state)
    }

    /// Internal domain checks need only the owner's fields, without repeatedly scanning its dependencies.
    pub(super) fn property_fields(
        &mut self,
        id: &ObjectId,
    ) -> Result<EditingSnapshot, PropertyEditError> {
        self.reveal_declared_object(id)?;
        let (location, input) = self.editing_source(id)?;
        let properties = self.editing_document(id, &location, &input)?.inspect()?;
        let writable = self.can_edit(id, &location.path)?
            && std::fs::metadata(self.project().source().join(&location.path))
                .is_ok_and(|metadata| !metadata.permissions().readonly());
        let history = self
            .edit_history
            .get(&super::editing_history::EditedSource::Descriptor(
                id.clone(),
            ));
        Ok(EditingSnapshot {
            properties,
            path: location.path,
            writable,
            context_snapshot: None,
            linked_objects: 0,
            undo: history
                .and_then(|history| history.undo.last())
                .map(HistoryStep::operation),
            redo: history
                .and_then(|history| history.redo.last())
                .map(HistoryStep::operation),
        })
    }

    /// Validate and publish an existing property and its declared dependencies, then invalidate changed descriptors.
    ///
    /// # Errors
    /// Rejects stale snapshots, locked objects, unadvertised fields and invalid values.
    pub fn update_property(
        &mut self,
        id: &ObjectId,
        expected: &str,
        context: Option<&str>,
        path: &[FieldStep],
        change: &PropertyChange,
    ) -> Result<RefreshReport, PropertyEditError> {
        let guard = Guard::acquire(self.project().root())?;
        guard.check_clear(self.project().source())?;
        let plan = self.plan_property(id, expected, context, path, change)?;
        if plan.context_before.is_some() {
            let kind = self.object(id)?.kind;
            return self.save_linked_property(&guard, &plan, kind);
        }
        let (location, _) = self.editing_source(id)?;
        let history = plan.primary.plan.history();
        let changed = !plan.is_empty();
        let report = self.publish_property(&guard, id, &location, &plan.primary.plan);
        if changed && (report.is_ok() || matches!(report, Err(PropertyEditError::Committed(_)))) {
            self.record_property(id, HistoryStep::Scalar(history));
        }
        report
    }

    /// Replay the previous backend-generated replacement without accepting arbitrary XML.
    ///
    /// # Errors
    /// Rejects stale source, changed support policy and unavailable session history.
    pub fn undo_property(
        &mut self,
        id: &ObjectId,
        expected: &str,
        context: Option<&str>,
        undo: bool,
    ) -> Result<PropertyReplay, PropertyEditError> {
        let guard = Guard::acquire(self.project().root())?;
        guard.check_clear(self.project().source())?;
        self.reveal_declared_object(id)?;
        let (location, input) = self.editing_source(id)?;
        if crate::project::metadata_edit::snapshot(&input) != expected {
            return Err(EditError::Conflict.into());
        }
        let history = self
            .edit_history
            .get(&super::editing_history::EditedSource::Descriptor(
                id.clone(),
            ))
            .ok_or(EditError::HistoryUnavailable)?;
        let step = if undo {
            history.undo.last()
        } else {
            history.redo.last()
        }
        .ok_or(EditError::HistoryUnavailable)?
        .clone();
        let scalar = match &step {
            HistoryStep::Scalar(scalar) => scalar,
            HistoryStep::Rename(_) => return self.replay_rename(&guard, id, step, undo),
            HistoryStep::Linked(_) => {
                return self.replay_linked_property(&guard, id, context, step, undo);
            }
        };
        let plan = scalar.replay(&input, undo)?;
        let report = self.publish_property(&guard, id, &location, &plan);
        if report.is_ok() || matches!(report, Err(PropertyEditError::Committed(_))) {
            self.advance_history(id, step, undo);
        }
        report.map(|refresh| PropertyReplay {
            object_id: id.clone(),
            refresh,
        })
    }

    /// Resolve the existing descriptor again at every write boundary.
    pub(super) fn editing_source(
        &self,
        id: &ObjectId,
    ) -> Result<(SourceLocation, String), PropertyEditError> {
        let location = self
            .source
            .object_descriptor(id)
            .map_err(WorkspaceError::Source)?
            .ok_or(EditError::UnsupportedValue)?;
        let input = self
            .source
            .read_xml(&location.path)
            .map_err(WorkspaceError::Source)?
            .ok_or(EditError::UnsupportedValue)?;
        Ok((location, input))
    }

    /// Determine the owning descriptor identity through the existing inline mapping.
    pub(super) fn editing_document<'a>(
        &self,
        id: &'a ObjectId,
        location: &SourceLocation,
        input: &'a str,
    ) -> Result<EditingDocument<'a>, EditError> {
        let mut owner = id.clone();
        for _ in &location.inline {
            owner = owner.parent().ok_or(EditError::UnsupportedValue)?;
        }
        let predefined = location
            .inline
            .first()
            .is_some_and(|item| item.kind == MetadataKind::PredefinedItem);
        let profile = EditingProfile::for_platform(
            self.project()
                .configuration()
                .build_settings()
                .platform_version(),
        );
        Ok(EditingDocument {
            input,
            id,
            parent: if predefined {
                Some(owner)
            } else {
                owner.parent()
            },
            predefined,
            profile,
        })
    }

    /// Object-level support permits a narrow edit even when a sibling in the same XML is locked.
    pub(super) fn can_edit(
        &mut self,
        id: &ObjectId,
        path: &std::path::Path,
    ) -> Result<bool, WorkspaceError> {
        if self.source_protections.blocks(path, None) {
            return Ok(false);
        }
        let policy = self.support_files(&[path.to_path_buf()])?;
        Ok(policy.diagnostics.is_empty()
            && policy.objects.iter().any(|object| {
                &object.object_id == id
                    && matches!(
                        object.state,
                        State::Unrestricted | State::EditableWithSupport
                    )
            }))
    }

    /// Generated reference types must name a declared object in this project.
    fn validate_property_type(
        &mut self,
        key: &crate::project::metadata_model::PropertyKey,
    ) -> Result<(), PropertyEditError> {
        use crate::project::metadata_edit::types;
        if key.namespace.as_deref() != Some(types::CFG) {
            return Ok(());
        }
        let (prefix, name) = key.name.split_once('.').ok_or(EditError::InvalidValue)?;
        let tag = types::REFERENCE_TYPES
            .iter()
            .find_map(|(family, tag)| (*family == prefix).then_some(*tag))
            .ok_or(EditError::InvalidValue)?;
        let kind = MetadataKind::from_xml_tag(tag).map_err(|_| EditError::InvalidValue)?;
        self.property_reference(&[(kind, name)])?;
        Ok(())
    }

    /// Publication precedes cache invalidation, and no-op edits leave the generation untouched.
    fn publish_property(
        &mut self,
        guard: &Guard,
        id: &ObjectId,
        location: &SourceLocation,
        plan: &EditPlan,
    ) -> Result<RefreshReport, PropertyEditError> {
        if !self.can_edit(id, &location.path)? {
            return Err(EditError::ReadOnly.into());
        }
        self.validate_predefined_constraints(id, plan)?;
        if self.generation == u64::MAX {
            return Err(WorkspaceError::GenerationExhausted.into());
        }
        let mut ancestry = Vec::new();
        let mut cursor = Some(id.clone());
        while let Some(ref current) = cursor {
            let object = self.object(current)?;
            ancestry.push((object.kind, object.name.clone()));
            cursor = object.parent.clone();
        }
        ancestry.reverse();
        let source = self.project().source();
        if plan.is_empty() {
            // Preserve the exact no-op validation boundary without creating a journal.
            plan.publish(source, &location.path)?;
            return Ok(RefreshReport {
                generation: self.generation,
                affected: Vec::new(),
            });
        }
        let change = FileChange {
            path: location.path.clone(),
            before: Some(plan.original().as_bytes().to_vec()),
            after: Some(plan.output().as_bytes().to_vec()),
        };
        match guard.publish_files(source, &[change], &[]) {
            Err(EditError::Io(error)) if error.kind() == ErrorKind::CrossesDevices => {
                // A mounted source may live on a different filesystem from the journal.
                // Only a cleared transaction permits retrying beside the source file.
                guard.check_clear(source)?;
                plan.publish(source, &location.path)?;
            }
            result => result?,
        }
        let report = self
            .changed_paths(std::slice::from_ref(&location.path))
            .map_err(|error| PropertyEditError::Committed(Box::new(error)))?;
        let parts: Vec<_> = ancestry
            .iter()
            .map(|(kind, name)| (*kind, name.as_str()))
            .collect();
        self.property_reference(&parts)
            .map_err(|error| PropertyEditError::Committed(Box::new(error)))?;
        Ok(report)
    }

    /// Read the current payload rather than a watcher cache before both ordinary writes and undo.
    fn validate_predefined_constraints(
        &self,
        id: &ObjectId,
        plan: &EditPlan,
    ) -> Result<(), PropertyEditError> {
        use crate::project::metadata_edit::ext_dimensions::PredefinedDimensions;
        use crate::project::metadata_edit::lengths::PredefinedLengths;
        if plan.is_empty() {
            return Ok(());
        }
        let lengths = PredefinedLengths::changed(plan.original(), plan.output())?;
        let dimensions = PredefinedDimensions::changed(plan.original(), plan.output())?;
        if lengths.is_none() && dimensions.is_none() {
            return Ok(());
        }
        if let Some(path) = self
            .source
            .predefined_path(id)
            .map_err(WorkspaceError::Source)?
            && let Some(input) = self
                .source
                .read_xml(&path)
                .map_err(WorkspaceError::Source)?
        {
            if let Some(lengths) = lengths {
                lengths.validate(&input)?;
            }
            if let Some(dimensions) = dimensions {
                dimensions.validate(&input)?;
            }
        }
        Ok(())
    }
}
