//! Resolve input ownership before malformed on-disk XML can prevent dirty-buffer collection.

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{
    application::ContextSnapshot,
    project::{designer_source::DesignerSource, metadata_model::ObjectId},
};

use super::{
    FormContextError, FormOverlay,
    context::{FormSources, managed_payload, open_designer, source_identity},
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
    let sources = FormSources::new(context, selected)?;
    let mut roots = Vec::new();
    let mut forms = Vec::new();
    let mut selected_found = false;
    for input in sources.roots() {
        roots.push(input.project().source().to_path_buf());
        if roots.len() > super::MAX_FORM_DEPENDENCIES {
            return Err(FormContextError::TooManyResources);
        }
        if !sources.composes(input) {
            continue;
        }
        let source = open_designer(input)?;
        let Some(path) = managed_payload(&source, owner)? else {
            continue;
        };
        selected_found |= input.project().source() == selected;
        forms.push(FormInputSource {
            id: source_identity(context, input.project().source(), &path),
            root: input.project().source().to_path_buf(),
            path,
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
    sources: &FormSources<'_>,
    owner: &ObjectId,
    inputs: Vec<FormInput>,
) -> Result<(Vec<FormOverlay>, HashSet<String>), FormContextError> {
    if inputs.len() > super::MAX_FORM_DEPENDENCIES {
        return Err(FormContextError::TooManyResources);
    }
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
        let input_source = sources
            .roots()
            .find(|source| source.project().source() == root)
            .ok_or(FormContextError::InvalidOverlay)?;
        let source = open_designer(input_source)?;
        for input in inputs {
            let form =
                checked_input_is_form(&source, owner, &input, sources.composes(input_source))?;
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
    source: &DesignerSource,
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
    if form && (!form_allowed || managed_payload(source, owner)?.as_ref() != Some(&input.path)) {
        return Err(FormContextError::InvalidOverlay);
    }
    Ok(form)
}
