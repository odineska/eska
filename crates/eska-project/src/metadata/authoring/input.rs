//! Requests and plans accept semantic data only; unknown envelopes fail before source writes.

use std::{collections::BTreeMap, path::Path};

use eska_project_core::project::metadata_model::{MetadataKind, ObjectId, PropertyKey};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::super::{read_input, read_json_limited};

pub(super) fn object_id(value: &str) -> Result<ObjectId, Value> {
    if value.is_empty() {
        return Err(invalid("objectId", "empty"));
    }
    serde_json::from_value(json!(value)).map_err(|_| invalid("objectId", "invalid_identity"))
}

pub(super) fn metadata_kind(value: &str) -> Result<MetadataKind, Value> {
    MetadataKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.as_str() == value)
        .ok_or_else(|| invalid("kind", "unsupported_kind"))
}

pub(super) fn properties(values: &[String]) -> Result<BTreeMap<String, String>, Value> {
    let mut result = BTreeMap::new();
    for value in values {
        let (property, value) = value
            .split_once('=')
            .ok_or_else(|| invalid("set", "expected_property_value"))?;
        if property.is_empty()
            || result
                .insert(property.to_owned(), value.to_owned())
                .is_some()
        {
            return Err(invalid("set", "duplicate_or_empty_property"));
        }
    }
    Ok(result)
}

pub(super) fn references(values: &[String]) -> Result<BTreeMap<String, Vec<String>>, Value> {
    let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for value in values {
        let (property, value) = value
            .split_once('=')
            .ok_or_else(|| invalid("reference", "expected_property_value"))?;
        if property.is_empty() || value.is_empty() {
            return Err(invalid("reference", "empty_property_or_reference"));
        }
        result
            .entry(property.to_owned())
            .or_default()
            .push(value.to_owned());
    }
    Ok(result)
}

pub(super) fn request<T: DeserializeOwned>(path: &Path) -> Result<T, Value> {
    let value = read_input(path, &["schemaVersion", "request"])?;
    serde_json::from_value(value["request"].clone())
        .map_err(|_| invalid("request", "invalid_request"))
}

pub(super) fn types(values: &[String]) -> Result<BTreeMap<String, Vec<PropertyKey>>, Value> {
    let mut result: BTreeMap<String, Vec<PropertyKey>> = BTreeMap::new();
    for value in values {
        let (property, value) = value
            .split_once('=')
            .ok_or_else(|| invalid("type", "expected_property_type"))?;
        let (prefix, name) = value
            .split_once(':')
            .ok_or_else(|| invalid("type", "expected_qualified_type"))?;
        let namespace = match prefix {
            "xs" => "http://www.w3.org/2001/XMLSchema",
            "cfg" => "http://v8.1c.ru/8.1/data/enterprise/current-config",
            _ => return Err(invalid("type", "unsupported_namespace_prefix")),
        };
        if property.is_empty() || name.is_empty() {
            return Err(invalid("type", "empty_property_or_type"));
        }
        result
            .entry(property.to_owned())
            .or_default()
            .push(PropertyKey {
                namespace: Some(namespace.to_owned()),
                name: name.to_owned(),
            });
    }
    Ok(result)
}

pub(super) fn plan<T: DeserializeOwned>(path: &Path, operation: &str) -> Result<T, Value> {
    let value = read_json_limited(path, 64 * 1024 * 1024)?;
    let outer = value
        .as_object()
        .ok_or_else(|| invalid("plan", "invalid_envelope"))?;
    if value["schema_version"] != 1
        || outer
            .keys()
            .any(|key| !["schema_version", "result"].contains(&key.as_str()))
    {
        return Err(invalid("plan", "invalid_envelope"));
    }
    let result = value["result"]
        .as_object()
        .ok_or_else(|| invalid("plan", "invalid_result"))?;
    let allowed: &[&str] = if operation == "form-edit" {
        &[
            "schemaVersion",
            "valid",
            "changed",
            "operation",
            "form",
            "source",
            "prepared",
            "inputs",
            "assignedIds",
            "changes",
        ]
    } else {
        &[
            "schemaVersion",
            "valid",
            "changed",
            "operation",
            "objectId",
            "prepared",
            "inputs",
            "files",
            "directories",
            "references",
        ]
    };
    if value["result"]["schemaVersion"] != 1
        || value["result"]["valid"] != true
        || value["result"]["operation"] != operation
        || result.keys().any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(invalid("plan", "invalid_result"));
    }
    serde_json::from_value(value["result"]["prepared"].clone())
        .map_err(|_| invalid("plan", "invalid_prepared_request"))
}

pub(super) fn required(fields: &[&str]) -> Value {
    json!({"kind":"creation_arguments_required","details":{"fields":fields}})
}

pub(super) fn invalid(field: &str, reason: &str) -> Value {
    json!({"kind":"object_input_invalid","details":{"field":field,"reason":reason}})
}
