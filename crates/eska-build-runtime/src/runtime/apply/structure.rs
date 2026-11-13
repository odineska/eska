//! Plan top-level Designer XML changes that have a verified native partial import.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use eska_project_core::project::{ProjectType, object_model};
use serde::{Deserialize, Serialize};

use super::{Catalog, Change, checksum, descriptor, picture};

const NAMESPACE: &str = "http://v8.1c.ru/8.3/MDClasses";
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
pub(super) const ROOT: &str = "Configuration.xml";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Language,
    CommonForm,
    Role,
    CommonModule,
    CommonPicture,
}

impl ObjectKind {
    const fn collection(self) -> &'static str {
        match self {
            Self::Language => "Languages",
            Self::CommonForm => "CommonForms",
            Self::Role => "Roles",
            Self::CommonModule => "CommonModules",
            Self::CommonPicture => picture::COLLECTION,
        }
    }

    const fn identity(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::CommonForm => "common-form",
            Self::Role => "role",
            Self::CommonModule => "common-module",
            Self::CommonPicture => "common-picture",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectOperation {
    Added,
    Removed,
    Renamed,
}

#[derive(Debug, Serialize)]
pub struct ObjectChange {
    pub kind: ObjectKind,
    pub operation: ObjectOperation,
    pub name: String,
    pub previous_name: Option<String>,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Root {
    entries: BTreeSet<Entry>,
    order: Vec<Entry>,
    default_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_role: Option<String>,
    #[serde(default)]
    common_modules: bool,
    #[serde(default)]
    common_pictures: bool,
    other_xml: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Entry {
    kind: ObjectKind,
    name: String,
}

impl Entry {
    fn directory(&self) -> PathBuf {
        Path::new(self.kind.collection()).join(&self.name)
    }

    fn descriptor(&self) -> PathBuf {
        self.directory().with_extension("xml")
    }

    fn uuid(&self, shapes: &descriptor::Shapes) -> Option<String> {
        let uuid = shapes.get(&self.descriptor())?.uuid(&format!(
            "{}:{}",
            self.kind.identity(),
            self.name
        ))?;
        descriptor::valid_uuid(&uuid).then_some(uuid)
    }

    fn files(&self, catalog: &Catalog) -> Option<Vec<PathBuf>> {
        let path = self.descriptor();
        catalog.get(&path)?;
        let root = self.directory();
        let mut files = vec![path];
        files.extend(
            catalog
                .range(root.clone()..)
                .take_while(|(candidate, _)| candidate.starts_with(&root))
                .map(|(candidate, _)| candidate.clone()),
        );
        match self.kind {
            ObjectKind::Language | ObjectKind::Role if files.len() == 1 => Some(files),
            ObjectKind::CommonModule
                if files.len() == 2 && files.contains(&root.join("Ext/Module.bsl")) =>
            {
                Some(files)
            }
            ObjectKind::CommonForm
                if files.contains(&root.join("Ext/Form.xml"))
                    && (files.len() == 2
                        || files.len() == 3
                            && files.contains(&root.join("Ext/Form/Module.bsl"))) =>
            {
                Some(files)
            }
            ObjectKind::CommonPicture if picture::matches_bundle(&root, &files) => Some(files),
            _ => None,
        }
    }
}

impl Root {
    pub(super) fn read(source: &Path, project_type: ProjectType) -> Option<Self> {
        Self::read_with_layout(source, project_type, true, true)
    }

    fn read_with_layout(
        source: &Path,
        project_type: ProjectType,
        common_modules: bool,
        common_pictures: bool,
    ) -> Option<Self> {
        let xml = descriptor::read_xml(&source.join(ROOT))?;
        let xml = xml.trim_start_matches('\u{feff}');
        let bytes = xml.as_bytes();
        let document = descriptor::parse_xml(xml)?;
        let root = document.root_element();
        let configuration = only_child(root, "Configuration")?;
        if !root.has_tag_name((NAMESPACE, "MetaDataObject")) {
            return None;
        }
        let children = only_child(configuration, "ChildObjects")?;
        let range = children.range();
        let (open, close) = (b"<ChildObjects>".as_slice(), b"</ChildObjects>".as_slice());
        if !bytes[range.clone()].starts_with(open) || !bytes[range.clone()].ends_with(close) {
            return None;
        }
        let properties = only_child(configuration, "Properties")?;
        let (default_language, default_role) = match project_type {
            ProjectType::Configuration => (
                Some(
                    only_child(properties, "DefaultLanguage")?
                        .text()?
                        .strip_prefix("Language.")?
                        .to_owned(),
                ),
                None,
            ),
            ProjectType::Extension => (None, default_role(properties).map(str::to_owned)),
            _ => return None,
        };
        let mut other = vec![
            &bytes[..range.start + open.len()],
            &bytes[range.end - close.len()..],
        ];
        let mut entries = BTreeSet::new();
        let mut order = Vec::new();
        let mut names = BTreeSet::new();
        for node in children.children() {
            let kind = match (
                project_type,
                node.tag_name().namespace(),
                node.tag_name().name(),
            ) {
                (ProjectType::Configuration, Some(NAMESPACE), "Language") => {
                    Some(ObjectKind::Language)
                }
                (ProjectType::Configuration, Some(NAMESPACE), "CommonForm") => {
                    Some(ObjectKind::CommonForm)
                }
                (ProjectType::Extension, Some(NAMESPACE), "Role") => Some(ObjectKind::Role),
                (_, Some(NAMESPACE), "CommonModule") if common_modules => {
                    Some(ObjectKind::CommonModule)
                }
                (_, Some(NAMESPACE), "CommonPicture") if common_pictures => {
                    Some(ObjectKind::CommonPicture)
                }
                _ => None,
            };
            if let Some(kind) = kind {
                let name = node.text()?.to_owned();
                if node.attributes().len() != 0
                    || node.children().count() != 1
                    || !descriptor::valid_name(&name)
                    || !names.insert((kind, name.to_lowercase()))
                {
                    return None;
                }
                let entry = Entry { kind, name };
                if kind == ObjectKind::Role && !own_extension_object(source, &entry) {
                    return None;
                }
                if project_type == ProjectType::Extension
                    && matches!(kind, ObjectKind::CommonModule | ObjectKind::CommonPicture)
                    && !own_extension_object(source, &entry)
                {
                    other.push(&bytes[node.range()]);
                    continue;
                }
                entries.insert(entry.clone());
                order.push(entry);
            } else if !node.is_text() || !node.text()?.trim().is_empty() {
                other.push(&bytes[node.range()]);
            }
        }
        let recorded = Self {
            entries,
            order,
            default_language,
            default_role,
            common_modules,
            common_pictures,
            other_xml: checksum(&other),
        };
        if let Some(anchor) = recorded.anchor()
            && !recorded.entries.contains(&anchor)
        {
            return None;
        }
        Some(recorded)
    }

    fn anchor(&self) -> Option<Entry> {
        self.default_language
            .as_ref()
            .map(|name| (ObjectKind::Language, name))
            .or_else(|| {
                self.default_role
                    .as_ref()
                    .map(|name| (ObjectKind::Role, name))
            })
            .map(|(kind, name)| Entry {
                kind,
                name: name.clone(),
            })
    }

    fn read_for_delta(
        &self,
        source: &Path,
        project_type: ProjectType,
        old: &Catalog,
    ) -> Option<Self> {
        // Older baselines hashed these declarations as unrelated XML. Preserve
        // that proof unless there were no descriptors of that kind.
        let contains = |kind: ObjectKind| {
            old.keys().any(|path| {
                path.parent() == Some(Path::new(kind.collection()))
                    && descriptor::has_extension(path, "xml")
            })
        };
        Self::read_with_layout(
            source,
            project_type,
            self.common_modules || !contains(ObjectKind::CommonModule),
            self.common_pictures || !contains(ObjectKind::CommonPicture),
        )
    }
}

fn default_role<'a>(properties: roxmltree::Node<'a, '_>) -> Option<&'a str> {
    let roles = only_child(properties, "DefaultRoles")?;
    let mut items = roles.children().filter(roxmltree::Node::is_element);
    let item = items.next()?;
    if items.next().is_some() || !item.has_tag_name((READABLE, "Item")) {
        return None;
    }
    item.text()?
        .strip_prefix("Role.")
        .filter(|name| descriptor::valid_name(name))
}

fn own_extension_object(source: &Path, entry: &Entry) -> bool {
    let tag = match entry.kind {
        ObjectKind::Role => "Role",
        ObjectKind::CommonModule => "CommonModule",
        ObjectKind::CommonPicture => "CommonPicture",
        _ => return false,
    };
    let Some(xml) = descriptor::read_xml(&source.join(entry.descriptor())) else {
        return false;
    };
    let Some(document) = descriptor::parse_xml(xml.trim_start_matches('\u{feff}')) else {
        return false;
    };
    let Some(object) = only_child(document.root_element(), tag) else {
        return false;
    };
    let Some(properties) = only_child(object, "Properties") else {
        return false;
    };
    // Owned objects have no adoption or mapping properties.
    object
        .children()
        .filter(roxmltree::Node::is_element)
        .count()
        == 1
        && properties
            .children()
            .filter(roxmltree::Node::is_element)
            .all(|node| {
                node.tag_name().namespace() == Some(NAMESPACE)
                    && own_property(entry.kind, node.tag_name().name())
            })
        && only_child(properties, "Name").and_then(|node| node.text()) == Some(entry.name.as_str())
}

pub(super) fn own_extension_picture(source: &Path, descriptor: &Path) -> bool {
    descriptor
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            own_extension_object(
                source,
                &Entry {
                    kind: ObjectKind::CommonPicture,
                    name: name.to_owned(),
                },
            )
        })
}

fn own_property(kind: ObjectKind, name: &str) -> bool {
    matches!(name, "Name" | "Synonym" | "Comment")
        || kind == ObjectKind::CommonModule
            && matches!(
                name,
                "Global"
                    | "ClientManagedApplication"
                    | "Server"
                    | "ExternalConnection"
                    | "ClientOrdinaryApplication"
                    | "ServerCall"
                    | "Privileged"
                    | "ReturnValuesReuse"
            )
        || kind == ObjectKind::CommonPicture
            && matches!(name, "AvailabilityForChoice" | "AvailabilityForAppearance")
}

fn only_child<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'i>> {
    let mut matching = node
        .children()
        .filter(|child| child.has_tag_name((NAMESPACE, name)));
    let first = matching.next()?;
    matching.next().is_none().then_some(first)
}

