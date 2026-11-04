//! Picture references remain inert image resources; a form never opens a referenced URL.

use super::{AppearanceDiagnostic, EffectiveElement, FormResource};
use crate::project::{
    designer_source::PicturePreview,
    metadata_model::{MetadataKind, MetadataValue},
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FormPicture {
    Standard { name: String },
    Common { source: String },
    Missing { reference: String },
}

pub(super) fn resolve(
    element: &EffectiveElement,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> Option<FormPicture> {
    let property = element.properties.iter().find(|property| {
        property.property.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
            && property.property.key.name == "Picture"
    })?;
    let MetadataValue::Record(fields) = &property.property.value else {
        return None;
    };
    let field = fields.iter().find(|field| {
        field.key.namespace.as_deref() == Some("http://v8.1c.ru/8.3/xcf/readable")
            && field.key.name == "Ref"
    })?;
    let reference = super::appearance::text(&field.value)?.trim();
    if reference.is_empty() {
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
        origin: property.origin.clone(),
        target: None,
    });
    Some(FormPicture::Missing {
        reference: reference.into(),
    })
}
