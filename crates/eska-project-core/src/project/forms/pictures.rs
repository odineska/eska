//! Picture references remain inert image resources; a form never opens a referenced URL.

use std::ops::Range;

use super::{
    AppearanceDiagnostic, FormResource, PropertySource, SourcedProperty,
    presentation::located_metadata_property,
};
use crate::project::{
    designer_source::PicturePreview,
    metadata_model::{MetadataKind, MetadataValue},
    metadata_parser::ParsedObject,
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormPicture {
    Standard { name: String },
    Common { source: String },
    Missing { reference: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ButtonRepresentation {
    Text,
    Picture,
    PictureAndText,
}

impl ButtonRepresentation {
    pub(super) fn effective(
        picture: Option<&FormPicture>,
        explicit: Option<&str>,
        inherited: Option<&str>,
        automatic: Self,
    ) -> Self {
        if matches!(picture, Some(FormPicture::Missing { .. })) {
            return Self::Text;
        }
        let automatic = if picture.is_some() {
            automatic
        } else {
            Self::Text
        };
        [explicit, inherited]
            .into_iter()
            .flatten()
            .find_map(|value| match value {
                "Text" => Some(Self::Text),
                "Picture" => Some(Self::Picture),
                "PictureAndText" => Some(Self::PictureAndText),
                _ => None,
            })
            .unwrap_or(automatic)
    }
}

pub(super) fn form_picture(
    property: &SourcedProperty,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> Option<FormPicture> {
    resolve(
        &property.property.value,
        &property.origin.source,
        &property.origin.range,
        resources,
        diagnostics,
    )
}

pub(super) fn metadata_picture(
    owner: &ParsedObject,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> Option<FormPicture> {
    let (resource, property) = located_metadata_property(owner, "Picture", resources)?;
    resolve(
        &property.property.value,
        &resource.source,
        &property.range,
        resources,
        diagnostics,
    )
}

fn resolve(
    value: &MetadataValue,
    source: &str,
    range: &Range<usize>,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> Option<FormPicture> {
    let MetadataValue::Record(fields) = value else {
        return None;
    };
    let field = fields.iter().find(|field| {
        field.key.namespace.as_deref() == Some("http://v8.1c.ru/8.3/xcf/readable")
            && field.key.name == "Ref"
    })?;
    let reference = super::appearance::text(&field.value)?.trim();
    // Configurator serializes a cleared picture as Ref=0.
    if reference.is_empty() || reference == "0" {
        return None;
    }
    if let Some(name) = reference.strip_prefix("StdPicture.") {
        return Some(FormPicture::Standard { name: name.into() });
    }
    if let Some(name) = reference.strip_prefix("CommonPicture.") {
        let resource = resources.iter().rev().find(|resource| {
            resource.picture.is_some()
                && resource.descriptor.objects.iter().any(|object| {
                    object.metadata.kind() == MetadataKind::CommonPicture
                        && object.metadata.name() == name
                })
        });
        if let Some(resource) = resource {
            match &resource.picture {
                Some(PicturePreview::Ready(_)) => {
                    return Some(FormPicture::Common {
                        source: resource.source.clone(),
                    });
                }
                Some(PicturePreview::Empty { .. }) => return None,
                _ => {}
            }
        }
    }
    diagnostics.push(AppearanceDiagnostic {
        code: "picture_unavailable",
        origin: PropertySource {
            source: source.into(),
            range: range.clone(),
        },
        target: None,
    });
    Some(FormPicture::Missing {
        reference: reference.into(),
    })
}
