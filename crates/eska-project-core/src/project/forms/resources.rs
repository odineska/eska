//! Discover only metadata needed by static captions, appearance and bound field types.

use roxmltree::{Document, Node};

use super::{
    FormContextError, FormReference,
    context::{FormDependency, FormOverlay, source_identity},
};
use crate::{
    application::{ContextSnapshot, ContextSource, SourceRole},
    project::{
        designer_source::{self, DesignerSource},
        metadata_edit::snapshot,
        metadata_model::{MetadataKind, ObjectId, ProjectScope},
        metadata_parser::{self, ParsedDescriptor, PropertiesMode},
    },
};

#[derive(Debug)]
pub struct FormResource {
    pub source: String,
    pub descriptor: ParsedDescriptor,
    pub references: Vec<FormReference>,
    pub picture: Option<designer_source::PicturePreview>,
    pub retained_bytes: usize,
}

pub(super) fn load(
    context: &ContextSnapshot,
    owner: &ObjectId,
    selected: &ContextSource,
    documents: &[super::FormDocument],
    dependencies: &mut Vec<FormDependency>,
    overlays: &[FormOverlay],
) -> Result<Vec<FormResource>, FormContextError> {
    let external = selected.role() == SourceRole::ExternalObject;
    let (owners, requests) = requests(owner, documents)?;
    let mut output = Vec::new();
    let mut input_bytes: usize = documents
        .iter()
        .map(|document| document.input().len())
        .sum();
    for input in context.context.sources().iter().filter(|input| {
        input.role() != SourceRole::ExternalObject
            || input.project().source() == selected.project().source()
    }) {
        let scope = input.member().map_or(ProjectScope::Standalone, |member| {
            ProjectScope::Member(member.clone())
        });
        let (source, _) = designer_source::open_analysis_project(input.project().clone(), scope)
            .map_err(FormContextError::Source)?;
        // DefaultStyle is a configuration-level dependency even when no element overrides appearance.
        let mut pending = vec![source.root().id().clone()];
        if !external || input.role() == SourceRole::ExternalObject {
            pending.extend(owners.iter().cloned());
        }
        pending.extend(requests.iter().cloned());
        let mut read = std::collections::HashSet::new();
        let mut cursor = 0;
        while cursor < pending.len() {
            let id = pending[cursor].clone();
            cursor += 1;
            if !resource_belongs_to_source(&source, &id, input.role()) {
                continue;
            }
            let Some(location) = source
                .object_descriptor(&id)
                .map_err(FormContextError::Source)?
            else {
                continue;
            };
            if !read.insert(location.path.clone()) {
                continue;
            }
            if read.len() > 256 {
                return Err(FormContextError::TooManyResources);
            }
            let Some((dependency, text)) = read_input(
                context,
                &source,
                &location.path,
                overlays,
                input
                    .member()
                    .map(super::super::workspace::ProjectName::as_str),
                super::context::FormDependencyRole::Metadata,
            )?
            else {
                continue;
            };
            input_bytes = input_bytes.saturating_add(text.len());
            if input_bytes > super::MAX_FORM_CONTEXT_BYTES {
                return Err(FormContextError::InputTooLarge);
            }
            let xml = Document::parse(&text).map_err(|_| FormContextError::InvalidMetadata {
                source: dependency.id.clone(),
            })?;
            let descriptor = metadata_parser::parse(&text, id.parent(), PropertiesMode::All)
                .map_err(|_| FormContextError::InvalidMetadata {
                    source: dependency.id.clone(),
                })?;
            discover(xml.root_element(), &mut pending, false);
            let references = super::references::collect(xml.root_element());
            let (picture, picture_bytes) = load_picture(
                context,
                &source,
                &descriptor,
                &location.path,
                input
                    .member()
                    .map(super::super::workspace::ProjectName::as_str),
                overlays,
                dependencies,
            )?;
            input_bytes = input_bytes.saturating_add(picture_bytes);
            if input_bytes > super::MAX_FORM_CONTEXT_BYTES {
                return Err(FormContextError::InputTooLarge);
            }
            output.push(FormResource {
                source: dependency.id.clone(),
                descriptor,
                references,
                picture,
                retained_bytes: text.len().saturating_add(picture_bytes),
            });
            dependencies.push(dependency);
            if dependencies.len() > super::MAX_FORM_DEPENDENCIES {
                return Err(FormContextError::TooManyResources);
            }
        }
    }
    Ok(output)
}

