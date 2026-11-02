//! Design-time empty-value marks follow field overrides and the effective metadata attribute.

use super::{
    EffectiveElement, ElementKind, FormResource, PropertySource, appearance, metadata_property,
    property, standard_property,
};

pub(super) fn mark(
    element: &EffectiveElement,
    binding: Option<&PropertySource>,
    resources: &[FormResource],
) -> bool {
    if element.kind != Some(ElementKind::InputField) {
        return false;
    }
    let scalar =
        |name| property(element, name).and_then(|value| appearance::text(&value.property.value));
    match scalar("AutoMarkIncomplete") {
        Some("true") => return true,
        Some("false") => return false,
        _ => {}
    }
    let Some(binding) = binding else { return false };
    let Some(owner) = resources
        .iter()
        .filter(|resource| resource.source == binding.source)
        .flat_map(|resource| &resource.descriptor.objects)
        .find(|object| object.range == binding.range)
    else {
        return false;
    };
    let checking =
        if owner.metadata.kind() == crate::project::metadata_model::MetadataKind::Attribute {
            metadata_property(owner, "FillChecking", resources)
        } else {
            scalar("DataPath")
                .and_then(|path| path.rsplit('.').next())
                .and_then(|name| standard_property(owner, name, "FillChecking", resources))
        };
    checking.and_then(|value| appearance::text(&value.value)) == Some("ShowError")
}