pub(super) struct Selection {
    root: Root,
    pub files: Vec<PathBuf>,
    pub objects: Vec<ObjectChange>,
    removed: Vec<PathBuf>,
    shapes: descriptor::Shapes,
}

impl Selection {
    pub(super) fn update_baseline(
        self,
        root: &mut Option<Root>,
        descriptors: &mut descriptor::Shapes,
    ) -> Vec<ObjectChange> {
        *root = Some(self.root);
        for path in self.removed {
            descriptors.remove(&path);
        }
        descriptors.extend(self.shapes);
        self.objects
    }
}

pub(super) fn select(
    source: &Path,
    project_type: ProjectType,
    changes: &BTreeMap<PathBuf, Change>,
    old: &Catalog,
    current: &Catalog,
    previous: &Root,
    descriptors: &descriptor::Shapes,
) -> Option<Selection> {
    let mut remaining = changes.clone();
    if remaining.remove(Path::new(ROOT)) != Some(Change::Modified) {
        return None;
    }
    let root = previous.read_for_delta(source, project_type, old)?;
    if root.other_xml != previous.other_xml
        || root.entries == previous.entries
        || previous
            .order
            .iter()
            .filter(|entry| root.entries.contains(*entry))
            .ne(root
                .order
                .iter()
                .filter(|entry| previous.entries.contains(*entry)))
    {
        return None;
    }

    let paths = root
        .entries
        .difference(&previous.entries)
        .map(Entry::descriptor)
        .collect::<Vec<_>>();
    let shapes = read_added_shapes(source, project_type, current, &paths)?;
    let old_ids = identities(&previous.entries, descriptors, &descriptor::Shapes::new())?;
    let new_ids = identities(&root.entries, descriptors, &shapes)?;
    let mut files = BTreeSet::from([PathBuf::from(ROOT)]);
    let mut objects = Vec::new();
    let mut removed = Vec::new();

    for (identity, entry) in &old_ids {
        let next = new_ids.get(identity).copied();
        if next == Some(*entry) {
            continue;
        }
        let mut paths = consume(&mut remaining, entry, old, Change::Removed)?;
        removed.push(entry.descriptor());
        if let Some(next) = next {
            let destination = consume(&mut remaining, next, current, Change::Added)?;
            files.extend(destination.iter().cloned());
            paths.extend(destination);
            objects.push(ObjectChange {
                kind: entry.kind,
                operation: ObjectOperation::Renamed,
                name: next.name.clone(),
                previous_name: Some(entry.name.clone()),
                paths,
            });
        } else {
            objects.push(ObjectChange {
                kind: entry.kind,
                operation: ObjectOperation::Removed,
                name: entry.name.clone(),
                previous_name: None,
                paths,
            });
        }
    }
    for (_, entry) in new_ids.iter().filter(|(id, _)| !old_ids.contains_key(*id)) {
        let paths = consume(&mut remaining, entry, current, Change::Added)?;
        files.extend(paths.iter().cloned());
        objects.push(ObjectChange {
            kind: entry.kind,
            operation: ObjectOperation::Added,
            name: entry.name.clone(),
            previous_name: None,
            paths,
        });
    }
    if objects
        .iter()
        .any(|change| change.operation == ObjectOperation::Removed)
    {
        // The unchanged default object supplies an import anchor for a root-only deletion.
        let anchor = root.anchor()?;
        if anchor.kind == ObjectKind::Role {
            anchor.files(current)?;
        }
        let anchor = anchor.descriptor();
        if current.get(&anchor)? != old.get(&anchor)? {
            return None;
        }
        files.insert(anchor);
    }
    files.extend(descriptor::import_files(
        source,
        project_type,
        &remaining,
        current,
        descriptors,
    )?);
    let mut files: Vec<_> = files.into_iter().collect();
    files.sort_by_key(|path| path.components().count());
    Some(Selection {
        root,
        files,
        objects,
        removed,
        shapes,
    })
}

