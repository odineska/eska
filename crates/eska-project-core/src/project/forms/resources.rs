//! Discover only metadata needed by static captions, appearance and bound field types.

use std::{collections::HashSet, path::PathBuf};

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
        metadata_model::{MetadataKind, MetadataValue, ObjectId, ProjectScope},
        metadata_parser::{self, ParsedDescriptor, PropertiesMode},
    },
};

#[derive(Debug)]
pub struct FormResource {
    pub source: String,
    /// Identity of the owning project root descriptor, shared by this layer's resources.
    pub layer_source: String,
    pub descriptor: ParsedDescriptor,
    pub references: Vec<FormReference>,
    pub picture: Option<designer_source::PicturePreview>,
    pub style: Option<super::FormStyleData>,
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
    let mut loader = Loader {
        context,
        dependencies,
        overlays,
        output: Vec::new(),
        input_bytes: documents
            .iter()
            .map(|document| document.input().len())
            .sum(),
        pending: Vec::new(),
        requested: HashSet::new(),
    };
    for request in requests {
        loader.request(request)?;
    }
    let mut readers = Vec::new();
    for input in context.context.sources().iter().filter(|input| {
        input.role() != SourceRole::ExternalObject
            || input.project().source() == selected.project().source()
    }) {
        let scope = input.member().map_or(ProjectScope::Standalone, |member| {
            ProjectScope::Member(member.clone())
        });
        let (source, _) = designer_source::open_analysis_project(input.project().clone(), scope)
            .map_err(FormContextError::Source)?;
        let root = source.root().id().clone();
        let mut reader = Reader {
            source,
            member: input
                .member()
                .map(super::super::workspace::ProjectName::as_str),
            role: input.role(),
            read: HashSet::new(),
            layer_source: source_identity(context, input.project().source(), input.descriptor()),
        };
        // Roots stay local; their referenced styles/types must reach every applicable layer.
        loader.read(&mut reader, &root)?;
        if !external || input.role() == SourceRole::ExternalObject {
            for owner in &owners {
                loader.read(&mut reader, owner)?;
            }
        }
        readers.push(reader);
    }
    let mut cursor = 0;
    while cursor < loader.pending.len() {
        let id = loader.pending[cursor].clone();
        cursor += 1;
        for reader in &mut readers {
            loader.read(reader, &id)?;
        }
    }
    Ok(loader.output)
}

struct Reader<'a> {
    source: DesignerSource,
    member: Option<&'a str>,
    role: SourceRole,
    read: HashSet<PathBuf>,
    layer_source: String,
}

struct Loader<'a> {
    context: &'a ContextSnapshot,
    dependencies: &'a mut Vec<FormDependency>,
    overlays: &'a [FormOverlay],
    output: Vec<FormResource>,
    input_bytes: usize,
    pending: Vec<ObjectId>,
    requested: HashSet<ObjectId>,
}

impl Loader<'_> {
    fn request(&mut self, id: ObjectId) -> Result<(), FormContextError> {
        if self.requested.insert(id.clone()) {
            if self.requested.len() > super::MAX_FORM_DEPENDENCIES {
                return Err(FormContextError::TooManyResources);
            }
            self.pending.push(id);
        }
        Ok(())
    }

    fn read(&mut self, reader: &mut Reader<'_>, id: &ObjectId) -> Result<(), FormContextError> {
        if !resource_belongs_to_source(&reader.source, id, reader.role) {
            return Ok(());
        }
        let Some(location) = reader
            .source
            .object_descriptor(id)
            .map_err(FormContextError::Source)?
        else {
            return Ok(());
        };
        if !reader.read.insert(location.path.clone()) {
            return Ok(());
        }
        if reader.read.len() > 256 {
            return Err(FormContextError::TooManyResources);
        }
        let Some((dependency, text)) = read_input(
            self.context,
            &reader.source,
            &location.path,
            self.overlays,
            reader.member,
            super::context::FormDependencyRole::Metadata,
        )?
        else {
            return Ok(());
        };
        self.input_bytes = self.input_bytes.saturating_add(text.len());
        if self.input_bytes > super::MAX_FORM_CONTEXT_BYTES {
            return Err(FormContextError::InputTooLarge);
        }
        let mut requests = Vec::new();
        let metadata = parse_metadata(&text, id, &dependency, &mut requests)?;
        let mut payload = load_payload(
            self.context,
            &reader.source,
            &metadata.descriptor,
            &location.path,
            reader.member,
            self.overlays,
            self.dependencies,
        )?;
        // A borrowed picture may explicitly clear its value without an Ext payload.
        if payload.picture.is_none() && metadata.picture_extended {
            payload.picture = Some(designer_source::PicturePreview::Empty { inputs: Vec::new() });
        }
        requests.extend(payload.requests);
        self.input_bytes = self.input_bytes.saturating_add(payload.bytes);
        if self.input_bytes > super::MAX_FORM_CONTEXT_BYTES {
            return Err(FormContextError::InputTooLarge);
        }
        self.output.push(FormResource {
            source: dependency.id.clone(),
            layer_source: reader.layer_source.clone(),
            descriptor: metadata.descriptor,
            references: metadata.references,
            picture: payload.picture,
            style: payload.style,
            retained_bytes: text.len().saturating_add(payload.bytes),
        });
        self.dependencies.push(dependency);
        if self.dependencies.len() > super::MAX_FORM_DEPENDENCIES {
            return Err(FormContextError::TooManyResources);
        }
        for id in requests {
            self.request(id)?;
        }
        Ok(())
    }
}

