//! Managed-form type descriptions share primitive limits and native qualifier validation.

use roxmltree::Node;

use super::{FormEditError, invalid};
use crate::project::{
    metadata_create::capabilities, metadata_edit::types, metadata_model::PropertyKey,
};

const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const XS: &str = "http://www.w3.org/2001/XMLSchema";

pub(super) fn validate(node: Node<'_, '_>) -> Result<(), FormEditError> {
    if node
        .children()
        .any(|child| child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(invalid("type", "invalid_shape"));
    }
    types::validate_description(node, supported)?;
    let limits = capabilities().limits;
    for qualifier in node.children().filter(Node::is_element) {
        let value = |name| {
            qualifier
                .children()
                .find(|child| child.has_tag_name((CORE, name)))
                .and_then(|child| child.text())
        };
        let valid = match qualifier.tag_name().name() {
            "NumberQualifiers" => matches!(value("AllowedSign"), Some("Any" | "Nonnegative")),
            "StringQualifiers" | "BinaryDataQualifiers" => {
                let length = value("Length").and_then(|value| value.parse::<u32>().ok());
                match (length, value("AllowedLength")) {
                    (Some(length), Some("Variable")) => length <= limits.string_length_max,
                    (Some(length), Some("Fixed")) => (limits.string_fixed_length_min
                        ..=limits.string_fixed_length_max)
                        .contains(&length),
                    _ => false,
                }
            }
            "DateQualifiers" => {
                matches!(value("DateFractions"), Some("Date" | "Time" | "DateTime"))
            }
            _ => true,
        };
        if !valid {
            return Err(invalid("type", "invalid_qualifiers"));
        }
    }
    Ok(())
}

fn supported(key: &PropertyKey) -> bool {
    match key.namespace.as_deref() {
        Some(XS) => types::PRIMITIVES
            .iter()
            .any(|(namespace, name)| *namespace == XS && *name == key.name),
        Some(CORE) => {
            types::PRIMITIVES
                .iter()
                .any(|(namespace, name)| *namespace == CORE && *name == key.name)
                || matches!(
                    key.name.as_str(),
                    "ValueTable"
                        | "ValueTree"
                        | "ValueList"
                        | "TypeDescription"
                        | "Array"
                        | "FixedArray"
                        | "Structure"
                        | "FixedStructure"
                        | "Map"
                        | "FixedMap"
                )
        }
        Some(types::CFG) => {
            key.name == "DynamicList" || crate::project::forms::metadata_type_id(key).is_some()
        }
        _ => false,
    }
}
