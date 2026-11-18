//! Apply typed paths to editable TOML without rebuilding the surrounding document.

use std::path::Path;

use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value as Json};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use crate::{
    config::{FILE_NAME, ManifestConfig},
    formatting::{self, Language, Layer, Patch},
    project::discovery::{self, ContextDiscoveryError, DiscoveryContext},
};

use super::Error;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    path: Vec<Segment>,
    #[serde(default, deserialize_with = "present_value")]
    value: Option<Json>,
}

fn present_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Json>, D::Error> {
    Json::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Segment {
    Key(String),
    Index(usize),
}

pub(super) fn json(text: &str) -> Result<Json, Error> {
    let document = parse(text)?;
    Ok(table_json(document.as_table()))
}

fn parse(text: &str) -> Result<DocumentMut, Error> {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .parse()
        .map_err(|_| Error::InvalidManifest)
}

pub(super) fn apply(source: &str, edits: Vec<Edit>) -> Result<String, Error> {
    let mut document = parse(source)?;
    let original = document.to_string();
    for edit in edits {
        let field = edit
            .path
            .iter()
            .map(|part| match part {
                Segment::Key(key) => key.clone(),
                Segment::Index(index) => index.to_string(),
            })
            .collect::<Vec<_>>()
            .join(".");
        if edit.path.len() > 64 {
            return Err(Error::InvalidField(field));
        }
        edit_table(document.as_table_mut(), &edit.path, edit.value)
            .ok_or(Error::InvalidField(field))?;
    }
    let rendered = document.to_string();
    if rendered == original {
        return Ok(source.to_owned());
    }
    // The editable parser drops an initial BOM. Retain it when the input had one.
    let mut result = if source.starts_with('\u{feff}') {
        String::from("\u{feff}")
    } else {
        String::new()
    };
    if source.contains("\r\n") && !source.replace("\r\n", "").contains('\n') {
        result.push_str(&rendered.replace("\r\n", "\n").replace('\n', "\r\n"));
    } else {
        result.push_str(&rendered);
    }
    if result.len() > super::MAX_BYTES {
        return Err(Error::TooLarge);
    }
    Ok(result)
}

fn edit_table(table: &mut dyn TableLike, path: &[Segment], value: Option<Json>) -> Option<()> {
    let (Segment::Key(key), rest) = path.split_first()? else {
        return None;
    };
    if rest.is_empty() && value.is_none() {
        table.remove(key);
        return Some(());
    }
    if let Some(item) = table.get_mut(key) {
        return edit_item(item, rest, value);
    }
    if value.is_none() {
        return Some(());
    }
    let mut item = container(rest.first());
    edit_item(&mut item, rest, value)?;
    table.insert(key, item);
    Some(())
}

fn edit_item(item: &mut Item, path: &[Segment], value: Option<Json>) -> Option<()> {
    if path.is_empty() {
        return replace_item(item, value?);
    }
    match item {
        Item::Table(table) => edit_table(table, path, value),
        Item::ArrayOfTables(array) => {
            edit_table_array(array, path, value)?;
            if array.is_empty() {
                *item = Item::Value(Value::Array(Array::new()));
            }
            Some(())
        }
        Item::Value(current) => edit_value(current, path, value),
        Item::None => None,
    }
}

fn edit_value(current: &mut Value, path: &[Segment], value: Option<Json>) -> Option<()> {
    if path.is_empty() {
        return replace_value(current, value?);
    }
    match current {
        Value::InlineTable(table) => edit_table(table, path, value),
        Value::Array(array) => edit_array(array, path, value),
        _ => None,
    }
}

fn edit_array(array: &mut Array, path: &[Segment], value: Option<Json>) -> Option<()> {
    let (Segment::Index(index), rest) = path.split_first()? else {
        return None;
    };
    if *index < array.len() && rest.is_empty() && value.is_none() {
        array.remove(*index);
    } else if let Some(current) = array.get_mut(*index) {
        edit_value(current, rest, value)?;
    } else if value.is_none() {
        return Some(());
    } else if *index == array.len() {
        let mut current = container(rest.first());
        edit_item(&mut current, rest, value)?;
        array.push(current.into_value().ok()?);
    } else {
        return None;
    }
    Some(())
}

fn edit_table_array(
    array: &mut ArrayOfTables,
    path: &[Segment],
    value: Option<Json>,
) -> Option<()> {
    let (Segment::Index(index), rest) = path.split_first()? else {
        return None;
    };
    if *index < array.len() && rest.is_empty() && value.is_none() {
        array.remove(*index);
    } else if let Some(table) = array.get_mut(*index) {
        edit_or_replace_table(table, rest, value)?;
    } else if value.is_none() {
        return Some(());
    } else if *index == array.len() {
        let mut table = Table::new();
        edit_or_replace_table(&mut table, rest, value)?;
        array.push(table);
    } else {
        return None;
    }
    Some(())
}

fn edit_or_replace_table(table: &mut Table, path: &[Segment], value: Option<Json>) -> Option<()> {
    if path.is_empty() {
        let Json::Object(values) = value? else {
            return None;
        };
        replace_table(table, values)
    } else {
        edit_table(table, path, value)
    }
}

fn container(next: Option<&Segment>) -> Item {
    match next {
        Some(Segment::Key(_)) => Item::Table(Table::new()),
        Some(Segment::Index(_)) => Item::Value(Value::Array(Array::new())),
        None => Item::None,
    }
}

fn replace_item(item: &mut Item, value: Json) -> Option<()> {
    match (item, value) {
        (Item::Table(table), Json::Object(values)) => replace_table(table, values),
        (item @ Item::ArrayOfTables(_), Json::Array(values)) if values.is_empty() => {
            // An empty array of tables has no TOML header representation.
            *item = Item::Value(Value::Array(Array::new()));
            Some(())
        }
        (Item::ArrayOfTables(array), Json::Array(values)) => {
            while array.len() > values.len() {
                array.remove(array.len() - 1);
            }
            for (index, value) in values.into_iter().enumerate() {
                let Json::Object(values) = value else {
                    return None;
                };
                if let Some(table) = array.get_mut(index) {
                    replace_table(table, values)?;
                } else {
                    let mut table = Table::new();
                    replace_table(&mut table, values)?;
                    array.push(table);
                }
            }
            Some(())
        }
        (Item::Value(current), value) => replace_value(current, value),
        (item, Json::Object(values)) => {
            let mut table = Table::new();
            replace_table(&mut table, values)?;
            *item = Item::Table(table);
            Some(())
        }
        (item, value) => {
            *item = Item::Value(new_value(value)?);
            Some(())
        }
    }
}

fn replace_table(table: &mut dyn TableLike, values: Map<String, Json>) -> Option<()> {
    let removed: Vec<_> = table
        .iter()
        .filter(|(key, _)| !values.contains_key(*key))
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in removed {
        table.remove(&key);
    }
    for (key, value) in values {
        edit_table(table, &[Segment::Key(key)], Some(value))?;
    }
    Some(())
}

fn replace_value(current: &mut Value, value: Json) -> Option<()> {
    if value_json(current) == value {
        return Some(());
    }
    match (current, value) {
        (Value::InlineTable(table), Json::Object(values)) => replace_table(table, values),
        (Value::Array(array), Json::Array(values)) => {
            while array.len() > values.len() {
                array.remove(array.len() - 1);
            }
            for (index, value) in values.into_iter().enumerate() {
                if let Some(current) = array.get_mut(index) {
                    replace_value(current, value)?;
                } else {
                    array.push(new_value(value)?);
                }
            }
            Some(())
        }
        (current, value) => {
            let mut value = new_value(value)?;
            *value.decor_mut() = current.decor().clone();
            *current = value;
            Some(())
        }
    }
}

fn new_value(input: Json) -> Option<Value> {
    Some(match input {
        Json::Null => return None,
        Json::Bool(value) => Value::from(value),
        Json::Number(value) if value.is_i64() || value.is_u64() => Value::from(value.as_i64()?),
        Json::Number(value) => Value::from(value.as_f64()?),
        Json::String(value) => Value::from(value),
        Json::Array(values) => {
            let mut array = Array::new();
            for value in values {
                array.push(new_value(value)?);
            }
            Value::Array(array)
        }
        Json::Object(values) => {
            let mut table = InlineTable::new();
            for (key, value) in values {
                table.insert(key, new_value(value)?);
            }
            Value::InlineTable(table)
        }
    })
}

fn table_json(table: &dyn TableLike) -> Json {
    Json::Object(
        table
            .iter()
            .map(|(key, item)| {
                let value = match item {
                    Item::None => Json::Null,
                    Item::Value(value) => value_json(value),
                    Item::Table(table) => table_json(table),
                    Item::ArrayOfTables(array) => {
                        array.iter().map(|table| table_json(table)).collect()
                    }
                };
                (key.to_owned(), value)
            })
            .collect(),
    )
}

fn value_json(value: &Value) -> Json {
    match value {
        Value::String(value) => value.value().clone().into(),
        Value::Integer(value) => (*value.value()).into(),
        Value::Float(value) => serde_json::Number::from_f64(*value.value())
            .map_or_else(|| value.value().to_string().into(), Json::Number),
        Value::Boolean(value) => (*value.value()).into(),
        Value::Datetime(value) => value.value().to_string().into(),
        Value::Array(array) => array.iter().map(value_json).collect(),
        Value::InlineTable(table) => table_json(table),
    }
}

pub(super) fn validate(root: &Path, input: &str) -> Result<(), Error> {
    let path = root.join(FILE_NAME);
    let config =
        ManifestConfig::from_toml(input.trim_start_matches('\u{feff}')).map_err(|source| {
            Error::Manifest(Box::new(ContextDiscoveryError::Manifest {
                path: path.clone(),
                source: Box::new(source),
            }))
        })?;

    let format = match &config {
        ManifestConfig::Project(config) => config.configuration().formatting_settings(),
        ManifestConfig::Workspace(config) => config.formatting_settings(),
    }
    .clone();
    let candidate = Layer::new(
        root,
        "manifest",
        &format,
        input.trim_start_matches('\u{feff}'),
    )
    .map_err(Error::Formatting)?;
    let context = discovery::resolve_manifest(root, &path, config)
        .map_err(|source| Error::Manifest(Box::new(source)))?;
    match context {
        DiscoveryContext::Standalone(_) => resolve(&[candidate], root),
        DiscoveryContext::Workspace {
            workspace,
            current_member: Some(_),
        } => {
            let parent_root = workspace.root();
            let (text, config) = policy(parent_root)?;
            let parent =
                Layer::new(parent_root, "workspace", &config, &text).map_err(Error::Formatting)?;
            resolve(&[parent, candidate], root)
        }
        DiscoveryContext::Workspace {
            workspace,
            current_member: None,
        } => {
            resolve(std::slice::from_ref(&candidate), root)?;
            for member in workspace.members() {
                let (text, config) = policy(member.root())?;
                let member_layer = Layer::new(member.root(), "member", &config, &text)
                    .map_err(Error::Formatting)?;
                let layers = [
                    Layer {
                        root: candidate.root,
                        name: candidate.name,
                        config: candidate.config,
                        legacy: candidate.legacy.clone(),
                    },
                    member_layer,
                ];
                resolve(&layers, member.root())?;
            }
            Ok(())
        }
    }
}

fn policy(root: &Path) -> Result<(String, formatting::Config), Error> {
    let path = root.join(FILE_NAME);
    let text = super::Raw::read(&path)?.text;
    let config =
        ManifestConfig::from_toml(text.trim_start_matches('\u{feff}')).map_err(|source| {
            Error::Manifest(Box::new(ContextDiscoveryError::Manifest {
                path,
                source: Box::new(source),
            }))
        })?;
    let format = match config {
        ManifestConfig::Project(project) => project.configuration().formatting_settings().clone(),
        ManifestConfig::Workspace(workspace) => workspace.formatting_settings().clone(),
    };
    Ok((text, format))
}

fn resolve(layers: &[Layer<'_>], path: &Path) -> Result<(), Error> {
    for language in [Language::Bsl, Language::Sdbl] {
        formatting::resolve(layers, path, language, &Patch::default())
            .map_err(Error::Formatting)?;
    }
    Ok(())
}
