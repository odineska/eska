//! Source locations come from the Designer resolver; XML coordinates are verified in current bytes.

use std::{collections::BTreeSet, ops::Range, path::PathBuf};

use super::{ProjectSession, WorkspaceError};
use crate::project::{
    ProjectType,
    configurator::ConfiguratorSchema,
    metadata_edit::EditError,
    metadata_model::{MetadataKind, ObjectId},
    metadata_objects::{
        ObjectError, invalid,
        inventory::{Inputs, text},
        xml,
    },
    metadata_parser::{self, PropertiesMode},
    metadata_rename::{same_name, validate_name},
};

pub(super) struct Context {
    pub kind: MetadataKind,
    pub parent: Option<ObjectId>,
    pub path: PathBuf,
    pub input: String,
    pub range: Range<usize>,
    pub name_range: Range<usize>,
    pub inline: bool,
    pub ancestry: Vec<(String, String)>,
    pub descriptor_parent: Option<ObjectId>,
}

pub(super) struct ObjectSources {
    pub files: Vec<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub directories: BTreeSet<PathBuf>,
}

impl ProjectSession {
    /// Inline commands can own modules; other standalone declarations own their complete stem tree.
    pub(super) fn object_sources(
        &self,
        id: &ObjectId,
        context: &Context,
        inputs: &mut Inputs,
    ) -> Result<ObjectSources, ObjectError> {
        let root = self.project().source();
        let mut files = Vec::new();
        let mut roots = BTreeSet::new();
        if context.inline {
            for location in self.source.sources(id).map_err(WorkspaceError::Source)? {
                if location.path != context.path {
                    roots.insert(
                        location
                            .path
                            .parent()
                            .and_then(std::path::Path::parent)
                            .ok_or(EditError::UnsafePath)?
                            .to_path_buf(),
                    );
                }
            }
        } else {
            files.push(context.path.clone());
            roots.insert(context.path.with_extension(""));
        }
        for directory in &roots {
            files.extend(inputs.subtree(root, directory)?);
        }
        files.sort();
        files.dedup();
        let directories = inputs
            .directories
            .iter()
            .filter(|(path, exists)| **exists && roots.iter().any(|root| path.starts_with(root)))
            .map(|(path, _)| path.clone())
            .collect();
        Ok(ObjectSources {
            files,
            roots: roots.into_iter().collect(),
            directories,
        })
    }
    /// Resolver-backed module ownership plus a complete field inventory proves the implicit scope.
    pub(super) fn owned_module_context(
        &self,
        id: &ObjectId,
        inputs: &mut Inputs,
    ) -> Result<BTreeSet<PathBuf>, ObjectError> {
        let mut pending = vec![id.clone()];
        let mut visited = BTreeSet::new();
        let mut descriptors = BTreeSet::new();
        let mut modules = BTreeSet::new();
        let prefix = format!("{id}/");
        while let Some(id) = pending.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let sources = self.source.sources(&id).map_err(WorkspaceError::Source)?;
            modules.extend(
                sources
                    .iter()
                    .filter(|source| {
                        matches!(
                            source.role,
                            crate::project::designer_source::SourceRole::Module(_)
                        )
                    })
                    .map(|source| source.path.clone()),
            );
            let Some(location) = sources.iter().find(|source| {
                source.role == crate::project::designer_source::SourceRole::Descriptor
            }) else {
                continue;
            };
            if !descriptors.insert(location.path.clone()) {
                continue;
            }
            let descriptor = location
                .descriptor_id(&id)
                .ok_or(EditError::UnsupportedValue)?;
            let bytes = inputs.required(self.project().source(), &location.path)?;
            let parsed =
                metadata_parser::parse(text(&bytes)?, descriptor.parent(), PropertiesMode::Summary)
                    .map_err(|_| EditError::InvalidXml)?;
            pending.extend(
                parsed
                    .objects
                    .into_iter()
                    .map(|object| object.metadata.id().clone())
                    .chain(parsed.references.into_iter().map(|reference| reference.id))
                    .filter(|candidate| candidate.as_str().starts_with(&prefix)),
            );
        }
        Ok(modules)
    }

    pub(super) fn canonical_parent(&self, parent: Option<&ObjectId>) -> Option<ObjectId> {
        if self.source.root().kind() == MetadataKind::Configuration {
            parent.filter(|id| *id != self.source.root().id()).cloned()
        } else {
            Some(
                parent
                    .cloned()
                    .unwrap_or_else(|| self.source.root().id().clone()),
            )
        }
    }

    pub(super) fn object_context(
        &mut self,
        id: &ObjectId,
        inputs: &mut Inputs,
    ) -> Result<Context, ObjectError> {
        let object = self.reveal_declared_object(id)?;
        if object.kind == MetadataKind::PredefinedItem {
            return Err(ObjectError::Unsupported(object.kind));
        }
        let location = self
            .source
            .object_descriptor(id)
            .map_err(WorkspaceError::Source)?
            .ok_or(EditError::Conflict)?;
        let input = inputs.required(self.project().source(), &location.path)?;
        let input = text(&input)?.to_owned();
        let descriptor = location
            .descriptor_id(id)
            .ok_or(EditError::UnsupportedValue)?;
        let parsed = metadata_parser::parse(&input, descriptor.parent(), PropertiesMode::Summary)
            .map_err(|_| EditError::InvalidXml)?;
        let target = parsed
            .objects
            .iter()
            .find(|object| object.metadata.id() == id)
            .ok_or(EditError::Conflict)?;
        let document = xml::document(&input)?;
        let node = document
            .descendants()
            .find(|node| node.is_element() && node.range() == target.range)
            .ok_or(EditError::Conflict)?;
        let name_range = xml::name(node)?.range();
        if xml::properties(node)?.children().any(|node| {
            node.has_tag_name((xml::MD, "ObjectBelonging")) && node.text() == Some("Adopted")
        }) {
            return Err(EditError::ReadOnly.into());
        }
        let parts =
            crate::project::designer_source::identity_parts(id).map_err(WorkspaceError::Source)?;
        let ancestry = parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let tag = if index == 0
                    && self.project().configuration().project_type() == ProjectType::Processing
                {
                    "ExternalDataProcessor"
                } else if index == 0
                    && self.project().configuration().project_type() == ProjectType::Report
                {
                    "ExternalReport"
                } else {
                    part.kind.designer_tag()
                };
                (tag.to_owned(), part.name.clone())
            })
            .collect();
        Ok(Context {
            kind: object.kind,
            parent: object.parent,
            path: location.path,
            input,
            range: target.range.clone(),
            name_range,
            inline: !location.inline.is_empty(),
            ancestry,
            descriptor_parent: descriptor.parent(),
        })
    }

    pub(super) fn target_identity(
        &mut self,
        kind: MetadataKind,
        name: &str,
        parent: Option<&ObjectId>,
        inputs: &mut Inputs,
    ) -> Result<(ObjectId, Context), ObjectError> {
        let parent = self.canonical_parent(parent);
        let parent = parent.as_ref();
        validate_name(name).map_err(ObjectError::Name)?;
        if kind == MetadataKind::CommonModule {
            crate::project::metadata_rename::validate_common_module_name(name)
                .map_err(ObjectError::Name)?;
        }
        let owner = parent
            .cloned()
            .unwrap_or_else(|| self.source.root().id().clone());
        let context = self.object_context(&owner, inputs)?;
        let allowed = if parent.is_none() && context.kind == MetadataKind::Configuration {
            kind.collection_folder().is_some()
        } else {
            ConfiguratorSchema::collections(context.kind).contains(&kind)
        };
        if !allowed || kind == MetadataKind::PredefinedItem {
            return Err(ObjectError::Unsupported(kind));
        }
        crate::project::metadata_rename::validate_standard_child_name(
            context.kind.designer_tag(),
            kind,
            name,
        )
        .map_err(ObjectError::Name)?;
        let id = ObjectId::from_parts(parent, kind.as_str(), name);
        let parsed = metadata_parser::parse(
            &context.input,
            context.descriptor_parent.clone(),
            PropertiesMode::Summary,
        )
        .map_err(|_| EditError::InvalidXml)?;
        let collision = parsed.objects.iter().any(|item| {
            item.metadata.parent() == parent
                && super::super::renaming::context::shared_namespace(
                    parent,
                    item.metadata.kind(),
                    kind,
                )
                && same_name(item.metadata.name(), name)
        }) || parsed.references.iter().any(|item| {
            item.id.parent().as_ref() == parent
                && super::super::renaming::context::shared_namespace(parent, item.kind, kind)
                && same_name(&item.name, name)
        });
        if collision {
            return Err(ObjectError::Collision(id));
        }
        Ok((id, context))
    }

    pub(super) fn new_descriptor(
        &self,
        id: &ObjectId,
        parent: &Context,
    ) -> Result<Option<PathBuf>, ObjectError> {
        let (owner, paths) = self
            .source
            .descriptor_candidates(id)
            .map_err(WorkspaceError::Source)?;
        if owner != *id {
            return Ok(None);
        }
        if paths.len() == 1 {
            return Ok(paths.into_iter().next());
        }
        // External XML can use a wrapped or direct layout. Preserve its existing physical parent.
        let stem = parent.path.with_extension("");
        let mut candidates = paths.into_iter().filter(|path| {
            if self.project().source().join(&stem).is_dir() {
                path.starts_with(&stem)
            } else {
                !path.starts_with(&stem)
            }
        });
        let path = candidates
            .next()
            .ok_or_else(|| invalid("parent", "ambiguous_layout"))?;
        if candidates.next().is_some() {
            return Err(invalid("parent", "ambiguous_layout"));
        }
        Ok(Some(path))
    }
}
