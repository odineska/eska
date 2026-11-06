//! Discover only metadata needed by static captions, appearance and bound field types.

mod command_candidates;

use std::{
    borrow::Cow,
    collections::HashSet,
    path::{Path, PathBuf},
};

use roxmltree::{Document, Node};

use super::{
    ComposedForm, FormContextError, FormReference, based_on, command_types, compose,
    context::{FormDependency, FormOverlay, FormSources, open_designer, source_identity},
    layer_order::LayerOrder,
    presentation, references,
};
use crate::{
    application::{ContextSnapshot, SourceRole},
    project::{
        designer_source::{self, DesignerSource},
        metadata_edit::snapshot,
        metadata_model::{MetadataKind, MetadataValue, ObjectId, PropertyKey},
        metadata_parser::{
            self, MD_NAMESPACE, MetadataReference, ParsedDescriptor, PropertiesMode,
        },
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
    sources: &FormSources<'_>,
    documents: &mut [super::FormDocument],
    dependencies: &mut Vec<FormDependency>,
    overlays: &[FormOverlay],
    previous: &[FormDependency],
) -> Result<(ComposedForm, Vec<FormResource>), FormContextError> {
    let owners = std::iter::successors(Some(owner.clone()), ObjectId::parent).collect::<Vec<_>>();
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
    for request in documents.iter().flat_map(|document| &document.resource_ids) {
        loader.request(request.clone())?;
    }
    let mut readers = Vec::new();
    for input in sources.roots() {
        let source = open_designer(input)?;
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
        if sources.composes(input) {
            for owner in &owners {
                loader.read(&mut reader, owner)?;
            }
        }
        readers.push(reader);
    }
    // Resolve priority from effective root descriptors before asking which object this form edits.
    let order = LayerOrder::resolve(context, &loader.output)?;
    order.sort(documents, loader.dependencies, |document| &document.source)?;
    let (base, extensions) = documents
        .split_first()
        .ok_or(FormContextError::MissingForm)?;
    let form = compose(base, &extensions.iter().collect::<Vec<_>>());
    if super::commands::autofill(&form.root)
        && let Some(owner_type) = super::root_commands::object_type(&form)
    {
        order.sort(&mut readers, loader.dependencies, |reader| {
            &reader.layer_source
        })?;
        loader.discover_automatic_commands(&readers, &form, &owner_type, previous)?;
    }
    let mut cursor = 0;
    loop {
        while cursor < loader.pending.len() {
            let id = loader.pending[cursor].clone();
            cursor += 1;
            for reader in &mut readers {
                loader.read(reader, &id)?;
            }
        }
        order.sort(&mut loader.output, loader.dependencies, |resource| {
            &resource.source
        })?;
        for target in presentation::binding_resource_requests(&form, &loader.output) {
            loader.request(target)?;
        }
        if cursor == loader.pending.len() {
            break;
        }
    }
    Ok((form, loader.output))
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

type KnownPaths<'a> = HashSet<(&'a Path, &'a Path)>;
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";

pub(super) fn has_readable_reference(value: &MetadataValue, reference: &str) -> bool {
    let MetadataValue::Record(items) = value else {
        return false;
    };
    items.iter().any(|item| {
        item.key.namespace.as_deref() == Some(READABLE)
            && item.key.name == "Item"
            && super::appearance::text(&item.value).map(str::trim) == Some(reference)
    })
}

impl Loader<'_> {
    fn discover_automatic_commands(
        &mut self,
        readers: &[Reader<'_>],
        form: &ComposedForm,
        owner_type: &PropertyKey,
        previous: &[FormDependency],
    ) -> Result<(), FormContextError> {
        let parameter = command_types::reference_type(owner_type);
        let known_paths = previous
            .iter()
            .map(|input| (input.root.as_path(), input.path.as_path()))
            .collect::<HashSet<_>>();
        let mut targets =
            command_candidates::select(self, readers, form, &parameter, &known_paths)?;
        if let Some(basis) = based_on::basis(owner_type) {
            self.discover_reverse_relations(readers, &basis, &known_paths, &mut targets)?;
        }
        // Keep later overrides as well as the layer that first identified the candidate.
        for target in targets {
            self.request(target)?;
        }
        Ok(())
    }

    fn discover_reverse_relations(
        &self,
        readers: &[Reader<'_>],
        basis: &str,
        known_paths: &KnownPaths<'_>,
        targets: &mut Vec<ObjectId>,
    ) -> Result<(), FormContextError> {
        for reader in readers {
            let Some(root) = self
                .output
                .iter()
                .find(|resource| resource.source == reader.layer_source)
            else {
                continue;
            };
            for candidate in configuration_objects(&root.descriptor) {
                let (relation, end_tag, qualified) = match candidate.kind {
                    MetadataKind::DocumentJournal if basis.starts_with("Document.") => (
                        "RegisteredDocuments",
                        "</RegisteredDocuments",
                        ":RegisteredDocuments",
                    ),
                    kind if based_on::supports_creation(kind) => {
                        ("BasedOn", "</BasedOn", ":BasedOn")
                    }
                    _ => continue,
                };
                let Some((input, known)) = self.probe_input(reader, &candidate.id, known_paths)?
                else {
                    continue;
                };
                // Empty relation properties are normally exported as self-closing tags.
                if !known && !input.text.contains(end_tag) && !input.text.contains(qualified) {
                    continue;
                }
                match metadata_parser::parse_xml(&input.text, metadata_parser::MAX_DESCRIPTOR_NODES)
                {
                    Ok(document)
                        if direct_property(document.root_element(), candidate.kind, relation)
                            .is_some_and(|property| {
                                property.children().any(|item| {
                                    item.has_tag_name((READABLE, "Item"))
                                        && item.text().map(str::trim) == Some(basis)
                                })
                            }) =>
                    {
                        targets.push(candidate.id.clone());
                    }
                    Err(_) if known => {
                        return Err(FormContextError::InvalidMetadata { source: input.id });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn probe_input(
        &self,
        reader: &Reader<'_>,
        id: &ObjectId,
        known_paths: &KnownPaths<'_>,
    ) -> Result<Option<(InputText<'_>, bool)>, FormContextError> {
        let Some(location) = reader
            .source
            .object_descriptor(id)
            .map_err(FormContextError::Source)?
        else {
            return Ok(None);
        };
        let known =
            known_paths.contains(&(reader.source.project().source(), location.path.as_path()));
        match read_input_text(self.context, &reader.source, &location.path, self.overlays) {
            Ok(input) => Ok(input.map(|input| (input, known))),
            Err(error) if known => Err(error),
            Err(_) => Ok(None),
        }
    }

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
        let descriptor_id =
            location
                .descriptor_id(id)
                .ok_or_else(|| FormContextError::InvalidMetadata {
                    source: dependency.id.clone(),
                })?;
        let mut requests = Vec::new();
        let metadata = parse_metadata(&text, &descriptor_id, &dependency, &mut requests)?;
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
    let xml = metadata_parser::parse_xml(text, metadata_parser::MAX_DESCRIPTOR_NODES)
        .map_err(|_| invalid())?;
    let descriptor = metadata_parser::parse_descriptor(&xml, id.parent(), PropertiesMode::All)
        .map_err(|_| invalid())?;
    let root = xml.root_element();
    let references = references::collect(root);
    pending.extend(references::discover_resources(root, &references, false));
    pending.extend(
        descriptor
            .objects
            .iter()
            .flat_map(command_types::dependencies),
    );
    Ok(ResourceMetadata {
        descriptor,
        references,
        picture_extended: picture_extended(root),
    })
}

pub(super) fn configuration_objects(
    descriptor: &ParsedDescriptor,
) -> impl Iterator<Item = &MetadataReference> {
    let configuration = descriptor
        .objects
        .first()
        .filter(|object| object.metadata.kind() == MetadataKind::Configuration);
    descriptor.references.iter().filter(move |reference| {
        configuration.is_some_and(|root| &reference.owner == root.metadata.id())
    })
}

pub(super) fn direct_property<'a, 'input>(
    root: Node<'a, 'input>,
    kind: MetadataKind,
    property: &str,
) -> Option<Node<'a, 'input>> {
    descriptor_properties(root, kind)?
        .children()
        .find(|node| node.has_tag_name((MD_NAMESPACE, property)))
}

fn descriptor_properties<'a, 'input>(
    root: Node<'a, 'input>,
    kind: MetadataKind,
) -> Option<Node<'a, 'input>> {
    root.children()
        .find(|node| node.has_tag_name((MD_NAMESPACE, kind.designer_tag())))?
        .children()
        .find(|node| node.has_tag_name((MD_NAMESPACE, "Properties")))
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
    let document = metadata_parser::parse_xml(&text, metadata_parser::MAX_DESCRIPTOR_NODES)
        .map_err(|_| invalid())?;
    let style = super::style_data::parse(&dependency.id, &document).ok_or_else(invalid)?;
    let requests =
        references::discover_resources(document.root_element(), &style.references, false);
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
    path: &Path,
    overlays: &[FormOverlay],
    member: Option<&str>,
    role: super::context::FormDependencyRole,
) -> Result<Option<(FormDependency, String)>, FormContextError> {
    let Some(input) = read_input_text(context, source, path, overlays)? else {
        return Ok(None);
    };
    Ok(Some((
        FormDependency {
            id: input.id,
            root: source.project().source().into(),
            path: path.into(),
            snapshot: snapshot(&input.text),
            version: input.version.map(str::to_owned),
            member: member.map(str::to_owned),
            role,
            extension_form: false,
        },
        input.text.into_owned(),
    )))
}

struct InputText<'a> {
    id: String,
    text: Cow<'a, str>,
    version: Option<&'a str>,
}

fn read_input_text<'a>(
    context: &ContextSnapshot,
    source: &DesignerSource,
    path: &Path,
    overlays: &'a [FormOverlay],
) -> Result<Option<InputText<'a>>, FormContextError> {
    let id = source_identity(context, source.project().source(), path);
    let overlay = overlays.iter().find(|overlay| overlay.source == id);
    let text = if let Some(overlay) = overlay {
        // An unsaved buffer cannot bypass Designer source containment.
        if source
            .checked_file(path)
            .map_err(FormContextError::Source)?
            .is_none()
        {
            return Ok(None);
        }
        Cow::Borrowed(overlay.text.as_str())
    } else {
        let Some(text) = source.read_xml(path).map_err(FormContextError::Source)? else {
            return Ok(None);
        };
        Cow::Owned(text)
    };
    if text.len() > super::MAX_FORM_BYTES {
        return Err(FormContextError::InputTooLarge);
    }
    Ok(Some(InputText {
        id,
        text,
        version: overlay.map(|overlay| overlay.version.as_str()),
    }))
}
