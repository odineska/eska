//! Loaded borrowed metadata must exist in the base, rather than in another extension.

use std::collections::{HashMap, HashSet};

use super::{FormContextError, FormDependency, FormResource};
use crate::{
    application::{ContextSnapshot, SourceRole},
    project::{
        metadata_model::{MetadataKind, MetadataValue},
        metadata_parser::MD_NAMESPACE,
    },
};

pub(super) fn validate(
    context: &ContextSnapshot,
    dependencies: &[FormDependency],
    resources: &[FormResource],
) -> Result<(), FormContextError> {
    let roots: HashMap<_, _> = context
        .context
        .sources()
        .iter()
        .map(|source| (source.project().source(), source.role()))
        .collect();
    // A standalone extension may still show its independent part as incomplete.
    if !roots
        .values()
        .any(|role| *role == SourceRole::Configuration)
    {
        return Ok(());
    }
    let roles: HashMap<_, _> = dependencies
        .iter()
        .filter_map(|input| {
            roots
                .get(input.root.as_path())
                .map(|role| (input.id.as_str(), *role))
        })
        .collect();
    let base: HashSet<_> = resources
        .iter()
        .filter(|input| roles.get(input.source.as_str()) == Some(&SourceRole::Configuration))
        .flat_map(|input| &input.descriptor.objects)
        .map(|object| object.metadata.id())
        .collect();
    for resource in resources
        .iter()
        .filter(|input| roles.get(input.source.as_str()) == Some(&SourceRole::Extension))
    {
        for object in &resource.descriptor.objects {
            let borrowed = object.metadata.kind() != MetadataKind::Configuration
                && object.properties.as_ref().is_some_and(|properties| {
                    properties.iter().any(|property| {
                        property.property.key.namespace.as_deref() == Some(MD_NAMESPACE)
                            && property.property.key.name == "ObjectBelonging"
                            && matches!(&property.property.value, MetadataValue::Text(value) if value == "Adopted")
                    })
                });
            // This is intentionally limited to already loaded form dependencies. Native
            // applicability of the extension's unrelated metadata requires a separate check.
            if borrowed && !base.contains(object.metadata.id()) {
                return Err(FormContextError::InvalidMetadata {
                    source: resource.source.clone(),
                });
            }
        }
    }
    Ok(())
}
