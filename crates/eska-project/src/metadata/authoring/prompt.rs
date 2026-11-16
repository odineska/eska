//! Required fields come from the backend catalog, just as they do in the editor wizard.

use eska_presentation::localization::Localizer;
use eska_project_core::project::{
    metadata_objects::ObjectRequest, metadata_workspace::ProjectSession,
};
use serde_json::Value;

use super::{super::creation::prompt, input};
use crate::ide::authoring as shared;

pub(super) fn required_properties(
    project: &mut ProjectSession,
    request: &mut ObjectRequest,
    locale: &Localizer,
) -> Result<(), Value> {
    let ObjectRequest::Create {
        kind,
        parent,
        template: None,
        property_values,
        property_references,
        property_types,
        ..
    } = request
    else {
        return Ok(());
    };
    let schema = project
        .object_schema(parent.as_ref())
        .map_err(shared::object_failure)?;
    for property in schema
        .required_properties
        .get(kind.as_str())
        .into_iter()
        .flatten()
    {
        if property_values.contains_key(property)
            || property_references.contains_key(property)
            || property_types.contains_key(property)
        {
            continue;
        }
        let is_type = schema
            .type_properties
            .get(kind.as_str())
            .is_some_and(|fields| fields.contains(property));
        let is_reference = schema
            .reference_properties
            .get(kind.as_str())
            .is_some_and(|fields| fields.contains(property));
        if is_type {
            eprintln!("{}", locale.text("metadata-authoring-required-type-prompt"));
        } else if is_reference {
            eprintln!(
                "{}",
                locale.text("metadata-authoring-required-reference-prompt")
            );
        }
        let value = prompt::line(locale, &format!("platform-property-md-{property}"), None)?;
        if is_type {
            property_types.extend(input::types(&[format!("{property}={value}")])?);
        } else if is_reference {
            property_references.extend(input::references(&[format!("{property}={value}")])?);
        } else {
            property_values.insert(property.clone(), value);
        }
    }
    Ok(())
}
