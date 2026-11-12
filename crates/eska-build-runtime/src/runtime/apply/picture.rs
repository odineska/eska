//! Recognize the native Designer XML bundle of a common SVG picture.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use eska_project_core::project::ProjectType;

use super::{Catalog, ObjectChange, ObjectKind, ObjectOperation, descriptor, structure};

pub(super) const COLLECTION: &str = "CommonPictures";
const PACKAGE: &str = "Ext/Picture.xml";
const SVG: &str = "Ext/Picture/Picture.svg";
const EXTERNAL: &str = "http://v8.1c.ru/8.3/xcf/extrnprops";
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";

pub(super) fn owner(path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(COLLECTION).ok()?;
    let first = relative.components().next()?.as_os_str().to_str()?;
    if relative.components().count() == 1 {
        descriptor::valid_name(first.strip_suffix(".xml")?).then(|| path.to_owned())
    } else {
        let suffix = relative.strip_prefix(first).ok()?;
        (descriptor::valid_name(first)
            && (suffix == Path::new(PACKAGE) || suffix == Path::new(SVG)))
        .then(|| Path::new(COLLECTION).join(first).with_extension("xml"))
    }
}

pub(super) fn matches_bundle(root: &Path, files: &[PathBuf]) -> bool {
    files.len() == 3 && files.contains(&root.join(PACKAGE)) && files.contains(&root.join(SVG))
}

pub(super) fn import_resources(
    source: &Path,
    project_type: ProjectType,
    catalog: &Catalog,
    owner: &Path,
) -> Option<[PathBuf; 2]> {
    let root = owner.with_extension("");
    let files = [root.join(PACKAGE), root.join(SVG)];
    // A resource omitted from the staged import can disappear even when ibcmd
    // reports success. Include both resources even for a descriptor-only edit.
    (catalog.contains_key(owner)
        && files.iter().all(|file| catalog.contains_key(file))
        && catalog
            .range(root.clone()..)
            .take_while(|(path, _)| path.starts_with(&root))
            .count()
            == 2
        && valid_package(source, owner)
        && (project_type != ProjectType::Extension
            || structure::own_extension_picture(source, owner)))
    .then_some(files)
}

pub(super) fn valid_package(source: &Path, owner: &Path) -> bool {
    let Some(xml) = descriptor::read_xml(&source.join(owner.with_extension("")).join(PACKAGE))
    else {
        return false;
    };
    let Some(document) = descriptor::parse_xml(xml.trim_start_matches('\u{feff}')) else {
        return false;
    };
    let root = document.root_element();
    if !root.has_tag_name((EXTERNAL, "ExtPicture")) {
        return false;
    }
    let mut pictures = root.children().filter(roxmltree::Node::is_element);
    let Some(picture) = pictures
        .next()
        .filter(|node| node.has_tag_name((EXTERNAL, "Picture")))
    else {
        return false;
    };
    let mut properties = picture.children().filter(roxmltree::Node::is_element);
    pictures.next().is_none()
        && properties.next().is_some_and(|node| {
            node.has_tag_name((READABLE, "Abs"))
                && node.children().count() == 1
                && node.text() == Some("Picture.svg")
        })
        && properties.next().is_some_and(|node| {
            node.has_tag_name((READABLE, "LoadTransparent"))
                && node.children().count() == 1
                && matches!(node.text(), Some("true" | "false"))
        })
        && properties.next().is_none()
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
