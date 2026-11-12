//! Plan partial imports for native Designer XML common-picture bundles.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use eska_project_core::project::ProjectType;

use super::{
    Catalog, Change, ObjectChange, ObjectKind, ObjectOperation, checksum, descriptor, structure,
};

pub(super) const COLLECTION: &str = "CommonPictures";
const PACKAGE: &str = "Ext/Picture.xml";
const PAYLOAD_DIR: &str = "Ext/Picture";
const EXTERNAL: &str = "http://v8.1c.ru/8.3/xcf/extrnprops";
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
const METADATA: &str = "http://v8.1c.ru/8.3/MDClasses";

pub(super) fn owner(path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(COLLECTION).ok()?;
    let first = relative.components().next()?.as_os_str().to_str()?;
    if relative.components().count() == 1 {
        descriptor::valid_name(first.strip_suffix(".xml")?).then(|| path.to_owned())
    } else {
        let suffix = relative.strip_prefix(first).ok()?;
        (descriptor::valid_name(first) && (suffix == Path::new(PACKAGE) || is_payload(suffix)))
            .then(|| Path::new(COLLECTION).join(first).with_extension("xml"))
    }
}

fn supported_payload(name: &str) -> bool {
    matches!(
        name,
        "Picture"
            | "Picture.svg"
            | "Picture.png"
            | "Picture.bmp"
            | "Picture.gif"
            | "Picture.ico"
            | "Picture.jpg"
            | "Picture.tif"
            | "Picture.zip"
    )
}

fn is_payload(path: &Path) -> bool {
    path.parent() == Some(Path::new(PAYLOAD_DIR))
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(supported_payload)
}

pub(super) fn matches_bundle(root: &Path, files: &[PathBuf]) -> bool {
    files.len() == 3
        && files.contains(&root.join(PACKAGE))
        && files
            .iter()
            .any(|path| path.strip_prefix(root).is_ok_and(is_payload))
}

pub(super) fn resources(source: &Path, catalog: &Catalog, owner: &Path) -> Option<[PathBuf; 2]> {
    let root = owner.with_extension("");
    let package = root.join(PACKAGE);
    if !catalog.contains_key(&package) || resource_count(catalog, &root) != 2 {
        return None;
    }
    let payload = package_payload(source, &root, &package)?;
    catalog.contains_key(&payload).then_some([package, payload])
}

fn resource_count(catalog: &Catalog, root: &Path) -> usize {
    catalog
        .range(root.to_owned()..)
        .take_while(|(path, _)| path.starts_with(root))
        .count()
}

pub(super) fn import_resources(
    source: &Path,
    project_type: ProjectType,
    catalog: &Catalog,
    changes: &BTreeMap<PathBuf, Change>,
    owner: &Path,
) -> Option<[PathBuf; 2]> {
    // A resource omitted from the staged import can disappear even when ibcmd
    // reports success. Include both resources even for a descriptor-only edit.
    let files = resources(source, catalog, owner)?;
    (catalog.contains_key(owner)
        && can_replace(
            changes,
            &files,
            removed_payload_count(changes, &owner.with_extension("")),
            false,
        )
        && (project_type != ProjectType::Extension
            || structure::own_extension_picture(source, owner)))
    .then_some(files)
}

fn removed_payload_count(changes: &BTreeMap<PathBuf, Change>, root: &Path) -> usize {
    changes
        .range(root.to_owned()..)
        .take_while(|(path, _)| path.starts_with(root))
        .filter(|(path, change)| {
            **change == Change::Removed && path.strip_prefix(root).is_ok_and(is_payload)
        })
        .count()
}

fn can_replace(
    changes: &BTreeMap<PathBuf, Change>,
    [package, payload]: &[PathBuf; 2],
    removed_payloads: usize,
    allow_restore: bool,
) -> bool {
    match (changes.get(package), changes.get(payload), removed_payloads) {
        (None | Some(Change::Modified), None | Some(Change::Modified), 0)
        | (Some(Change::Modified), Some(Change::Added), 1) => true,
        (Some(Change::Added), Some(Change::Added), 0) => allow_restore,
        _ => false,
    }
}

pub(super) struct Adoption {
    pub fingerprint: String,
    extended: bool,
}

