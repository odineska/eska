//! Resolve input ownership before malformed on-disk XML can prevent dirty-buffer collection.

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{
    application::{ContextSnapshot, SourceRole},
    project::{
        designer_source,
        metadata_model::{ObjectId, ProjectScope},
    },
};

use super::{
    FormContextError, FormOverlay,
    context::{context_source, source_identity},
};

/// A dirty XML candidate is checked against the selected context before its text can be used.
#[derive(Debug)]
pub struct FormInput {
    pub root: PathBuf,
    pub path: PathBuf,
    pub text: String,
    pub version: String,
}

#[derive(Debug)]
pub struct FormInputSource {
    pub id: String,
    pub root: PathBuf,
    pub path: PathBuf,
}

/// Checked roots and composing form files, without parsing their payloads or resource descriptors.
#[derive(Debug)]
pub struct FormPreparation {
    pub context_fingerprint: String,
    pub application: Option<String>,
    pub limited_context: bool,
    pub roots: Vec<PathBuf>,
    pub forms: Vec<FormInputSource>,
}

/// Resolve only this form's input scope from an already selected application.
///
/// # Errors
/// Returns invalid membership, missing form payload, source-containment or resource-limit errors.
pub fn prepare(
    context: &ContextSnapshot,
    selected: &Path,
    owner: &ObjectId,
) -> Result<FormPreparation, FormContextError> {
    let selected_source = context_source(context, selected)?;
    let external = selected_source.role() == SourceRole::ExternalObject;
    let mut roots = Vec::new();
    let mut forms = Vec::new();
    let mut selected_found = false;
    for input in context.context.sources().iter().filter(|source| {
        source.role() != SourceRole::ExternalObject || source.project().source() == selected
    }) {
        roots.push(input.project().source().to_path_buf());
        if roots.len() > super::MAX_FORM_DEPENDENCIES {
            return Err(FormContextError::TooManyResources);
        }
        if external && input.project().source() != selected {
            continue;
        }
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
        selected_found |= input.project().source() == selected;
        forms.push(FormInputSource {
            id: source_identity(context, input.project().source(), &location.path),
            root: input.project().source().to_path_buf(),
            path: location.path,
        });
        if forms.len() > super::MAX_FORM_DEPENDENCIES {
            return Err(FormContextError::TooManyResources);
        }
    }
    if !selected_found {
        return Err(FormContextError::MissingForm);
    }
    Ok(FormPreparation {
        context_fingerprint: context.fingerprint.clone(),
        application: context.context.name().map(|name| name.as_str().into()),
        limited_context: context.context.limited_context(),
        roots,
        forms,
    })
}

pub(super) fn resolve_inputs(
    context: &ContextSnapshot,
    selected: &Path,
    owner: &ObjectId,
    inputs: Vec<FormInput>,
) -> Result<(Vec<FormOverlay>, HashSet<String>), FormContextError> {
    if inputs.len() > super::MAX_FORM_DEPENDENCIES {
        return Err(FormContextError::TooManyResources);
    }
    let external = context_source(context, selected)?.role() == SourceRole::ExternalObject;
    let mut groups: BTreeMap<PathBuf, Vec<FormInput>> = BTreeMap::new();
    let mut bytes = 0usize;
    for input in inputs {
        bytes = bytes.saturating_add(input.text.len());
        if input.text.len() > super::MAX_FORM_BYTES || bytes > super::MAX_FORM_CONTEXT_BYTES {
            return Err(FormContextError::InputTooLarge);
        }
        groups.entry(input.root.clone()).or_default().push(input);
    }
    let mut overlays = Vec::new();
    let mut metadata = HashSet::new();
    let mut seen = HashSet::new();
    for (root, inputs) in groups {
        let input_source = context
            .context
            .sources()
            .iter()
            .find(|source| {
                source.project().source() == root
                    && (source.role() != SourceRole::ExternalObject || root == selected)
            })
            .ok_or(FormContextError::InvalidOverlay)?;
        let scope = input_source
            .member()
            .map_or(ProjectScope::Standalone, |member| {
                ProjectScope::Member(member.clone())
            });
        let (source, _) =
            designer_source::open_analysis_project(input_source.project().clone(), scope)
                .map_err(FormContextError::Source)?;
        for input in inputs {
            let form =
                checked_input_is_form(&source, owner, &input, !external || root == selected)?;
            let id = source_identity(context, input_source.project().source(), &input.path);
            if !seen.insert(id.clone()) {
                return Err(FormContextError::InvalidOverlay);
            }
            if !form {
                metadata.insert(id.clone());
            }
            overlays.push(FormOverlay {
                source: id,
                text: input.text,
                version: input.version,
            });
        }
    }
    Ok((overlays, metadata))
}

fn checked_input_is_form(
    source: &designer_source::DesignerSource,
    owner: &ObjectId,
    input: &FormInput,
    form_allowed: bool,
) -> Result<bool, FormContextError> {
    if input.version.is_empty()
        || !input
            .path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
        || source
            .checked_file(&input.path)
            .map_err(FormContextError::Source)?
            .is_none()
    {
        return Err(FormContextError::InvalidOverlay);
    }
    let form = input
        .path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("Form.xml"));
    if form
        && (!form_allowed
            || !source
                .sources(owner)
                .map_err(FormContextError::Source)?
                .iter()
                .any(|location| {
                    location.role == designer_source::SourceRole::Payload
                        && location.path == input.path
                }))
    {
        return Err(FormContextError::InvalidOverlay);
    }
    Ok(form)
}
