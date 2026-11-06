//! Native extension purpose precedes the selected context's order within each purpose.

use std::{collections::HashMap, path::Path};

use super::{FormContextError, FormDependency, FormResource, context::source_identity};
use crate::{
    application::{ContextSnapshot, SourceRole},
    project::{metadata_model::MetadataValue, metadata_parser::MD_NAMESPACE},
};

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Priority {
    Configuration,
    Patch,
    Customization,
    AddOn,
    ExternalObject,
}

pub(super) struct LayerOrder<'a> {
    roots: HashMap<&'a Path, (Priority, usize)>,
}

impl<'a> LayerOrder<'a> {
    pub(super) fn resolve(
        context: &'a ContextSnapshot,
        resources: &[FormResource],
    ) -> Result<Self, FormContextError> {
        let mut roots = HashMap::new();
        for (position, input) in context.context.sources().iter().enumerate() {
            let priority = match input.role() {
                SourceRole::Configuration => Priority::Configuration,
                SourceRole::ExternalObject => Priority::ExternalObject,
                SourceRole::Extension => {
                    let id = source_identity(context, input.project().source(), input.descriptor());
                    let descriptor = resources
                        .iter()
                        .find(|resource| resource.source == id)
                        .ok_or(FormContextError::InvalidMetadata { source: id })?;
                    extension_priority(descriptor)?
                }
            };
            roots.insert(input.project().source(), (priority, position));
        }
        Ok(Self { roots })
    }

    pub(super) fn sort<T>(
        &self,
        inputs: &mut [T],
        dependencies: &[FormDependency],
        source: impl Fn(&T) -> &str,
    ) -> Result<(), FormContextError> {
        let positions: HashMap<_, _> = dependencies
            .iter()
            .filter_map(|dependency| {
                self.roots
                    .get(dependency.root.as_path())
                    .map(|priority| (dependency.id.as_str(), *priority))
            })
            .collect();
        if let Some(source) = inputs
            .iter()
            .map(&source)
            .find(|source| !positions.contains_key(source))
        {
            return Err(FormContextError::InvalidMetadata {
                source: source.into(),
            });
        }
        inputs.sort_by_key(|input| positions.get(source(input)).copied());
        Ok(())
    }
}

fn extension_priority(resource: &FormResource) -> Result<Priority, FormContextError> {
    let invalid = || FormContextError::InvalidMetadata {
        source: resource.source.clone(),
    };
    let properties = resource
        .descriptor
        .objects
        .first()
        .and_then(|object| object.properties.as_ref())
        .ok_or_else(invalid)?;
    let mut values = properties.iter().filter(|property| {
        property.property.key.name == "ConfigurationExtensionPurpose"
            && property.property.key.namespace.as_deref() == Some(MD_NAMESPACE)
    });
    let Some(property) = values.next() else {
        return Ok(Priority::Customization);
    };
    if values.next().is_some() {
        return Err(invalid());
    }
    match &property.property.value {
        MetadataValue::Text(value) => match value.as_str() {
            "Patch" => Ok(Priority::Patch),
            "Customization" => Ok(Priority::Customization),
            "AddOn" => Ok(Priority::AddOn),
            _ => Err(invalid()),
        },
        _ => Err(invalid()),
    }
}