/// External artifacts get configuration collections from the other application sources.
fn resource_belongs_to_source(source: &DesignerSource, id: &ObjectId, role: SourceRole) -> bool {
    role != SourceRole::ExternalObject
        || id == source.root().id()
        || id
            .as_str()
            .strip_prefix(source.root().id().as_str())
            .is_some_and(|tail| tail.starts_with('/'))
}

fn requests(
    owner: &ObjectId,
    documents: &[super::FormDocument],
) -> Result<(Vec<ObjectId>, Vec<ObjectId>), FormContextError> {
    let mut owners = vec![owner.clone()];
    let mut ancestor = owner.parent();
    while let Some(id) = ancestor {
        ancestor = id.parent();
        owners.push(id);
    }
    let mut requests = Vec::new();
    for document in documents {
        let parsed = Document::parse(document.input()).map_err(|_| FormContextError::Parse {
            source: document.source.clone(),
            error: super::FormParseError::InvalidXml,
        })?;
        discover(parsed.root_element(), &mut requests, true);
    }
    Ok((owners, requests))
}

fn load_picture(
    context: &ContextSnapshot,
    source: &DesignerSource,
    metadata: &ParsedDescriptor,
    descriptor: &std::path::Path,
    member: Option<&str>,
    overlays: &[FormOverlay],
    dependencies: &mut Vec<FormDependency>,
) -> Result<(Option<designer_source::PicturePreview>, usize), FormContextError> {
    let Some(object) = metadata
        .objects
        .iter()
        .find(|object| object.metadata.kind() == MetadataKind::CommonPicture)
    else {
        return Ok((None, 0));
    };
    let id = object.metadata.id();
    let path = descriptor.with_extension("").join("Ext/Picture.xml");
    let Some((dependency, text)) = read_input(
        context,
        source,
        &path,
        overlays,
        member,
        super::FormDependencyRole::Metadata,
    )?
    else {
        // Borrowed picture descriptors without a payload keep the preceding application's image.
        return Ok((None, 0));
    };
    if text.len() > 64 * 1024 {
        return Err(FormContextError::InputTooLarge);
    }
    Document::parse(&text).map_err(|_| FormContextError::InvalidMetadata {
        source: dependency.id.clone(),
    })?;
    dependencies.push(dependency);
    let preview = source.picture_preview_xml(id, &text);
    if let designer_source::PicturePreview::Ready(picture) = &preview {
        for input in &picture.inputs {
            let id = source_identity(context, source.project().source(), &input.path);
            if dependencies.iter().any(|dependency| dependency.id == id) {
                continue;
            }
            dependencies.push(FormDependency {
                id,
                root: source.project().source().into(),
                path: input.path.clone(),
                snapshot: input.snapshot.clone(),
                version: None,
                member: member.map(str::to_owned),
                role: super::FormDependencyRole::Picture,
            });
        }
    }
    let bytes = text.len().saturating_add(match &preview {
        designer_source::PicturePreview::Ready(picture) => picture.bytes.len(),
        designer_source::PicturePreview::Failed(_) => 0,
    });
    Ok((Some(preview), bytes))
}

pub(super) fn read_input(
    context: &ContextSnapshot,
    source: &DesignerSource,
    path: &std::path::Path,
    overlays: &[FormOverlay],
    member: Option<&str>,
    role: super::context::FormDependencyRole,
) -> Result<Option<(FormDependency, String)>, FormContextError> {
    // Containment/existence is checked even for unsaved buffers; arbitrary paths cannot become dependencies.
    if source
        .checked_file(path)
        .map_err(FormContextError::Source)?
        .is_none()
    {
        return Ok(None);
    }
    let id = source_identity(context, source.project().source(), path);
    let overlay = overlays.iter().find(|overlay| overlay.source == id);
    let text = match overlay {
        Some(overlay) => overlay.text.clone(),
        None => source
            .read_xml(path)
            .map_err(FormContextError::Source)?
            .ok_or(FormContextError::MissingForm)?,
    };
    if text.len() > super::MAX_FORM_BYTES {
        return Err(FormContextError::InputTooLarge);
    }
    Ok(Some((
        FormDependency {
            id,
            root: source.project().source().into(),
            path: path.into(),
            snapshot: snapshot(&text),
            version: overlay.map(|overlay| overlay.version.clone()),
            member: member.map(str::to_owned),
            role,
        },
        text,
    )))
}

