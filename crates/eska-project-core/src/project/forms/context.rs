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
}

#[derive(Debug)]
pub struct FormPreview {
    pub context_fingerprint: String,
    pub application: Option<String>,
    pub limited_context: bool,
    pub dependencies: Vec<FormDependency>,
    pub documents: Vec<FormDocument>,
    pub form: ComposedForm,
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
        });
        documents.push(document);
    }
    if !selected_found {
        return Err(FormContextError::MissingForm);
    }
    for (index, overlay) in overlays.iter().enumerate() {
        if !dependencies
            .iter()
            .any(|dependency| dependency.id == overlay.source)
            || overlays[..index]
                .iter()
                .any(|before| before.source == overlay.source)
        {
            return Err(FormContextError::InvalidOverlay);
        }
    }
    let (base, extensions) = documents
        .split_first()
        .ok_or(FormContextError::MissingForm)?;
    let form = compose(base, &extensions.iter().collect::<Vec<_>>());
    Ok(FormPreview {
        context_fingerprint: context.fingerprint.clone(),
        application: context.context.name().map(|name| name.as_str().into()),
        limited_context: context.context.limited_context(),
        dependencies,
        documents,
        form,
    })
}

fn source_identity(context: &ContextSnapshot, root: &Path, path: &Path) -> String {
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
