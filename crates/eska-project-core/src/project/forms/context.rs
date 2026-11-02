//! Reuse the application's ordered source set and checked Designer ownership; never scan the workspace.

use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{ComposedForm, FormDocument, FormParseError, compose, parse};
use crate::{
    application::{ContextSnapshot, SourceRole},
    project::{
        designer_source::{self, SourceError},
        metadata_model::{ObjectId, ProjectScope},
    },
};

/// Unsaved text can replace only an input already resolved from the application and logical form owner.
#[derive(Clone, Debug)]
pub struct FormOverlay {
    pub source: String,
    pub text: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDependency {
    pub id: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub snapshot: String,
    pub version: Option<String>,
    pub member: Option<String>,
    pub role: FormDependencyRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormDependencyRole {
    Form,
    Metadata,
    Picture,
}

#[derive(Debug)]
pub struct FormPreview {
    pub context_fingerprint: String,
    pub application: Option<String>,
    pub limited_context: bool,
    pub dependencies: Vec<FormDependency>,
    pub documents: Vec<FormDocument>,
    pub resources: Vec<super::FormResource>,
    pub form: ComposedForm,
    pub presentation: super::FormPresentation,
}

#[derive(Debug)]
pub enum FormContextError {
    Source(SourceError),
    Parse {
        source: String,
        error: FormParseError,
    },
    MemberMismatch,
    MissingForm,
    InvalidOverlay,
    TooManyResources,
    InputTooLarge,
    InvalidMetadata {
        source: String,
    },
}

/// Load only this form from the already selected application, in dependency order.
///
/// # Errors
/// Rejects unknown members, missing managed forms, foreign/duplicate overlays and invalid XML.
pub fn load(
    context: &ContextSnapshot,
    selected: &Path,
    owner: &ObjectId,
    overlays: &[FormOverlay],
) -> Result<FormPreview, FormContextError> {
    let selected_source = context
        .context
        .sources()
        .iter()
        .find(|source| source.project().source() == selected)
        .ok_or(FormContextError::MemberMismatch)?;
    let external = selected_source.role() == SourceRole::ExternalObject;
    let mut dependencies = Vec::new();
    let mut documents = Vec::new();
    let mut selected_found = false;
    let mut input_bytes = 0usize;
    for input in context.context.sources().iter().filter(|source| {
        if external {
            source.project().source() == selected
        } else {
            source.role() != SourceRole::ExternalObject
        }
    }) {
        let scope = input.member().map_or(ProjectScope::Standalone, |member| {
            ProjectScope::Member(member.clone())
        });
        let (source, _) = designer_source::open_analysis_project(input.project().clone(), scope)
            .map_err(FormContextError::Source)?;
        let Some(location) = source
            .sources(owner)
            .map_err(FormContextError::Source)?
            .into_iter()
            .find(|location| {
                location.role == designer_source::SourceRole::Payload
                    && location
                        .path
                        .file_name()
                        .is_some_and(|name| name == "Form.xml")
            })
        else {
            continue;
        };
        let id = source_identity(context, input.project().source(), &location.path);
        let overlay = overlays.iter().find(|overlay| overlay.source == id);
        let text = match overlay {
            Some(overlay) => overlay.text.clone(),
            None => source
                .read_xml(&location.path)
                .map_err(FormContextError::Source)?
                .ok_or(FormContextError::MissingForm)?,
        };
        input_bytes = input_bytes.saturating_add(text.len());
        if input_bytes > super::MAX_FORM_CONTEXT_BYTES {
            return Err(FormContextError::InputTooLarge);
        }
        let document = parse(&id, text).map_err(|error| FormContextError::Parse {
            source: id.clone(),
            error,
        })?;
        selected_found |= input.project().source() == selected;
        dependencies.push(FormDependency {
            id,
            root: input.project().source().into(),
            path: location.path,
            snapshot: document.revision.clone(),
            version: overlay.map(|overlay| overlay.version.clone()),
            member: input.member().map(|member| member.as_str().into()),
            role: FormDependencyRole::Form,
        });
        documents.push(document);
        if dependencies.len() > super::MAX_FORM_DEPENDENCIES {
            return Err(FormContextError::TooManyResources);
        }
    }
    if !selected_found {
        return Err(FormContextError::MissingForm);
    }
    let resources = super::resources::load(
        context,
        owner,
        selected_source,
        &documents,
        &mut dependencies,
        overlays,
    )?;
    validate_overlays(overlays, &dependencies)?;
    let (base, extensions) = documents
        .split_first()
        .ok_or(FormContextError::MissingForm)?;
    let form = compose(base, &extensions.iter().collect::<Vec<_>>());
    let presentation = super::present(&form, &documents, &resources);
    Ok(FormPreview {
        context_fingerprint: context.fingerprint.clone(),
        application: context.context.name().map(|name| name.as_str().into()),
        limited_context: context.context.limited_context(),
        dependencies,
        documents,
        resources,
        form,
        presentation,
    })
}

pub(super) fn source_identity(context: &ContextSnapshot, root: &Path, path: &Path) -> String {
    let mut digest = Sha256::new();
    for field in [
        context.context.root().as_os_str().as_encoded_bytes(),
        context
            .context
            .name()
            .map_or("", |name| name.as_str())
            .as_bytes(),
        root.as_os_str().as_encoded_bytes(),
        path.as_os_str().as_encoded_bytes(),
    ] {
        digest.update(field.len().to_le_bytes());
        digest.update(field);
    }
    digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut value, byte| {
            let _ = write!(value, "{byte:02x}");
            value
        })
}

fn validate_overlays(
    overlays: &[FormOverlay],
    dependencies: &[FormDependency],
) -> Result<(), FormContextError> {
    for (index, overlay) in overlays.iter().enumerate() {
        if !dependencies.iter().any(|dependency| {
            dependency.id == overlay.source && dependency.role != FormDependencyRole::Picture
        }) || overlays[..index]
            .iter()
            .any(|before| before.source == overlay.source)
        {
            return Err(FormContextError::InvalidOverlay);
        }
    }

    Ok(())
}