fn request(kind: MetadataKind, name: &str, output: &mut Vec<ObjectId>) {
    if name.is_empty() || name.contains(['/', '\\', '\0']) {
        return;
    }
    let id = ObjectId::from_parts(None, kind.as_str(), name);
    if !output.contains(&id) {
        output.push(id);
    }
}

fn discover(root: Node<'_, '_>, output: &mut Vec<ObjectId>, include_types: bool) {
    for node in root.descendants().filter(Node::is_element) {
        let name = node.tag_name().name();
        let text = node.text().unwrap_or_default().trim();
        let text = text
            .split_once(':')
            .filter(|(prefix, _)| {
                node.lookup_namespace_uri(Some(prefix)) == Some(super::references::CONFIG)
            })
            .map_or(text, |(_, value)| value);
        let mut parts = text.split('.');
        let kind = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        if matches!(name, "DefaultStyle" | "Style") && kind == "Style" {
            request(MetadataKind::Style, value, output);
        }
        if name == "CommandName" && kind == "CommonCommand" {
            request(MetadataKind::CommonCommand, value, output);
        }
        if name == "Ref"
            && node.tag_name().namespace() == Some("http://v8.1c.ru/8.3/xcf/readable")
            && kind == "CommonPicture"
        {
            request(MetadataKind::CommonPicture, value, output);
        }
        if include_types
            && matches!(name, "Type" | "MainTable" | "Value")
            && !kind.starts_with("External")
            && let Some(kind) = type_kind(kind)
        {
            request(kind, value, output);
        }
    }
    for reference in super::references::collect(root) {
        if reference.target.namespace.as_deref() == Some(super::references::STYLE) {
            request(MetadataKind::StyleItem, &reference.target.name, output);
        }
        if reference.target.namespace.as_deref() == Some(super::references::CONFIG)
            && let Some(name) = reference.target.name.strip_prefix("StyleItem.")
        {
            request(MetadataKind::StyleItem, name, output);
        }
    }
}

pub(super) fn type_kind(name: &str) -> Option<MetadataKind> {
    match name {
        "Catalog" | "CatalogObject" | "CatalogRef" => Some(MetadataKind::Catalog),
        "Document" | "DocumentObject" | "DocumentRef" => Some(MetadataKind::Document),
        "Enum" | "EnumRef" => Some(MetadataKind::Enum),
        "DataProcessor" | "DataProcessorObject" | "ExternalDataProcessorObject" => {
            Some(MetadataKind::DataProcessor)
        }
        "Report" | "ReportObject" | "ExternalReportObject" => Some(MetadataKind::Report),
        "InformationRegister" | "InformationRegisterRecordSet" => {
            Some(MetadataKind::InformationRegister)
        }
        "AccumulationRegister" | "AccumulationRegisterRecordSet" => {
            Some(MetadataKind::AccumulationRegister)
        }
        "ChartOfCharacteristicTypes"
        | "ChartOfCharacteristicTypesRef"
        | "ChartOfCharacteristicTypesObject" => Some(MetadataKind::ChartOfCharacteristicTypes),
        "ChartOfAccounts" | "ChartOfAccountsRef" | "ChartOfAccountsObject" => {
            Some(MetadataKind::ChartOfAccounts)
        }
        "ChartOfCalculationTypes"
        | "ChartOfCalculationTypesRef"
        | "ChartOfCalculationTypesObject" => Some(MetadataKind::ChartOfCalculationTypes),
        "BusinessProcess" | "BusinessProcessObject" | "BusinessProcessRef" => {
            Some(MetadataKind::BusinessProcess)
        }
        "Task" | "TaskObject" | "TaskRef" => Some(MetadataKind::Task),
        "ExchangePlan" | "ExchangePlanObject" | "ExchangePlanRef" => {
            Some(MetadataKind::ExchangePlan)
        }
        _ => None,
    }
}
