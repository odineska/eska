//! Deletion retains a full dependency preview; force permits dangling references, never hidden rewrites.

use super::super::renaming::bsl::Environment;
use super::{ProjectSession, context::Context, empty_plan};
use crate::project::{
    metadata_edit::{EditError, EditPlan, Replacement, snapshot},
    metadata_model::ObjectId,
    metadata_objects::{
        ObjectError, ObjectPlan, ObjectReference, ObjectRequest, invalid,
        inventory::{Inputs, io},
        xml,
    },
    metadata_parser::{self, PropertiesMode},
    metadata_rename::{
        ReferenceRename,
        bsl::BslRename,
        inventory::{Inventory, MAX_XML_BYTES, file_hash, read_text},
    },
    metadata_write::{DirectoryChange, FileChange},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::Path,
};

struct Incoming<'a> {
    declaration: &'a Context,
    parent_path: &'a Path,
    removal: Range<usize>,
    rename: ReferenceRename,
    code: BslRename,
    environment: Environment,
    identifiers: BTreeSet<String>,
}

impl ProjectSession {
    pub(super) fn delete_object(
        &mut self,
        id: &ObjectId,
        request: &ObjectRequest,
        saved: Option<&BTreeMap<String, String>>,
        inputs: &mut Inputs,
    ) -> Result<ObjectPlan, ObjectError> {
        if saved.is_some_and(|saved| !saved.is_empty()) {
            return Err(invalid("identities", "unexpected_mapping"));
        }
        let context = self.object_context(id, inputs)?;
        if id == self.source.root().id() {
            return Err(ObjectError::Unsupported(context.kind));
        }
        let root = self.project().source().to_path_buf();
        let parent = context
            .parent
            .clone()
            .unwrap_or_else(|| self.source.root().id().clone());
        let owner = self.object_context(&parent, inputs)?;
        let artifacts = self.object_sources(id, &context, inputs)?;
        let paths: BTreeSet<_> = artifacts.files.into_iter().collect();
        let mut plan = empty_plan(request, None, BTreeMap::new());
        let mut identifiers =
            xml::owned_ids(&context.input, context.inline.then_some(&context.range))?;
        for path in &paths {
            let bytes = inputs.required(&root, path)?;
            if let Some(input) = super::copying::xml_payload(path, &bytes)? {
                identifiers.extend(xml::owned_ids(input, None)?);
            }
            plan.files.push(FileChange {
                path: path.clone(),
                before: Some(bytes),
                after: None,
            });
        }
        plan.directories = artifacts
            .directories
            .into_iter()
            .map(|path| DirectoryChange {
                path,
                before: true,
                after: false,
            })
            .collect();
        let removal = declaration_removal(&context, &owner, id)?;
        let after = xml::remove(&owner.input, removal.clone())?;
        plan.files.push(FileChange {
            path: owner.path.clone(),
            before: Some(owner.input.as_bytes().to_vec()),
            after: Some(after.into_bytes()),
        });
        let excluded = self.rename_exclusions();
        let inventory =
            Inventory::read(&root, &excluded).map_err(|error| io(Path::new("."), error))?;
        inputs.observe_inventory(Path::new(""), &excluded, &inventory);
        let environment = Environment::read(
            &root,
            self.source.descriptor(),
            self.project().configuration().project_type(),
            &inventory,
        )
        .map_err(super::copying::rename_error)?;
        let generated = xml::generated(&context.input)?;
        let incoming = Incoming {
            declaration: &context,
            parent_path: &owner.path,
            removal,
            rename: ReferenceRename::new(&context.ancestry, "ESKADeletedObject", &generated)
                .map_err(ObjectError::Name)?,
            code: BslRename::new(&context.ancestry, "ESKADeletedObject"),
            environment,
            identifiers,
        };
        for directory in &inventory.directories {
            inputs.directories.insert(directory.clone(), true);
        }
        for path in inventory.files {
            incoming.scan(&root, &path, paths.contains(&path), inputs, &mut plan)?;
        }
        incoming
            .environment
            .check_snapshots(&inputs.observed)
            .map_err(super::copying::rename_error)?;
        plan.references.sort_by(|left, right| {
            (&left.path, left.range.start, left.reason).cmp(&(
                &right.path,
                right.range.start,
                right.reason,
            ))
        });
        plan.references.dedup();
        Ok(plan)
    }
}

fn declaration_removal(
    context: &Context,
    owner: &Context,
    id: &ObjectId,
) -> Result<Range<usize>, ObjectError> {
    if context.inline {
        return Ok(context.range.clone());
    }
    let parsed = metadata_parser::parse(
        &owner.input,
        owner.descriptor_parent.clone(),
        PropertiesMode::Summary,
    )
    .map_err(|_| EditError::InvalidXml)?;
    Ok(parsed
        .references
        .into_iter()
        .find(|reference| reference.id == *id)
        .ok_or(EditError::Conflict)?
        .range)
}

