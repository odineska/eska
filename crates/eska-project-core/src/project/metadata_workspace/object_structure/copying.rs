//! Deep copies enumerate the owned physical subtree and remap only declared identities.

use super::super::renaming::bsl::Environment;
use super::{ProjectSession, context::Context, empty_plan};
use crate::project::{
    metadata_edit::{EditError, EditPlan, Replacement},
    metadata_model::ObjectId,
    metadata_objects::{
        ObjectError, ObjectPlan, ObjectReference, ObjectRequest, invalid,
        inventory::{Inputs, io, target_directories, text},
        xml,
    },
    metadata_rename::{ReferenceRename, bsl::BslRename, inventory::Inventory},
    metadata_write::FileChange,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

struct CopyBindings {
    identifiers: BTreeSet<String>,
    generated: Vec<String>,
    implicit_names: BTreeSet<String>,
}

struct Copier<'a> {
    context: &'a Context,
    rename: ReferenceRename,
    code: BslRename,
    environment: Option<Environment>,
    local_modules: BTreeSet<PathBuf>,
}

impl ProjectSession {
    pub(super) fn copy_object(
        &mut self,
        source: &ObjectId,
        name: &str,
        parent: Option<&ObjectId>,
        request: &ObjectRequest,
        saved: Option<&BTreeMap<String, String>>,
        inputs: &mut Inputs,
    ) -> Result<ObjectPlan, ObjectError> {
        let context = self.object_context(source, inputs)?;
        if source == self.source.root().id() {
            return Err(ObjectError::Unsupported(context.kind));
        }
        let requested_parent = self.canonical_parent(parent.or(context.parent.as_ref()));
        let parent = requested_parent.as_ref();
        if parent != context.parent.as_ref() {
            return Err(invalid("parent", "cross_owner_copy"));
        }
        let (id, owner) = self.target_identity(context.kind, name, parent, inputs)?;
        let root = self.project().source().to_path_buf();
        let artifacts = self.object_sources(source, &context, inputs)?;
        let bindings = copy_bindings(&root, &context, &artifacts.files, inputs)?;
        let identities = xml::identities(&bindings.identifiers, saved)?;
        let rename = ReferenceRename::new(&context.ancestry, name, &bindings.generated)
            .map_err(ObjectError::Name)?;
        let environment =
            self.copy_environment(&artifacts.files, bindings.implicit_names, inputs)?;
        let local_modules = if environment.is_some() {
            self.owned_module_context(source, inputs)?
        } else {
            BTreeSet::new()
        };
        let copier = Copier {
            context: &context,
            rename,
            code: BslRename::new(&context.ancestry, name),
            environment,
            local_modules,
        };
        let mut plan = empty_plan(request, Some(id.clone()), identities);
        let new_path = if context.inline {
            context.path.clone()
        } else {
            self.new_descriptor(&id, &owner)?
                .ok_or(EditError::UnsupportedValue)?
        };
        let bases: Vec<_> = artifacts
            .roots
            .iter()
            .map(|old| {
                let new = if context.inline {
                    old.with_file_name(name)
                } else {
                    new_path.with_extension("")
                };
                (old.clone(), new)
            })
            .collect();
        for (_, target) in &bases {
            ensure_absent_tree(&root, inputs, target, &id)?;
        }
        let new_directories: Vec<_> = artifacts
            .directories
            .iter()
            .filter_map(|path| mapped(path, &bases))
            .collect();
        for path in &artifacts.files {
            let target = if *path == context.path {
                new_path.clone()
            } else {
                mapped(path, &bases).ok_or(EditError::UnsafePath)?
            };
            ensure_absent(&root, inputs, &target, &id)?;
            copier.append(path, target, inputs.required(&root, path)?, &mut plan)?;
        }
        let after = copier.parent_after(&owner, name, &plan.prepared.identities)?;
        plan.files.push(FileChange {
            path: owner.path,
            before: Some(owner.input.into_bytes()),
            after: Some(after.into_bytes()),
        });
        plan.directories = target_directories(&root, inputs, &plan.files, new_directories)?;
        Ok(plan)
    }

