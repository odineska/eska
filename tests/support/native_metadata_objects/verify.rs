use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use eska_project_core::project::metadata_model::MetadataKind;
use roxmltree::{Document, Node};
use serde_json::Value;

use super::{MD, XR};

#[derive(Clone)]
pub struct Object {
    pub kind: MetadataKind,
    pub name: String,
    pub descriptor: PathBuf,
    pub files: Vec<PathBuf>,
    prefixes: BTreeSet<String>,
}

impl Object {
    pub fn id(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.name)
    }
    pub fn twin_name(&self) -> String {
        format!("NativeTwin{}", self.kind.designer_tag())
    }
    pub fn twin_descriptor(&self) -> PathBuf {
        self.descriptor
            .with_file_name(format!("{}.xml", self.twin_name()))
    }
    pub fn target(&self, file: &Path) -> PathBuf {
        if file == self.descriptor {
            return self.twin_descriptor();
        }
        self.twin_descriptor().with_extension("").join(
            file.strip_prefix(self.descriptor.with_extension(""))
                .unwrap(),
        )
    }
}

pub fn select(source: &Path) -> Vec<Object> {
    let xml = fs::read_to_string(source.join("Configuration.xml")).unwrap();
    let doc = Document::parse(&xml).unwrap();
    let children = doc
        .descendants()
        .find(|n| n.has_tag_name((MD, "ChildObjects")))
        .unwrap();
    let mut selected: BTreeMap<MetadataKind, (u64, Object)> = BTreeMap::new();
    for node in children.children().filter(Node::is_element) {
        let kind = MetadataKind::from_xml_tag(node.tag_name().name()).unwrap();
        let name = node.text().unwrap().to_owned();
        let descriptor = Path::new(kind.collection_folder().unwrap()).join(format!("{name}.xml"));
        let mut files = vec![descriptor.clone()];
        collect_files(source, &descriptor.with_extension(""), &mut files);
        let text = fs::read_to_string(source.join(&descriptor)).unwrap();
        let parsed = Document::parse(&text).unwrap();
        let nodes = parsed.descendants().count() as u64;
        let mut prefixes: BTreeSet<_> = parsed
            .descendants()
            .filter(|node| node.has_tag_name((XR, "GeneratedType")))
            .filter_map(|node| {
                node.attribute("name")
                    .and_then(|name| name.split_once('.'))
                    .map(|(prefix, _)| prefix.to_owned())
            })
            .collect();
        prefixes.insert(kind.designer_tag().to_owned());
        let score = files.iter().fold(nodes * 1000, |total, file| {
            total + fs::metadata(source.join(file)).unwrap().len()
        });
        let object = Object {
            kind,
            name,
            descriptor,
            files,
            prefixes,
        };
        if selected.get(&kind).is_none_or(|(old, _)| score > *old) {
            selected.insert(kind, (score, object));
        }
    }
    let filter = std::env::var("ESKA_NATIVE_METADATA_KINDS").ok();
    selected
        .into_values()
        .map(|(_, object)| object)
        .filter(|object| {
            filter
                .as_ref()
                .is_none_or(|filter| filter.split(',').any(|key| key == object.kind.as_str()))
        })
        .collect()
}

pub fn collect_files(source: &Path, path: &Path, result: &mut Vec<PathBuf>) {
    if !source.join(path).exists() {
        return;
    }
    for entry in fs::read_dir(source.join(path)).unwrap() {
        let entry = entry.unwrap();
        let child = path.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            collect_files(source, &child, result);
        } else {
            result.push(child);
        }
    }
    result.sort();
}

pub fn identities(bytes: &[u8]) -> BTreeSet<String> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return BTreeSet::new();
    };
    let Ok(xml) = Document::parse(text) else {
        return BTreeSet::new();
    };
    xml.descendants()
        .filter(Node::is_element)
        .filter_map(|node| {
            let namespace = node.tag_name().namespace();
            let value = match (namespace, node.tag_name().name()) {
                (Some(MD | "http://v8.1c.ru/8.3/xcf/scheme"), _) => node.attribute("uuid"),
                (Some("http://v8.1c.ru/8.3/xcf/predef"), "Item") => node.attribute("id"),
                (Some(XR), "ThisNode") => node.text(),
                (Some(XR), "TypeId" | "ValueId")
                    if node
                        .parent_element()
                        .is_some_and(|p| p.has_tag_name((XR, "GeneratedType"))) =>
                {
                    node.text()
                }
                _ => None,
            }?;
            (value != "00000000-0000-0000-0000-000000000000").then(|| value.to_ascii_lowercase())
        })
        .collect()
}

