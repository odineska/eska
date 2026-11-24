//! Shared Configurator notation for physical metadata ownership paths.

use crate::localization::Localizer;
use eska_project_core::project::metadata::{MetadataPath, MetadataSourcePath};

/// Render a logical metadata identity in Configurator notation.
#[must_use]
pub fn render_metadata_path(path: &MetadataPath, localizer: &Localizer) -> String {
    path.parts
        .iter()
        .map(|part| {
            let kind = metadata_kind(part.kind, localizer);
            part.name
                .as_ref()
                .map_or_else(|| kind.clone(), |name| format!("{kind}.{name}"))
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Render a physical source with the detail needed to distinguish it from its owner's other files.
#[must_use]
pub fn render_metadata_source_path(path: &MetadataSourcePath<'_>, localizer: &Localizer) -> String {
    let mut caption = render_metadata_path(&path.logical, localizer);
    if let Some(artifact) = &path.artifact {
        caption.push('.');
        caption.push_str(&metadata_kind(artifact.kind, localizer));
        if let Some(name) = artifact.name {
            caption.push('.');
            caption.push_str(name);
        }
    }
    caption
}

/// Resolve the localized Configurator name of a stable metadata kind.
#[must_use]
pub fn metadata_kind(kind: &str, localizer: &Localizer) -> String {
    localizer.text(&format!("diff-metadata-{kind}"))
}
