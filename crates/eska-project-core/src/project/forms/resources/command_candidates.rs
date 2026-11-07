//! Test applicability against effective metadata before requesting pictures and other resources.

use std::collections::{HashMap, HashSet};

use super::{KnownPaths, Loader, Reader, configuration_objects, descriptor_properties};
use crate::{
    application::SourceRole,
    project::{
        forms::{
            ComposedForm, FormContextError, MAX_FORM_CONTEXT_BYTES, MAX_FORM_DEPENDENCIES,
            appearance::text,
            command_types::{self, PARAMETER},
            root_commands::{BAR_GROUP_CATEGORY, BarGroup, interface_items, metadata_placements},
        },
        metadata_model::{MetadataKind, MetadataValue, ObjectId},
        metadata_parser::{self, MD_NAMESPACE},
    },
};

pub(super) fn select(
    loader: &Loader<'_>,
    readers: &[Reader<'_>],
    form: &ComposedForm,
    parameter: &str,
    known_paths: &KnownPaths<'_>,
) -> Result<Vec<ObjectId>, FormContextError> {
    let placements = metadata_placements(&interface_items(form));
    let mut probes = Probes {
        loader,
        readers,
        known_paths,
        definitions: HashMap::new(),
        definition_bytes: 0,
        categories: HashMap::new(),
    };
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    let candidates = loader
        .output
        .iter()
        .flat_map(|resource| configuration_objects(&resource.descriptor))
        .filter(|reference| reference.kind == MetadataKind::CommonCommand);
    for candidate in candidates {
        let id = &candidate.id;
        // Explicit uses are already handled by the full resource loader.
        if loader.requested.contains(id) || !seen.insert(id) {
            continue;
        }
        let [Some(types), group] =
            probes.properties(id, MetadataKind::CommonCommand, [PARAMETER, "Group"])?
        else {
            continue;
        };
        let group = placements
            .get(id)
            .copied()
            .or_else(|| group.as_ref().and_then(|value| text(&value.value)));
        let Some(group) = group.and_then(BarGroup::parse) else {
            continue;
        };
        if probes.on_bar(group)? && probes.accepts(&types.value, parameter)? {
            selected.push(id.clone());
        }
    }
    Ok(selected)
}

struct Property {
    value: MetadataValue,
    bytes: usize,
}

/// Probe results live for one load. Every revision reads the active layers and overlays again.
struct Probes<'a, 'input> {
    loader: &'a Loader<'input>,
    readers: &'a [Reader<'input>],
    known_paths: &'a KnownPaths<'a>,
    definitions: HashMap<ObjectId, Option<MetadataValue>>,
    definition_bytes: usize,
    categories: HashMap<ObjectId, bool>,
}

impl Probes<'_, '_> {
    fn on_bar(&mut self, group: BarGroup) -> Result<bool, FormContextError> {
        let BarGroup::Named(id) = group else {
            return Ok(true);
        };
        if let Some(result) = self.categories.get(&id) {
            return Ok(*result);
        }
        let [category] = self.properties(&id, MetadataKind::CommandGroup, ["Category"])?;
        let on_bar = category.as_ref().and_then(|property| text(&property.value))
            == Some(BAR_GROUP_CATEGORY);
        self.categories.insert(id, on_bar);
        Ok(on_bar)
    }

    fn accepts(
        &mut self,
        value: &MetadataValue,
        parameter: &str,
    ) -> Result<bool, FormContextError> {
        if command_types::matches_parameter(value, parameter, |_| None) {
            return Ok(true);
        }
        let mut pending = command_types::type_sets(value).collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            if self.definitions.contains_key(&id) {
                continue;
            }
            if self.definitions.len() == MAX_FORM_DEPENDENCIES {
                return Err(FormContextError::TooManyResources);
            }
            let [definition] = self.properties(&id, MetadataKind::DefinedType, ["Type"])?;
            if let Some(definition) = &definition {
                self.definition_bytes = self.definition_bytes.saturating_add(definition.bytes);
                if self.definition_bytes > MAX_FORM_CONTEXT_BYTES {
                    return Err(FormContextError::InputTooLarge);
                }
                pending.extend(command_types::type_sets(&definition.value));
            }
            self.definitions
                .insert(id, definition.map(|property| property.value));
        }
        Ok(command_types::matches_parameter(value, parameter, |id| {
            self.definitions.get(id).and_then(Option::as_ref)
        }))
    }

    fn properties<const N: usize>(
        &self,
        id: &ObjectId,
        kind: MetadataKind,
        names: [&str; N],
    ) -> Result<[Option<Property>; N], FormContextError> {
        let mut values = [const { None }; N];
        for reader in self
            .readers
            .iter()
            .rev()
            .filter(|reader| reader.role != SourceRole::ExternalObject)
        {
            let Some((input, known)) = self.loader.probe_input(reader, id, self.known_paths)?
            else {
                continue;
            };
            if !known
                && self.readers.len() == 1
                && kind == MetadataKind::CommonCommand
                && !command_types::may_declare(&input.text)
            {
                continue;
            }
            let xml = match metadata_parser::parse_xml(
                &input.text,
                metadata_parser::MAX_DESCRIPTOR_NODES,
            ) {
                Ok(xml) => xml,
                Err(_) if known => {
                    return Err(FormContextError::InvalidMetadata { source: input.id });
                }
                Err(_) => continue,
            };
            let Some(properties) = descriptor_properties(xml.root_element(), kind) else {
                continue;
            };
            // Match the projection's property-by-property inheritance, including an explicit empty value.
            for (name, slot) in names.iter().zip(&mut values) {
                if slot.is_none()
                    && let Some(node) = properties
                        .children()
                        .find(|node| node.has_tag_name((MD_NAMESPACE, *name)))
                {
                    *slot = Some(Property {
                        value: metadata_parser::values::property(node, &mut Vec::new()).value,
                        bytes: node.range().len(),
                    });
                }
            }
        }
        Ok(values)
    }
}