    fn copy_environment(
        &self,
        files: &[PathBuf],
        implicit_names: BTreeSet<String>,
        inputs: &mut Inputs,
    ) -> Result<Option<Environment>, ObjectError> {
        if !files
            .iter()
            .any(|path| path.extension().is_some_and(|extension| extension == "bsl"))
        {
            return Ok(None);
        }
        let root = self.project().source();
        let inventory = Inventory::read(root, &self.rename_exclusions())
            .map_err(|error| io(Path::new("."), error))?;
        let mut environment = Environment::read(
            root,
            self.source.descriptor(),
            self.project().configuration().project_type(),
            &inventory,
        )
        .map_err(rename_error)?;
        for (path, expected) in environment.input_snapshots() {
            if inputs
                .files
                .get(path)
                .and_then(Option::as_ref)
                .is_some_and(|bytes| {
                    crate::project::metadata_edit::snapshot_bytes(bytes) != *expected
                })
            {
                return Err(EditError::Conflict.into());
            }
        }
        environment.globals.shadowed.extend(implicit_names);
        inputs
            .observed
            .extend(environment.input_snapshots().clone());
        Ok(Some(environment))
    }
}

fn copy_bindings(
    root: &Path,
    context: &Context,
    files: &[PathBuf],
    inputs: &mut Inputs,
) -> Result<CopyBindings, ObjectError> {
    let mut result = CopyBindings {
        identifiers: xml::owned_ids(&context.input, context.inline.then_some(&context.range))?,
        generated: xml::generated(&context.input)?,
        implicit_names: BTreeSet::new(),
    };
    for path in files {
        let bytes = inputs.required(root, path)?;
        if let Some(input) = xml_payload(path, &bytes)? {
            result.identifiers.extend(xml::owned_ids(input, None)?);
            result.generated.extend(xml::generated(input)?);
            for node in xml::document(input)?
                .descendants()
                .filter(roxmltree::Node::is_element)
            {
                if (node.has_tag_name((xml::MD, "Name"))
                    || matches!(node.tag_name().name(), "Attribute" | "Parameter"))
                    && let Some(name) = node.attribute("name").or_else(|| node.text())
                {
                    result.implicit_names.insert(name.to_owned());
                }
            }
        }
    }
    Ok(result)
}

impl Copier<'_> {
    fn append(
        &self,
        path: &Path,
        target: PathBuf,
        before: Vec<u8>,
        plan: &mut ObjectPlan,
    ) -> Result<(), ObjectError> {
        let mut uncertain = Vec::new();
        let after = if let Some(input) = xml_payload(path, &before)? {
            let declarations = if path == self.context.path {
                vec![self.context.name_range.clone()]
            } else {
                Vec::new()
            };
            let (output, analysis) = xml::copy_xml(
                input,
                &self.rename,
                &declarations,
                &plan.prepared.identities,
            )?;
            uncertain = analysis.uncertain;
            output.into_bytes()
        } else if path.extension().is_some_and(|extension| extension == "bsl") {
            let input = text(&before)?;
            let environment = self.environment.as_ref().ok_or(EditError::Conflict)?;
            let analysis = self.code.analyze(
                input,
                &environment.globals,
                environment.known_module(path) || self.local_modules.contains(path),
            );
            let replacements = analysis
                .replacements
                .into_iter()
                .map(|item| Replacement {
                    range: item.range,
                    text: item.after,
                })
                .collect();
            uncertain = analysis.uncertain;
            EditPlan::from_replacements(input, replacements)?
                .output()
                .as_bytes()
                .to_vec()
        } else {
            before
        };
        plan.references
            .extend(uncertain.into_iter().map(|item| ObjectReference {
                path: target.clone(),
                range: item.range,
                reason: item.reason,
                confirmed: false,
            }));
        plan.files.push(FileChange {
            path: target,
            before: None,
            after: Some(after),
        });
        Ok(())
    }

    fn parent_after(
        &self,
        owner: &Context,
        name: &str,
        identities: &BTreeMap<String, String>,
    ) -> Result<String, ObjectError> {
        if !self.context.inline {
            return xml::add_reference(
                &owner.input,
                owner.range.clone(),
                self.context.kind.designer_tag(),
                name,
            );
        }
        let (output, _) = xml::copy_xml(
            &self.context.input,
            &self.rename,
            std::slice::from_ref(&self.context.name_range),
            identities,
        )?;
        let parsed = xml::document(&output)?;
        let old = xml::document(&self.context.input)?;
        let old_uuid = old
            .descendants()
            .find(|node| node.is_element() && node.range() == self.context.range)
            .and_then(|node| node.attribute("uuid"))
            .ok_or(EditError::InvalidXml)?;
        let new_uuid = identities
            .get(&old_uuid.to_ascii_lowercase())
            .ok_or(EditError::InvalidXml)?;
        let node = parsed
            .descendants()
            .find(|node| node.is_element() && node.attribute("uuid") == Some(new_uuid))
            .ok_or(EditError::Conflict)?;
        xml::insert_child(
            &owner.input,
            owner.range.clone(),
            "ChildObjects",
            &output[node.range()],
        )
    }
}

