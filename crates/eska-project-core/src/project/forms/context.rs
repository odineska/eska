//! Reuse the application's ordered source set and checked Designer ownership; never scan the workspace.

use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{ComposedForm, FormDocument, FormInput, FormParseError, parse};
use crate::{
    application::{ContextSnapshot, ContextSource, SourceRole},
    project::{
        designer_source::{self, DesignerSource, LogicalLocation, SourceError},
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
    #[serde(skip)]
    pub(super) extension_form: bool,
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
    pub(super) form_owner: Option<LogicalLocation>,
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
/// Previously resolved metadata overlays may retire when an edited reference changes.
/// They are ignored only if unused; form overlays and unknown inputs remain strict.
///
/// # Errors
/// Rejects unknown members, missing managed forms, foreign/duplicate overlays and invalid XML.
pub fn load(
    context: &ContextSnapshot,
    selected: &Path,
    owner: &ObjectId,
    overlays: &[FormOverlay],
    previous: &[FormDependency],
    inputs: Vec<FormInput>,
) -> Result<FormPreview, FormContextError> {
    if !overlays.is_empty() && !inputs.is_empty() {
        return Err(FormContextError::InvalidOverlay);
    }
    let sources = FormSources::new(context, selected)?;
    let (prepared, metadata) =
        super::preparation::resolve_inputs(context, &sources, owner, inputs)?;
    let overlays = if prepared.is_empty() {
        overlays
    } else {
        &prepared
    };
    let mut dependencies = Vec::new();
    let mut documents = Vec::new();
    let mut selected_found = false;
    let mut input_bytes = 0usize;
    for input in sources.roots().filter(|input| sources.composes(input)) {
        let source = open_designer(input)?;
        let Some(path) = managed_payload(&source, owner)? else {
            continue;
        };
        let id = source_identity(context, input.project().source(), &path);
        let overlay = overlays.iter().find(|overlay| overlay.source == id);
        let text = match overlay {
            Some(overlay) => overlay.text.clone(),
            None => source
                .read_xml(&path)
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
            path,
            snapshot: document.revision.clone(),
            version: overlay.map(|overlay| overlay.version.clone()),
            member: input.member().map(|member| member.as_str().into()),
            role: FormDependencyRole::Form,
            extension_form: input.role() == SourceRole::Extension,
        });
        documents.push(document);
        if dependencies.len() > super::MAX_FORM_DEPENDENCIES {
            return Err(FormContextError::TooManyResources);
        }
    }
    if !selected_found {
        return Err(FormContextError::MissingForm);
    }
    let (form, resources) = super::resources::load(
        context,
        owner,
        &sources,
        &mut documents,
        &mut dependencies,
        overlays,
        previous,
    )?;
    validate_overlays(overlays, &dependencies, previous, &metadata)?;
    super::borrowing::validate(context, &dependencies, &resources)?;
    let mut parts = designer_source::identity_parts(owner).map_err(FormContextError::Source)?;
    let form_part = parts.pop();
    let form_owner = parts.pop().or(form_part);
    Ok(build_preview(
        context,
        form_owner,
        dependencies,
        documents,
        resources,
        form,
    ))
}

fn build_preview(
    context: &ContextSnapshot,
    form_owner: Option<LogicalLocation>,
    dependencies: Vec<FormDependency>,
    documents: Vec<FormDocument>,
    resources: Vec<super::FormResource>,
    form: ComposedForm,
) -> FormPreview {
    let presentation = super::present(&form, &documents, &resources);
    FormPreview {
        context_fingerprint: context.fingerprint.clone(),
        application: context.context.name().map(|name| name.as_str().into()),
        limited_context: context.context.limited_context(),
        form_owner,
        dependencies,
        documents,
        resources,
        form,
        presentation,
    }
}

pub(super) struct FormSources<'a> {
    sources: &'a [ContextSource],
    selected: &'a ContextSource,
}

impl<'a> FormSources<'a> {
    pub(super) fn new(
        context: &'a ContextSnapshot,
        selected: &Path,
    ) -> Result<Self, FormContextError> {
        let sources = context.context.sources();
        let selected = sources
            .iter()
            .find(|source| source.project().source() == selected)
            .ok_or(FormContextError::MemberMismatch)?;
        Ok(Self { sources, selected })
    }

    pub(super) fn roots(&self) -> impl Iterator<Item = &'a ContextSource> + '_ {
        self.sources.iter().filter(|source| {
            source.role() != SourceRole::ExternalObject
                || source.project().source() == self.selected.project().source()
        })
    }

    /// External forms borrow metadata from the application but compose only their own form XML.
    pub(super) fn composes(&self, source: &ContextSource) -> bool {
        if self.selected.role() == SourceRole::ExternalObject {
            source.project().source() == self.selected.project().source()
        } else {
            source.role() != SourceRole::ExternalObject
        }
    }
}

pub(super) fn open_designer(input: &ContextSource) -> Result<DesignerSource, FormContextError> {
    let scope = input.member().map_or(ProjectScope::Standalone, |member| {
        ProjectScope::Member(member.clone())
    });
    designer_source::open_analysis_project(input.project().clone(), scope)
        .map(|(source, _)| source)
        .map_err(FormContextError::Source)
}

pub(super) fn managed_payload(
    source: &DesignerSource,
    owner: &ObjectId,
) -> Result<Option<PathBuf>, FormContextError> {
    Ok(source
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
        .map(|location| location.path))
}

pub(super) fn source_identity(context: &ContextSnapshot, root: &Path, path: &Path) -> String {
    let mut digest = Sha256::new();
    // Native path separators can differ between a resolver and a supplied XML path.
    let path: PathBuf = path.components().collect();
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
    previous: &[FormDependency],
    prepared_metadata: &std::collections::HashSet<String>,
) -> Result<(), FormContextError> {
    for (index, overlay) in overlays.iter().enumerate() {
        let referenced = dependencies.iter().any(|dependency| {
            dependency.id == overlay.source && dependency.role != FormDependencyRole::Picture
        });
        let retired_metadata = previous.iter().any(|dependency| {
            dependency.id == overlay.source && dependency.role == FormDependencyRole::Metadata
        });
        if (!referenced && !retired_metadata && !prepared_metadata.contains(&overlay.source))
            || overlays[..index]
                .iter()
                .any(|before| before.source == overlay.source)
        {
            return Err(FormContextError::InvalidOverlay);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unique_previous_metadata_overlays_may_retire() {
        let overlay = FormOverlay {
            source: "known".into(),
            text: "<unsaved/>".into(),
            version: "1".into(),
        };
        let dependency = |role| FormDependency {
            id: "known".into(),
            root: PathBuf::new(),
            path: PathBuf::new(),
            snapshot: String::new(),
            version: None,
            member: None,
            role,
            extension_form: false,
        };
        for role in [
            FormDependencyRole::Form,
            FormDependencyRole::Metadata,
            FormDependencyRole::Picture,
        ] {
            assert_eq!(
                validate_overlays(
                    std::slice::from_ref(&overlay),
                    &[],
                    &[dependency(role)],
                    &std::collections::HashSet::new()
                )
                .is_ok(),
                role == FormDependencyRole::Metadata
            );
        }
        assert!(
            validate_overlays(
                std::slice::from_ref(&overlay),
                &[],
                &[],
                &std::collections::HashSet::new()
            )
            .is_err()
        );
        assert!(
            validate_overlays(
                &[overlay.clone(), overlay],
                &[],
                &[dependency(FormDependencyRole::Metadata)],
                &std::collections::HashSet::new()
            )
            .is_err()
        );
    }
}
