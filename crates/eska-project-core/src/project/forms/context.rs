//! Reuse the application's ordered source set and checked Designer ownership; never scan the workspace.

use std::{
    borrow::Cow,
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
    pub form_help_available: bool,
    pub(super) form_owner: Option<LogicalLocation>,
    pub dependencies: Vec<FormDependency>,
    pub documents: Vec<FormDocument>,
    pub resources: Vec<super::FormResource>,
    pub form: ComposedForm,
    pub presentation: super::FormPresentation,
    pub expanded_data: Vec<Vec<String>>,
}

/// Inputs to one atomic form snapshot, including the data branches requested by its inspector.
#[derive(Default)]
pub struct FormPreviewInput<'a> {
    pub overlays: &'a [FormOverlay],
    pub previous: &'a [FormDependency],
    pub documents: Vec<FormInput>,
    pub expanded_data: &'a [Vec<String>],
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
    InvalidDataPath,
    TooManyResources,
    InputTooLarge,
    InvalidMetadata {
        source: String,
    },
}

/// Validate bounded semantic data paths before resolving any metadata.
///
/// # Errors
/// Rejects malformed paths and requests exceeding the inspector's limits.
pub fn validate_expanded_data(paths: &[Vec<String>]) -> Result<(), FormContextError> {
    if paths.len() > 128 {
        return Err(FormContextError::TooManyResources);
    }
    if paths.iter().any(|path| {
        path.is_empty()
            || path.len() > 64
            || path
                .iter()
                .any(|part| part.is_empty() || part.contains('.'))
            || path.iter().map(String::len).sum::<usize>() > 2048
    }) {
        return Err(FormContextError::InvalidDataPath);
    }
    Ok(())
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
    input: FormPreviewInput<'_>,
) -> Result<FormPreview, FormContextError> {
    let FormPreviewInput {
        overlays,
        previous,
        documents: inputs,
        expanded_data,
    } = input;
    validate_expanded_data(expanded_data)?;
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
    let mut form_help_available = false;
    for input in sources.roots().filter(|input| sources.composes(input)) {
        let source = open_designer(input)?;
        let Some((document, dependency)) =
            load_document(context, input, &source, owner, overlays, &mut input_bytes)?
        else {
            continue;
        };
        selected_found |= input.project().source() == selected;
        let help_paths = form_help_paths(&dependency.path);
        let form_index = dependencies.len();
        dependencies.push(dependency);
        for help_path in help_paths {
            if let Some((dependency, available, bytes)) = load_form_help(
                context,
                &source,
                &dependencies[form_index],
                help_path,
                overlays,
            )? {
                input_bytes = input_bytes.saturating_add(bytes);
                if input_bytes > super::MAX_FORM_CONTEXT_BYTES {
                    return Err(FormContextError::InputTooLarge);
                }
                form_help_available |= available;
                dependencies.push(dependency);
            }
        }
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
        &FormPreviewInput {
            overlays,
            previous,
            expanded_data,
            ..FormPreviewInput::default()
        },
    )?;
    validate_overlays(overlays, &dependencies, previous, &metadata)?;
    super::borrowing::validate(context, &dependencies, &resources)?;
    let mut parts = designer_source::identity_parts(owner).map_err(FormContextError::Source)?;
    let form_part = parts.pop();
    let form_owner = parts.pop().or(form_part);
    let mut preview = build_preview(
        context,
        form_owner,
        dependencies,
        documents,
        resources,
        form,
        form_help_available,
    );
    preview.expanded_data = expanded_data.to_vec();
    Ok(preview)
}

