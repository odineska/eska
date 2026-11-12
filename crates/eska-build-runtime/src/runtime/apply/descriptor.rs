//! Prove that a partial import addresses the same objects as the applied baseline.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read as _,
    path::{Path, PathBuf},
};

use eska_project_core::project::{
    ProjectType,
    object_model::{self, ObjectId, ObjectModel},
};
use serde::{Deserialize, Serialize};

use super::{Catalog, Change, checksum, picture, structure};

const ROOT: &str = "Configuration.xml";
const MD_NAMESPACE: &str = "http://v8.1c.ru/8.3/MDClasses";
const MAX_DESCRIPTOR_BYTES: u64 = 64 * 1024 * 1024;

pub(super) type Shapes = BTreeMap<PathBuf, Shape>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Shape {
    identities: BTreeMap<ObjectId, String>,
    structure: String,
}

impl Shape {
    pub(super) fn uuid(&self, identity: &str) -> Option<String> {
        let (id, uuid) = self.identities.first_key_value()?;
        (self.identities.len() == 1 && id.as_str() == identity).then(|| uuid.to_ascii_lowercase())
    }
}

pub(super) fn baseline(source: &Path, project_type: ProjectType, catalog: &Catalog) -> Shapes {
    let paths: Vec<_> = catalog
        .keys()
        .filter(|path| match project_type {
            ProjectType::Extension => {
                simple_role(path, catalog)
                    || path.parent() == Some(Path::new("CommonModules"))
                        && descriptor_path(path, project_type)
                    || path.parent() == Some(Path::new(picture::COLLECTION))
                        && structure::own_extension_picture(source, path)
            }
            _ => descriptor_path(path, project_type),
        })
        .cloned()
        .collect();
    if paths.is_empty() {
        return Shapes::new();
    }
    let Ok(affected) = object_model::discover_affected_from_source(source, project_type, &paths)
    else {
        return Shapes::new();
    };
    paths
        .into_iter()
        .filter_map(|path| shape(source, affected.model(), &path).map(|shape| (path, shape)))
        .collect()
}

pub(super) fn import_files(
    source: &Path,
    project_type: ProjectType,
    changes: &BTreeMap<PathBuf, Change>,
    catalog: &Catalog,
    baseline: &Shapes,
) -> Option<Vec<PathBuf>> {
    let mut files: BTreeSet<_> = changes.keys().cloned().collect();
    let mut pictures = BTreeSet::new();
    for (path, change) in changes {
        if *change != Change::Modified {
            return None;
        }
        if path.starts_with(picture::COLLECTION) {
            pictures.insert(picture::owner(path)?);
        } else if !has_extension(path, "bsl")
            && !match project_type {
                ProjectType::Extension => simple_role(path, catalog),
                _ => has_extension(path, "xml"),
            }
        {
            return None;
        }
    }
    for owner in &pictures {
        files.extend(picture::import_resources(
            source,
            project_type,
            catalog,
            owner,
        )?);
    }
    let catalog_roots: Vec<_> = changes
        .keys()
        .filter(|path| {
            path.parent() == Some(Path::new("Catalogs")) && descriptor_path(path, project_type)
        })
        .map(|path| path.with_extension(""))
        .collect();
    for root in &catalog_roots {
        let descendants: Vec<_> = catalog
            .range(root.clone()..)
            .take_while(|(path, _)| path.starts_with(root))
            .map(|(path, _)| path.clone())
            .collect();
        if descendants.is_empty() {
            return None;
        }
        for path in &descendants {
            // This catalog layout is covered by native import. Other
            // companions need a proven import sequence for their own kinds.
            if !catalog_form_file(root, path, catalog) {
                return None;
            }
        }
        files.extend(descendants);
    }
    let xml: Vec<_> = files
        .iter()
        .filter(|path| has_extension(path, "xml") && !path.starts_with(picture::COLLECTION))
        .cloned()
        .chain(pictures)
        .collect();
    if !xml.is_empty() {
        let affected =
            object_model::discover_affected_from_source(source, project_type, &xml).ok()?;
        if !affected.issues().is_empty()
            || !xml
                .iter()
                .all(|path| same_descriptor(source, project_type, affected.model(), path, baseline))
        {
            return None;
        }
    }
    let mut files: Vec<_> = files.into_iter().collect();
    // ibcmd must read parent and form descriptors before their layout resources.
    if !xml.is_empty() {
        files.sort_by_key(|path| path.components().count());
    }
    Some(files)
}

