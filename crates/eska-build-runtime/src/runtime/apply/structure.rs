//! Plan top-level Designer XML changes that have a verified native partial import.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use eska_project_core::project::{ProjectType, object_model};
use serde::{Deserialize, Serialize};

use super::{Catalog, Change, checksum, descriptor};

const NAMESPACE: &str = "http://v8.1c.ru/8.3/MDClasses";
pub(super) const ROOT: &str = "Configuration.xml";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Language,
    CommonForm,
}

impl ObjectKind {
    const fn collection(self) -> &'static str {
        match self {
            Self::Language => "Languages",
            Self::CommonForm => "CommonForms",
        }
    }

    const fn identity(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::CommonForm => "common-form",
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
    default_language: String,
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
        valid_uuid(&uuid).then_some(uuid)
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
            ObjectKind::Language if files.len() == 1 => Some(files),
            ObjectKind::CommonForm
                if files.contains(&root.join("Ext/Form.xml"))
                    && (files.len() == 2
                        || files.len() == 3
                            && files.contains(&root.join("Ext/Form/Module.bsl"))) =>
            {
                Some(files)
            }
            _ => None,
        }
    }
}

impl Root {
    pub(super) fn read(source: &Path) -> Option<Self> {
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
        let default_language =
            only_child(only_child(configuration, "Properties")?, "DefaultLanguage")?
                .text()?
                .strip_prefix("Language.")?
                .to_owned();
        let mut other = vec![
            &bytes[..range.start + open.len()],
            &bytes[range.end - close.len()..],
        ];
        let mut entries = BTreeSet::new();
        let mut order = Vec::new();
        let mut names = BTreeSet::new();
        for node in children.children() {
            let kind = match (node.tag_name().namespace(), node.tag_name().name()) {
                (Some(NAMESPACE), "Language") => Some(ObjectKind::Language),
                (Some(NAMESPACE), "CommonForm") => Some(ObjectKind::CommonForm),
                _ => None,
            };
            if let Some(kind) = kind {
                let name = node.text()?.to_owned();
                if node.attributes().len() != 0
                    || node.children().count() != 1
                    || !valid_name(&name)
                    || !names.insert((kind, name.to_lowercase()))
                {
                    return None;
                }
                let entry = Entry { kind, name };
                entries.insert(entry.clone());
                order.push(entry);
            } else if !node.is_text() || !node.text()?.trim().is_empty() {
                other.push(&bytes[node.range()]);
            }
        }
        let anchor = Entry {
            kind: ObjectKind::Language,
            name: default_language.clone(),
        };
        entries.contains(&anchor).then(|| Self {
            entries,
            order,
            default_language,
            other_xml: checksum(&other),
        })
    }
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

fn valid_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub(super) struct Selection {
    pub root: Root,
    pub files: Vec<PathBuf>,
    pub objects: Vec<ObjectChange>,
    pub removed: Vec<PathBuf>,
    pub shapes: descriptor::Shapes,
}

pub(super) fn select(
    source: &Path,
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
    let root = Root::read(source)?;
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
    let shapes = read_added_shapes(source, &paths)?;
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
        // A root descriptor alone is rejected by ibcmd. The unchanged default
        // language supplies a valid import anchor even when the last form is deleted.
        let anchor = Entry {
            kind: ObjectKind::Language,
            name: root.default_language.clone(),
        }
        .descriptor();
        if current.get(&anchor)? != old.get(&anchor)? {
            return None;
        }
        files.insert(anchor);
    }
    files.extend(descriptor::import_files(
        source,
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

fn read_added_shapes(source: &Path, paths: &[PathBuf]) -> Option<descriptor::Shapes> {
    if paths.is_empty() {
        return Some(descriptor::Shapes::new());
    }
    let affected =
        object_model::discover_affected_from_source(source, ProjectType::Configuration, paths)
            .ok()?;
    if !affected.issues().is_empty() {
        return None;
    }
    paths
        .iter()
        .map(|path| {
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