pub fn twins(source: &Path, object: &Object, plan: &Value, overrides: &BTreeMap<String, String>) {
    let map: BTreeMap<String, String> =
        serde_json::from_value(plan["result"]["prepared"]["identities"].clone()).unwrap();
    let owned: BTreeSet<_> = object
        .files
        .iter()
        .flat_map(|path| identities(&fs::read(source.join(path)).unwrap()))
        .collect();
    assert_eq!(
        owned,
        map.keys().cloned().collect(),
        "every owned identity must be allocated"
    );
    let fresh: BTreeSet<_> = map.values().cloned().collect();
    assert_eq!(fresh.len(), map.len(), "distinct generated identities");
    assert!(
        fresh.is_disjoint(&owned),
        "copy cannot retain source identities"
    );
    for id in &fresh {
        assert_eq!(id.as_bytes()[14], b'4', "UUID v4");
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    }
    for path in &object.files {
        let original = fs::read(source.join(path)).unwrap();
        let copied = fs::read(source.join(object.target(path))).unwrap();
        self_references(&original, &copied, object);
        let expected = normalize(&copied, &map, &object.twin_name(), &object.name);
        let expected = if path == &object.descriptor {
            revert_overrides(&original, expected, overrides)
        } else {
            expected
        };
        assert_eq!(
            original,
            expected,
            "unexplained content/BOM/EOL/reference change in {}",
            path.display()
        );
    }
}

fn self_references(original: &[u8], copied: &[u8], object: &Object) {
    let (Ok(original), Ok(copied)) = (std::str::from_utf8(original), std::str::from_utf8(copied))
    else {
        return;
    };
    let (Ok(original), Ok(copied)) = (Document::parse(original), Document::parse(copied)) else {
        return;
    };
    let before = original.descendants().filter(Node::is_element);
    let after = copied.descendants().filter(Node::is_element);
    for (old, new) in before.zip(after) {
        if old.has_tag_name((XR, "GeneratedType")) {
            let name = old.attribute("name").unwrap();
            assert_eq!(
                new.attribute("name"),
                Some(renamed_reference(name, object).as_str()),
                "generated type still names source owner"
            );
        }
        let typed = old.has_tag_name((XR, "Field"))
            || old.has_tag_name((XR, "Item"))
                && old.attributes().any(|attribute| {
                    attribute.name() == "type" && attribute.value().ends_with(":MDObjectRef")
                })
            || old.tag_name().namespace() == Some("http://v8.1c.ru/8.1/data/core")
                && matches!(old.tag_name().name(), "Type" | "TypeSet")
            || old.tag_name().namespace() == Some(MD)
                && (old.tag_name().name().starts_with("Default")
                    || old.tag_name().name().starts_with("Auxiliary"));
        if typed && let Some(value) = old.text() {
            let expected = renamed_reference(value, object);
            assert_eq!(
                new.text(),
                Some(expected.as_str()),
                "typed reference retained the source owner: {value}"
            );
        }
    }
}

fn renamed_reference(value: &str, object: &Object) -> String {
    let Some((prefix, tail)) = value.split_once('.') else {
        return value.to_owned();
    };
    if !object.prefixes.contains(prefix.rsplit(':').next().unwrap()) {
        return value.to_owned();
    }
    let Some(rest) = tail.strip_prefix(&object.name) else {
        return value.to_owned();
    };
    if !rest.is_empty() && !rest.starts_with('.') {
        return value.to_owned();
    }
    format!("{prefix}.{}{rest}", object.twin_name())
}

fn normalize(bytes: &[u8], map: &BTreeMap<String, String>, name: &str, original: &str) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut text = text.replace(name, original);
    for (old, new) in map {
        text = text.replace(new, old);
    }
    text.into_bytes()
}

fn revert_overrides(
    original: &[u8],
    copied: Vec<u8>,
    overrides: &BTreeMap<String, String>,
) -> Vec<u8> {
    if overrides.is_empty() {
        return copied;
    }
    let input = std::str::from_utf8(original).unwrap();
    let doc = Document::parse(input).unwrap();
    let properties = doc
        .descendants()
        .find(|n| n.has_tag_name((MD, "Properties")))
        .unwrap();
    let mut copied = String::from_utf8(copied).unwrap();
    for (key, value) in overrides {
        let old = properties
            .children()
            .find(|n| n.has_tag_name((MD, key.as_str())))
            .unwrap()
            .text()
            .unwrap_or_default();
        copied = copied.replace(&format!(">{value}<"), &format!(">{old}<"));
    }
    copied.into_bytes()
}

pub fn exported(source: &Path, export: &Path, object: &Object) {
    for path in &object.files {
        let target = object.target(path);
        let expected = fs::read(source.join(&target)).unwrap();
        let observed = fs::read(export.join(&target))
            .unwrap_or_else(|error| panic!("native export lost {}: {error}", target.display()));
        native_file(&expected, &observed, &target);
    }
}

pub fn native_file(expected: &[u8], observed: &[u8], path: &Path) {
    assert_eq!(
        identities(expected),
        identities(observed),
        "native changed/lost identities in {}",
        path.display()
    );
    assert_eq!(
        generated_types(expected),
        generated_types(observed),
        "native changed generated type ownership in {}",
        path.display()
    );
}

fn generated_types(bytes: &[u8]) -> BTreeSet<(String, String, String, String)> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return BTreeSet::new();
    };
    let Ok(document) = Document::parse(text) else {
        return BTreeSet::new();
    };
    document
        .descendants()
        .filter(|node| node.has_tag_name((XR, "GeneratedType")))
        .map(|node| {
            let value = |tag| {
                node.children()
                    .find(|child| child.has_tag_name((XR, tag)))
                    .and_then(|child| child.text())
                    .unwrap()
                    .to_owned()
            };
            (
                node.attribute("name").unwrap().to_owned(),
                node.attribute("category").unwrap().to_owned(),
                value("TypeId"),
                value("ValueId"),
            )
        })
        .collect()
}