pub(super) fn read_adoption(source: &Path, owner: &Path) -> Option<Adoption> {
    let xml = descriptor::read_xml(&source.join(owner))?;
    let contents = xml.trim_start_matches('\u{feff}');
    let document = descriptor::parse_xml(contents)?;
    adoption(contents, document.root_element(), owner)
}

pub(super) fn adoption(xml: &str, root: roxmltree::Node<'_, '_>, path: &Path) -> Option<Adoption> {
    if path.parent()? != Path::new(COLLECTION) || !root.has_tag_name((METADATA, "MetaDataObject")) {
        return None;
    }
    let object = only_element(root, METADATA, "CommonPicture")?;
    if !descriptor::valid_uuid(object.attribute("uuid")?) {
        return None;
    }
    let mut parts = object.children().filter(roxmltree::Node::is_element);
    let internal = parts
        .next()
        .filter(|part| part.has_tag_name((METADATA, "InternalInfo")))?;
    let properties = parts
        .next()
        .filter(|part| part.has_tag_name((METADATA, "Properties")))?;
    if parts.next().is_some()
        || internal.attributes().len() != 0
        || !adopted_by_name(properties, path.file_stem()?.to_str()?)
    {
        return None;
    }
    let extended = if internal.children().any(|node| node.is_element()) {
        let state = only_element(internal, READABLE, "PropertyState")?;
        let mut fields = state.children().filter(roxmltree::Node::is_element);
        let property = fields.next()?;
        let value = fields.next()?;
        if state.attributes().len() != 0
            || !readable_literal(property, "Property", "Picture")
            || !readable_literal(value, "State", "Extended")
            || fields.next().is_some()
        {
            return None;
        }
        true
    } else {
        false
    };
    // Only the independently verified Picture property state may vary. The
    // remaining source, including the adopted object's UUID, must match.
    let range = internal.range();
    let bytes = xml.as_bytes();
    let fingerprint = checksum(&[
        &(range.start as u64).to_le_bytes(),
        &bytes[..range.start],
        &bytes[range.end..],
    ]);
    Some(Adoption {
        fingerprint,
        extended,
    })
}

fn readable_literal(node: roxmltree::Node<'_, '_>, tag: &str, value: &str) -> bool {
    node.has_tag_name((READABLE, tag))
        && node.attributes().len() == 0
        && node.children().count() == 1
        && node.text() == Some(value)
}

fn only_element<'a, 'i>(
    parent: roxmltree::Node<'a, 'i>,
    namespace: &str,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'i>> {
    let mut children = parent.children().filter(roxmltree::Node::is_element);
    let node = children
        .next()
        .filter(|node| node.has_tag_name((namespace, tag)))?;
    children.next().is_none().then_some(node)
}

fn adopted_by_name(properties: roxmltree::Node<'_, '_>, name: &str) -> bool {
    let mut seen = BTreeSet::new();
    for node in properties.children().filter(roxmltree::Node::is_element) {
        let tag = node.tag_name();
        if tag.namespace() != Some(METADATA) || !seen.insert(tag.name()) {
            return false;
        }
        match tag.name() {
            "ObjectBelonging" if node.text() == Some("Adopted") => {}
            "Name" if node.text() == Some(name) => {}
            "Synonym" | "Comment" => {}
            _ => return false,
        }
    }
    seen.contains("ObjectBelonging") && seen.contains("Name")
}

pub(super) fn import_adopted(
    source: &Path,
    catalog: &Catalog,
    changes: &BTreeMap<PathBuf, Change>,
    owner: &Path,
    previous: Option<&str>,
    adoption: &Adoption,
) -> Option<Vec<PathBuf>> {
    if !catalog.contains_key(owner) {
        return None;
    }
    check_name_mapping(source, owner)?;
    let descriptor_changed = match changes.get(owner) {
        None => false,
        Some(Change::Modified) if previous == Some(adoption.fingerprint.as_str()) => true,
        _ => return None,
    };
    let root = owner.with_extension("");
    let package = root.join(PACKAGE);
    let removed_payloads = removed_payload_count(changes, &root);
    if resource_count(catalog, &root) == 0 {
        let resource_delta = (changes.get(&package), removed_payloads);
        let removed = resource_delta == (Some(&Change::Removed), 1);
        return (removed && (adoption.extended || descriptor_changed)
            || descriptor_changed && resource_delta == (None, 0))
            .then(|| vec![owner.to_owned()]);
    }
    let resources = resources(source, catalog, owner)?;
    if !adoption.extended || !can_replace(changes, &resources, removed_payloads, true) {
        return None;
    }
    let mut files = Vec::with_capacity(3);
    if descriptor_changed {
        files.push(owner.to_owned());
    }
    files.extend(resources);
    Some(files)
}