fn read_added_shapes(
    source: &Path,
    project_type: ProjectType,
    catalog: &Catalog,
    paths: &[PathBuf],
) -> Option<descriptor::Shapes> {
    if paths.is_empty() {
        return Some(descriptor::Shapes::new());
    }
    let affected = object_model::discover_affected_from_source(source, project_type, paths).ok()?;
    if !affected.issues().is_empty() {
        return None;
    }
    paths
        .iter()
        .map(|path| {
            if path.starts_with(picture::COLLECTION) {
                picture::resources(source, catalog, path)?;
            }
            descriptor::shape(source, affected.model(), path).map(|shape| (path.clone(), shape))
        })
        .collect()
}

fn identities<'a>(
    entries: &'a BTreeSet<Entry>,
    previous: &descriptor::Shapes,
    added: &descriptor::Shapes,
) -> Option<BTreeMap<(ObjectKind, String), &'a Entry>> {
    let mut identities = BTreeMap::new();
    for entry in entries {
        let uuid = entry.uuid(added).or_else(|| entry.uuid(previous))?;
        if identities.insert((entry.kind, uuid), entry).is_some() {
            return None;
        }
    }
    Some(identities)
}

fn consume(
    remaining: &mut BTreeMap<PathBuf, Change>,
    entry: &Entry,
    catalog: &Catalog,
    change: Change,
) -> Option<Vec<PathBuf>> {
    let paths = entry.files(catalog)?;
    paths
        .iter()
        .all(|path| remaining.remove(path) == Some(change))
        .then_some(paths)
}