// The platform probe covered roles whose only file was the descriptor.
fn simple_role(path: &Path, catalog: &Catalog) -> bool {
    if path.parent() != Some(Path::new("Roles")) || !has_extension(path, "xml") {
        return false;
    }
    let resources = path.with_extension("");
    !catalog
        .range(resources.clone()..)
        .next()
        .is_some_and(|(path, _)| path.starts_with(&resources))
}

fn catalog_form_file(root: &Path, path: &Path, catalog: &Catalog) -> bool {
    let forms = root.join("Forms");
    if path.parent() == Some(forms.as_path()) && descriptor_path(path, ProjectType::Configuration) {
        return catalog.contains_key(&path.with_extension("").join("Ext/Form.xml"));
    }
    path.parent().and_then(Path::parent).is_some_and(|owner| {
        owner.parent() == Some(forms.as_path())
            && path == owner.join("Ext/Form.xml")
            && catalog.contains_key(&owner.with_extension("xml"))
    })
}

fn same_descriptor(
    source: &Path,
    project_type: ProjectType,
    model: &ObjectModel,
    path: &Path,
    baseline: &Shapes,
) -> bool {
    let owner = if descriptor_path(path, project_type) {
        let supported = model.objects_for_changed_path(path).iter().any(|object| {
            object.descriptor_path() == path
                && match project_type {
                    ProjectType::Extension => {
                        matches!(object.metadata_type(), "role" | "common-picture")
                    }
                    _ => matches!(
                        object.metadata_type(),
                        "language" | "common-form" | "form" | "catalog" | "common-picture"
                    ),
                }
        });
        supported.then_some(path)
    } else {
        form_descriptor(model, path)
    };
    owner.is_some_and(|owner| {
        baseline
            .get(owner)
            .is_some_and(|previous| shape(source, model, owner).as_ref() == Some(previous))
    })
}

fn descriptor_path(path: &Path, project_type: ProjectType) -> bool {
    let Ok(bytes) = gix_path::into_bstr(path) else {
        return false;
    };
    path != Path::new(ROOT)
        && eska_project_core::project::metadata::is_object_descriptor(
            project_type,
            gix_path::to_unix_separators_on_windows(bytes).as_ref(),
        )
}

fn form_descriptor<'a>(model: &'a ObjectModel, path: &Path) -> Option<&'a Path> {
    let owner = path.parent()?.parent()?;
    if path != owner.join("Ext/Form.xml") {
        return None;
    }
    let descriptor = owner.with_extension("xml");
    model
        .objects_for_changed_path(path)
        .into_iter()
        .find(|object| {
            object.descriptor_path() == descriptor
                && matches!(object.metadata_type(), "common-form" | "form")
        })
        .map(object_model::LogicalObject::descriptor_path)
}

pub(super) fn shape(source: &Path, model: &ObjectModel, path: &Path) -> Option<Shape> {
    let objects = model.objects_for_changed_path(path);
    if !objects
        .iter()
        .any(|object| object.descriptor_path() == path)
    {
        return None;
    }
    let identities = objects
        .into_iter()
        .filter(|object| object.paths().any(|candidate| candidate == path))
        .map(|object| (object.id().clone(), object.uuid().to_owned()))
        .collect();

    let contents = read_xml(&source.join(path))?;
    let contents = contents.trim_start_matches('\u{feff}');
    let document = parse_xml(contents)?;
    let root = document.root_element();
    let object = root.first_element_child()?;
    let properties = object
        .children()
        .find(|node| node.has_tag_name((MD_NAMESPACE, "Properties")))?
        .range();
    // Only properties of the existing descriptor may change. Preserve child
    // declarations and generated type identifiers even when they lack UUIDs.
    let bytes = contents.as_bytes();
    let structure = checksum(&[
        &(properties.start as u64).to_le_bytes(),
        &bytes[..properties.start],
        &bytes[properties.end..],
    ]);
    Some(Shape {
        identities,
        structure,
    })
}

pub(super) fn read_xml(path: &Path) -> Option<String> {
    let mut contents = String::new();
    File::open(path)
        .ok()?
        .take(MAX_DESCRIPTOR_BYTES + 1)
        .read_to_string(&mut contents)
        .ok()?;
    (contents.len() as u64 <= MAX_DESCRIPTOR_BYTES).then_some(contents)
}

pub(super) fn parse_xml(contents: &str) -> Option<roxmltree::Document<'_>> {
    roxmltree::Document::parse_with_options(
        contents,
        roxmltree::ParsingOptions {
            nodes_limit: 1_000_000,
            ..Default::default()
        },
    )
    .ok()
}

pub(super) fn valid_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

pub(super) fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(super) fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}