fn check_name_mapping(source: &Path, owner: &Path) -> Option<()> {
    let xml = descriptor::read_xml(&source.join(structure::ROOT))?;
    let document = descriptor::parse_xml(xml.trim_start_matches('\u{feff}'))?;
    let configuration = only_element(document.root_element(), METADATA, "Configuration")?;
    let mut properties = configuration
        .children()
        .filter(|node| node.has_tag_name((METADATA, "Properties")));
    let first = properties.next()?;
    let mut mapping = first.children().filter(|node| {
        node.has_tag_name((METADATA, "KeepMappingToExtendedConfigurationObjectsByIDs"))
    });
    let by_id = mapping.next()?;
    if by_id.text() != Some("false") || mapping.next().is_some() || properties.next().is_some() {
        return None;
    }
    let mut children = configuration
        .children()
        .filter(|node| node.has_tag_name((METADATA, "ChildObjects")));
    let name = owner.file_stem()?.to_str()?;
    let mut declarations = children
        .next()?
        .children()
        .filter(|node| node.has_tag_name((METADATA, "CommonPicture")) && node.text() == Some(name));
    declarations.next()?;
    (children.next().is_none() && declarations.next().is_none()).then_some(())
}

fn package_payload(source: &Path, directory: &Path, package: &Path) -> Option<PathBuf> {
    let xml = descriptor::read_xml(&source.join(package))?;
    let document = descriptor::parse_xml(xml.trim_start_matches('\u{feff}'))?;
    let root = document.root_element();
    if !root.has_tag_name((EXTERNAL, "ExtPicture")) {
        return None;
    }
    let picture = only_element(root, EXTERNAL, "Picture")?;
    let mut properties = picture.children().filter(roxmltree::Node::is_element);
    let reference = properties.next()?;
    let payload = reference.text().filter(|name| supported_payload(name))?;
    let transparent = properties.next()?;
    (readable_literal(reference, "Abs", payload)
        && transparent.text().is_some_and(|value| {
            matches!(value, "true" | "false")
                && readable_literal(transparent, "LoadTransparent", value)
        })
        && properties.next().is_none())
    .then(|| directory.join(PAYLOAD_DIR).join(payload))
}

pub(super) fn reference_files(
    source: &Path,
    catalog: &Catalog,
    objects: &[ObjectChange],
) -> Vec<PathBuf> {
    let previous: BTreeSet<_> = objects
        .iter()
        .filter(|object| object.kind == ObjectKind::CommonPicture)
        .filter_map(|object| match object.operation {
            ObjectOperation::Added => None,
            ObjectOperation::Removed => Some(object.name.as_str()),
            ObjectOperation::Renamed => object.previous_name.as_deref(),
        })
        .map(str::to_lowercase)
        .collect();
    if previous.is_empty() {
        return Vec::new();
    }
    let matches = |text: &str| {
        text.trim().split_once('.').is_some_and(|(kind, name)| {
            kind.eq_ignore_ascii_case("CommonPicture") && previous.contains(&name.to_lowercase())
        })
    };
    // A native rename updates references by UUID, including in forms absent
    // from the import list. Refuse to record sources that retain the old name.
    catalog
        .keys()
        .filter(|path| descriptor::has_extension(path, "xml"))
        .filter(|path| {
            let Some(xml) = descriptor::read_xml(&source.join(path)) else {
                return true;
            };
            let prefix = b"CommonPicture.";
            if !xml.contains("&#")
                && !xml
                    .as_bytes()
                    .windows(prefix.len())
                    .any(|text| text.eq_ignore_ascii_case(prefix))
            {
                return false;
            }
            descriptor::parse_xml(xml.trim_start_matches('\u{feff}')).is_none_or(|document| {
                document.descendants().any(|node| {
                    node.is_text() && node.text().is_some_and(matches)
                        || node
                            .attributes()
                            .any(|attribute| matches(attribute.value()))
                })
            })
        })
        .cloned()
        .collect()
}