struct ResourceMetadata {
    descriptor: ParsedDescriptor,
    references: Vec<FormReference>,
    picture_extended: bool,
}

fn parse_metadata(
    text: &str,
    id: &ObjectId,
    dependency: &FormDependency,
    pending: &mut Vec<ObjectId>,
) -> Result<ResourceMetadata, FormContextError> {
    let invalid = || FormContextError::InvalidMetadata {
        source: dependency.id.clone(),
    };
    let xml = Document::parse(text).map_err(|_| invalid())?;
    let descriptor =
        metadata_parser::parse(text, id.parent(), PropertiesMode::All).map_err(|_| invalid())?;
    discover(xml.root_element(), pending, false);
    Ok(ResourceMetadata {
        descriptor,
        references: super::references::collect(xml.root_element()),
        picture_extended: picture_extended(xml.root_element()),
    })
}

fn picture_extended(root: Node<'_, '_>) -> bool {
    const METADATA: &str = "http://v8.1c.ru/8.3/MDClasses";
    const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
    root.children()
        .filter(|node| node.has_tag_name((METADATA, "CommonPicture")))
        .flat_map(|node| node.children())
        .filter(|node| node.has_tag_name((METADATA, "InternalInfo")))
        .flat_map(|node| node.children())
        .filter(|node| node.has_tag_name((READABLE, "PropertyState")))
        .any(|state| {
            [("Property", "Picture"), ("State", "Extended")]
                .iter()
                .all(|(key, value)| {
                    state.children().any(|node| {
                        node.has_tag_name((READABLE, *key))
                            && node.text().map(str::trim) == Some(*value)
                    })
                })
        })
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

struct ResourcePayload {
    picture: Option<designer_source::PicturePreview>,
    style: Option<super::FormStyleData>,
    requests: Vec<ObjectId>,
    bytes: usize,
}

fn load_payload(
    context: &ContextSnapshot,
    source: &DesignerSource,
    metadata: &ParsedDescriptor,
    descriptor: &std::path::Path,
    member: Option<&str>,
    overlays: &[FormOverlay],
    dependencies: &mut Vec<FormDependency>,
) -> Result<ResourcePayload, FormContextError> {
    let (style, style_bytes, requests) = load_style(
        context,
        source,
        metadata,
        descriptor,
        member,
        overlays,
        dependencies,
    )?;
    let (picture, picture_bytes) = load_picture(
        context,
        source,
        metadata,
        descriptor,
        member,
        overlays,
        dependencies,
    )?;
    Ok(ResourcePayload {
        picture,
        style,
        requests,
        bytes: style_bytes.saturating_add(picture_bytes),
    })
}

fn load_style(
    context: &ContextSnapshot,
    source: &DesignerSource,
    metadata: &ParsedDescriptor,
    descriptor: &std::path::Path,
    member: Option<&str>,
    overlays: &[FormOverlay],
    dependencies: &mut Vec<FormDependency>,
) -> Result<(Option<super::FormStyleData>, usize, Vec<ObjectId>), FormContextError> {
    if !metadata
        .objects
        .iter()
        .any(|object| object.metadata.kind() == MetadataKind::Style)
    {
        return Ok((None, 0, Vec::new()));
    }
    let path = descriptor.with_extension("").join("Ext/Style.xml");
    let Some((dependency, text)) = read_input(
        context,
        source,
        &path,
        overlays,
        member,
        super::FormDependencyRole::Metadata,
    )?
    else {
        // Native export omits the payload when a style has no overrides.
        return Ok((None, 0, Vec::new()));
    };
    let invalid = || FormContextError::InvalidMetadata {
        source: dependency.id.clone(),
    };
    let document = Document::parse(&text).map_err(|_| invalid())?;
    let style = super::style_data::parse(&dependency.id, &document).ok_or_else(invalid)?;
    let mut requests = Vec::new();
    discover(document.root_element(), &mut requests, false);
    dependencies.push(dependency);
    Ok((Some(style), text.len(), requests))
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
        let borrowed = object.properties.as_ref().is_some_and(|properties| {
            properties.iter().any(|property| {
                property.property.key.namespace.as_deref() == Some(metadata_parser::MD_NAMESPACE)
                    && property.property.key.name == "ObjectBelonging"
                    && matches!(&property.property.value, MetadataValue::Text(value) if value == "Adopted")
            })
        });
        // Own empty pictures are valid. Only borrowed absence can inherit a preceding value;
        // the caller separately handles Extended without a payload as an explicit clear.
        return Ok((
            (!borrowed).then_some(designer_source::PicturePreview::Empty { inputs: Vec::new() }),
            0,
        ));
    };
    if text.len() > 64 * 1024 {
        return Err(FormContextError::InputTooLarge);
    }
    Document::parse(&text).map_err(|_| FormContextError::InvalidMetadata {
        source: dependency.id.clone(),
    })?;
    dependencies.push(dependency);
    let preview = source.picture_preview_xml(id, &text);
    let inputs = match &preview {
        designer_source::PicturePreview::Ready(picture) => picture.inputs.as_slice(),
        designer_source::PicturePreview::Empty { inputs } => inputs.as_slice(),
        designer_source::PicturePreview::Failed(_) => &[],
    };
    for input in inputs {
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
            extension_form: false,
        });
    }
    let bytes = text.len().saturating_add(match &preview {
        designer_source::PicturePreview::Ready(picture) => picture.bytes.len(),
        designer_source::PicturePreview::Empty { .. }
        | designer_source::PicturePreview::Failed(_) => 0,
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
            extension_form: false,
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
