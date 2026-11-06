//! Design-time empty-value marks follow field overrides and the effective metadata attribute.

use super::{
    EffectiveElement, ElementKind, FormResource, PropertySource, appearance, metadata_property,
    standard_property,
};

pub(super) fn mark(
    element: &EffectiveElement,
    binding: Option<&PropertySource>,
    resources: &[FormResource],
) -> bool {
    if !matches!(
        element.kind,
        Some(ElementKind::InputField | ElementKind::Table)
    ) {
        return false;
    }
    match element.scalar("AutoMarkIncomplete") {
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
    let checking = if matches!(
        owner.metadata.kind(),
        crate::project::metadata_model::MetadataKind::Attribute
            | crate::project::metadata_model::MetadataKind::TabularSection
    ) {
        metadata_property(owner, "FillChecking", resources)
    } else {
        element
            .scalar("DataPath")
            .and_then(|path| path.rsplit('.').next())
            .and_then(|name| standard_property(owner, name, "FillChecking", resources))
    };
    checking.and_then(|value| appearance::text(&value.value)) == Some("ShowError")
}
