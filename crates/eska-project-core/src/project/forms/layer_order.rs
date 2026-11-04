//! Native extension purpose precedes the selected context's order within each purpose.

use std::collections::HashMap;

use super::{
    FormContextError, FormDependency, FormDocument, FormResource, context::source_identity,
};
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

pub(super) fn apply(
    context: &ContextSnapshot,
    dependencies: &[FormDependency],
    documents: &mut [FormDocument],
    resources: &mut [FormResource],
) -> Result<(), FormContextError> {
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
    let inputs: HashMap<_, _> = dependencies
        .iter()
        .filter_map(|dependency| {
            roots
                .get(dependency.root.as_path())
                .copied()
                .map(|priority| (dependency.id.as_str(), priority))
        })
        .collect();
    // Validate before mutating either sequence; every input must belong to a selected source.
    if let Some(source) = documents
        .iter()
        .map(|document| document.source.as_str())
        .chain(resources.iter().map(|resource| resource.source.as_str()))
        .find(|source| !inputs.contains_key(source))
    {
        return Err(FormContextError::InvalidMetadata {
            source: source.into(),
        });
    }
    // Both form deltas and style/type resources use the same layer precedence. Purpose is
    // read from the parsed metadata overlay, so unsaved Configuration.xml changes participate.
    documents.sort_by_key(|document| inputs.get(document.source.as_str()).copied());
    resources.sort_by_key(|resource| inputs.get(resource.source.as_str()).copied());
    Ok(())
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
