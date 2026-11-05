//! Navigation targets are resolved from retained model identities, never caller-provided paths or ranges.

use super::{Entry, failure};
use crate::ide::{envelope::error, params};
use eska_project_core::project::{
    forms::{ElementIdentity, ElementKey, FormPart, PropertySource},
    metadata_model::PropertyKey,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Target {
    revision: String,
    target: Value,
}
#[derive(Deserialize)]
struct Identity {
    source: String,
    part: String,
    key: Key,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Key {
    Root,
    Xml { value: String },
}
pub(super) fn identity(value: &Value) -> Result<ElementIdentity, Value> {
    let value: Identity = params::decode(value)?;
    if value.part != "current" {
        return Err(error(-32602));
    }
    Ok(ElementIdentity {
        source: value.source,
        part: FormPart::Current,
        key: match value.key {
            Key::Root => ElementKey::Root,
            Key::Xml { value } => ElementKey::Xml(value.parse().map_err(|_| error(-32602))?),
        },
    })
}
pub(super) fn check_revision(entry: &Entry, revision: &str) -> Result<(), Value> {
    if entry.last_error.is_some() {
        return Err(failure("stale_form_model"));
    }
    if params::counter(revision)? != entry.revision {
        return Err(failure("stale_form_revision"));
    }
    Ok(())
}
pub(super) fn locate(entry: &Entry, args: &Value, palette: bool) -> Result<Value, Value> {
    let input: Target = params::decode(args)?;
    check_revision(entry, &input.revision)?;
    let kind = input.target["kind"].as_str().ok_or_else(|| error(-32602))?;
    let origin: &PropertySource = if kind == "definition" {
        let values = match input.target["section"].as_str() {
            Some("attributes") => &entry.preview.form.attributes,
            Some("commands") => &entry.preview.form.commands,
            Some("parameters") => &entry.preview.form.parameters,
            _ => return Err(error(-32602)),
        };
        &values
            .iter()
            .find(|value| value.name.as_deref() == input.target["name"].as_str())
            .ok_or_else(|| failure("target_unavailable"))?
            .origin
    } else {
        let id = identity(&input.target["identity"])?;
        let value = entry
            .preview
            .form
            .root
            .find(&id)
            .ok_or_else(|| failure("target_unavailable"))?;
        match kind {
            "element" => &value.origin,
            "binding" => entry
                .preview
                .presentation
                .elements
                .iter()
                .find(|element| element.identity == id)
                .and_then(|element| element.binding.as_ref())
                .ok_or_else(|| failure("target_unavailable"))?,
            "property" => {
                let key: PropertyKey = params::decode(&input.target["property"])?;
                let occurrence = match input.target.get("occurrence") {
                    Some(index) if palette => params::decode::<usize>(index)?,
                    _ => 0,
                };
                &value
                    .properties
                    .iter()
                    .filter(|value| value.property.key == key)
                    .nth(occurrence)
                    .ok_or_else(|| failure("target_unavailable"))?
                    .origin
            }
            "attribute" if palette => {
                let key: PropertyKey = params::decode(&input.target["attribute"])?;
                &value
                    .qualifiers
                    .iter()
                    .find(|value| value.key == key)
                    .ok_or_else(|| failure("target_unavailable"))?
                    .origin
            }
            _ => return Err(error(-32602)),
        }
    };
    let dependency = entry
        .preview
        .dependencies
        .iter()
        .find(|dependency| dependency.id == origin.source)
        .ok_or_else(|| failure("target_unavailable"))?;
    Ok(
        json!({"source":dependency.id,"snapshot":dependency.snapshot,"root":crate::ide::dto::path(&dependency.root),"path":crate::ide::dto::path(&dependency.path),"range":origin.range}),
    )
}