pub(super) fn xml_payload<'a>(
    path: &Path,
    bytes: &'a [u8],
) -> Result<Option<&'a str>, ObjectError> {
    let xml = path.extension().is_some_and(|extension| extension == "xml");
    let utf8 = std::str::from_utf8(bytes).ok();
    let embedded = path.file_name().is_some_and(|name| name == "Package.bin")
        && utf8.is_some_and(|input| {
            input
                .trim_start_matches('\u{feff}')
                .trim_start()
                .starts_with('<')
        });
    if xml || embedded {
        Ok(Some(text(bytes)?))
    } else {
        Ok(None)
    }
}

fn mapped(path: &Path, bases: &[(PathBuf, PathBuf)]) -> Option<PathBuf> {
    bases
        .iter()
        .find_map(|(old, new)| path.strip_prefix(old).ok().map(|suffix| new.join(suffix)))
}

pub(super) fn ensure_absent(
    root: &Path,
    inputs: &mut Inputs,
    path: &Path,
    id: &ObjectId,
) -> Result<(), ObjectError> {
    if inputs.read(root, path)?.is_some() {
        return Err(ObjectError::Collision(id.clone()));
    }
    // Keep Linux-created sources portable to case-insensitive Designer workspaces.
    if let Some(parent) = path.parent().filter(|parent| root.join(parent).is_dir()) {
        let expected = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(EditError::UnsafePath)?;
        for entry in std::fs::read_dir(root.join(parent)).map_err(|error| io(parent, error))? {
            let entry = entry.map_err(|error| io(parent, error))?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| crate::project::metadata_rename::same_name(name, expected))
            {
                return Err(ObjectError::Collision(id.clone()));
            }
        }
    }
    Ok(())
}

pub(super) fn ensure_absent_tree(
    root: &Path,
    inputs: &mut Inputs,
    path: &Path,
    id: &ObjectId,
) -> Result<(), ObjectError> {
    if inputs.directory(root, path)? {
        return Err(ObjectError::Collision(id.clone()));
    }
    if let Some(parent) = path.parent().filter(|parent| root.join(parent).is_dir()) {
        let expected = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(EditError::UnsafePath)?;
        for entry in std::fs::read_dir(root.join(parent)).map_err(|error| io(parent, error))? {
            let entry = entry.map_err(|error| io(parent, error))?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| crate::project::metadata_rename::same_name(name, expected))
            {
                return Err(ObjectError::Collision(id.clone()));
            }
        }
    }
    Ok(())
}

pub(super) fn rename_error(error: super::super::RenameError) -> ObjectError {
    use super::super::RenameError;
    match error {
        RenameError::Workspace(error) => ObjectError::Workspace(error),
        RenameError::Edit(error) => ObjectError::Edit(error),
        RenameError::Name(error) => ObjectError::Name(error),
        RenameError::Collision(id) => ObjectError::Collision(id),
        RenameError::Io { path, source } => ObjectError::Io { path, source },
        _ => invalid("source", "reference_context_unavailable"),
    }
}