impl Incoming<'_> {
    fn scan(
        &self,
        root: &Path,
        path: &Path,
        deleted: bool,
        inputs: &mut Inputs,
        plan: &mut ObjectPlan,
    ) -> Result<(), ObjectError> {
        let is_xml = path.extension().is_some_and(|extension| extension == "xml")
            || path.file_name().is_some_and(|name| name == "Package.bin");
        let is_bsl = path.extension().is_some_and(|extension| extension == "bsl");
        if !is_xml && !is_bsl {
            let hash = file_hash(&root.join(path)).map_err(|error| io(path, error))?;
            inputs.observed.insert(
                path.to_path_buf(),
                crate::project::metadata_edit::snapshot_digest(&hash),
            );
            return Ok(());
        }
        let input = read_text(&root.join(path), MAX_XML_BYTES).map_err(|error| io(path, error))?;
        inputs.observed.insert(path.to_path_buf(), snapshot(&input));
        if deleted {
            return Ok(());
        }
        if path == Path::new("ConfigDumpInfo.xml") {
            let edit = remove_dump_entries(&input, &self.declaration.ancestry)?;
            if !edit.is_empty() {
                plan.files.push(FileChange {
                    path: path.to_path_buf(),
                    before: Some(input.into_bytes()),
                    after: Some(edit.output().as_bytes().to_vec()),
                });
            }
            return Ok(());
        }
        let analysis = if is_xml {
            self.rename.analyze_xml(&input, &[])?
        } else {
            self.code.analyze(
                &input,
                &self.environment.globals,
                self.environment.known_module(path),
            )
        };
        plan.references.extend(
            analysis
                .replacements
                .into_iter()
                .filter(|item| !self.removed(path, &item.range))
                .map(|item| ObjectReference {
                    path: path.to_path_buf(),
                    range: item.range,
                    reason: "metadata_reference",
                    confirmed: true,
                }),
        );
        plan.references.extend(
            analysis
                .uncertain
                .into_iter()
                .filter(|item| !self.removed(path, &item.range))
                .map(|item| ObjectReference {
                    path: path.to_path_buf(),
                    range: item.range,
                    reason: item.reason,
                    confirmed: false,
                }),
        );
        if is_xml {
            plan.references.extend(self.uuid_references(path, &input)?);
        }
        Ok(())
    }

    fn removed(&self, path: &Path, range: &Range<usize>) -> bool {
        path == self.parent_path
            && range.start >= self.removal.start
            && range.end <= self.removal.end
    }

    fn uuid_references(
        &self,
        path: &Path,
        input: &str,
    ) -> Result<Vec<ObjectReference>, ObjectError> {
        let document = xml::document(input)?;
        let mut references = Vec::new();
        for node in document
            .descendants()
            .filter(roxmltree::Node::is_element)
            .filter(|node| !self.removed(path, &node.range()))
        {
            let ranges = node
                .attributes()
                .filter(|attribute| {
                    self.identifiers
                        .contains(&attribute.value().to_ascii_lowercase())
                })
                .map(|attribute| attribute.range_value())
                .chain(
                    node.children()
                        .filter(roxmltree::Node::is_text)
                        .filter(|text| {
                            text.text().is_some_and(|value| {
                                self.identifiers.contains(&value.to_ascii_lowercase())
                            })
                        })
                        .map(|text| text.range()),
                );
            references.extend(ranges.map(|range| ObjectReference {
                path: path.to_path_buf(),
                range,
                reason: "uuid_reference",
                confirmed: true,
            }));
        }
        Ok(references)
    }
}

fn remove_dump_entries(
    input: &str,
    ancestry: &[(String, String)],
) -> Result<EditPlan, ObjectError> {
    let document = xml::document(input)?;
    let name = ancestry
        .iter()
        .flat_map(|(kind, name)| [kind.as_str(), name.as_str()])
        .collect::<Vec<_>>()
        .join(".");
    let prefix = format!("{name}.");
    let matches = |node: roxmltree::Node<'_, '_>| {
        node.has_tag_name(("http://v8.1c.ru/8.3/xcf/dumpinfo", "Metadata"))
            && node
                .attribute("name")
                .is_some_and(|value| value == name || value.starts_with(&prefix))
    };
    let changes = document
        .descendants()
        .filter(roxmltree::Node::is_element)
        .filter(|node| matches(*node) && !node.ancestors().skip(1).any(matches))
        .map(|node| Replacement {
            range: node.range(),
            text: String::new(),
        })
        .collect();
    Ok(EditPlan::from_replacements(input, changes)?)
}