/// Read one layer, accounting for its bytes before parsing or retaining it.
fn load_document(
    context: &ContextSnapshot,
    input: &ContextSource,
    source: &DesignerSource,
    owner: &ObjectId,
    overlays: &[FormOverlay],
    input_bytes: &mut usize,
) -> Result<Option<(FormDocument, FormDependency)>, FormContextError> {
    let Some(path) = managed_payload(source, owner)? else {
        return Ok(None);
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
    *input_bytes = input_bytes.saturating_add(text.len());
    if *input_bytes > super::MAX_FORM_CONTEXT_BYTES {
        return Err(FormContextError::InputTooLarge);
    }
    let document = parse(&id, text).map_err(|error| FormContextError::Parse {
        source: id.clone(),
        error,
    })?;
    let dependency = FormDependency {
        id,
        root: input.project().source().into(),
        path,
        snapshot: document.revision.clone(),
        version: overlay.map(|overlay| overlay.version.clone()),
        member: input.member().map(|member| member.as_str().into()),
        role: FormDependencyRole::Form,
        extension_form: input.role() == SourceRole::Extension,
    };
    Ok(Some((document, dependency)))
}

fn form_help_paths(path: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    // An object form is exported under the owning object's Forms directory.
    let owner_help = path
        .ancestors()
        .nth(3)
        .filter(|directory| directory.file_name().is_some_and(|name| name == "Forms"))
        .and_then(Path::parent)
        .map(|owner| owner.join("Ext").join("Help.xml"));
    std::iter::once(path.with_file_name("Help.xml")).chain(owner_help)
}

/// The command surface needs page availability, without loading or displaying help HTML.
fn load_form_help(
    context: &ContextSnapshot,
    source: &DesignerSource,
    form: &FormDependency,
    path: PathBuf,
    overlays: &[FormOverlay],
) -> Result<Option<(FormDependency, bool, usize)>, FormContextError> {
    const HELP_NAMESPACE: &str = "http://v8.1c.ru/8.3/xcf/extrnprops";
    let id = source_identity(context, &form.root, &path);
    let overlay = overlays.iter().find(|overlay| overlay.source == id);
    let input = if let Some(overlay) = overlay {
        if source
            .checked_file(&path)
            .map_err(FormContextError::Source)?
            .is_none()
        {
            return Ok(None);
        }
        Cow::Borrowed(overlay.text.as_str())
    } else {
        let Some(input) = source.read_xml(&path).map_err(FormContextError::Source)? else {
            return Ok(None);
        };
        Cow::Owned(input)
    };
    let document = roxmltree::Document::parse(&input)
        .map_err(|_| FormContextError::InvalidMetadata { source: id.clone() })?;
    let root = document.root_element();
    if !root.has_tag_name((HELP_NAMESPACE, "Help")) {
        return Err(FormContextError::InvalidMetadata { source: id });
    }
    let available = root.children().any(|page| {
        page.has_tag_name((HELP_NAMESPACE, "Page"))
            && page.text().is_some_and(|text| !text.trim().is_empty())
    });
    let dependency = FormDependency {
        id,
        root: form.root.clone(),
        path,
        snapshot: super::super::metadata_edit::snapshot(&input),
        version: overlay.map(|overlay| overlay.version.clone()),
        member: form.member.clone(),
        role: FormDependencyRole::Metadata,
        extension_form: false,
    };
    Ok(Some((dependency, available, input.len())))
}

fn build_preview(
    context: &ContextSnapshot,
    form_owner: Option<LogicalLocation>,
    dependencies: Vec<FormDependency>,
    documents: Vec<FormDocument>,
    resources: Vec<super::FormResource>,
    form: ComposedForm,
    form_help_available: bool,
) -> FormPreview {
    let presentation = super::present(&form, &documents, &resources);
    FormPreview {
        expanded_data: Vec::new(),
        context_fingerprint: context.fingerprint.clone(),
        application: context.context.name().map(|name| name.as_str().into()),
        limited_context: context.context.limited_context(),
        form_help_available,
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
    fn help_paths_follow_the_exported_form_and_object() {
        for (form, expected) in [
            (
                "CommonForms/Parameters/Ext/Form.xml",
                vec!["CommonForms/Parameters/Ext/Help.xml"],
            ),
            (
                "Documents/Order/Forms/Edit/Ext/Form.xml",
                vec![
                    "Documents/Order/Forms/Edit/Ext/Help.xml",
                    "Documents/Order/Ext/Help.xml",
                ],
            ),
            (
                "Forms/Edit/Ext/Form.xml",
                vec!["Forms/Edit/Ext/Help.xml", "Ext/Help.xml"],
            ),
        ] {
            assert_eq!(
                form_help_paths(Path::new(form)).collect::<Vec<_>>(),
                expected.iter().map(PathBuf::from).collect::<Vec<_>>()
            );
        }
    }

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
