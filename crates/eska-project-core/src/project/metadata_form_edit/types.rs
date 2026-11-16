//! Managed-form type descriptions share primitive limits and native qualifier validation.

use std::collections::BTreeMap;

use roxmltree::Node;

use super::{FormEditError, invalid};
use crate::project::{
    metadata_create::capabilities,
    metadata_edit::types,
    metadata_model::{MetadataKind, ObjectId, PropertyKey},
    metadata_objects::templates::generated_types,
    metadata_rename::validate_name,
    metadata_workspace::{ObjectSummary, PropertyTypeChoice},
};

const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const BUILTINS: &[(&str, &str)] = &[
    (CORE, "ValueTable"),
    (CORE, "ValueTree"),
    (CORE, "ValueListType"),
    (CORE, "TypeDescription"),
    (CORE, "StandardPeriod"),
    (CORE, "Array"),
    (CORE, "FixedArray"),
    (CORE, "Structure"),
    (CORE, "FixedStructure"),
    (CORE, "Map"),
    (CORE, "FixedMap"),
    (CORE, "Null"),
    (CORE, "Undefined"),
    (types::CFG, "DynamicList"),
    (types::CFG, "ConstantsSet"),
    (types::CFG, "ReportBuilder"),
    ("http://v8.1c.ru/8.1/data/ui", "Color"),
    ("http://v8.1c.ru/8.1/data/ui", "Picture"),
    ("http://v8.1c.ru/8.1/data/ui", "Font"),
    ("http://v8.1c.ru/8.1/data/ui", "FormattedString"),
    ("http://v8.1c.ru/8.1/data/txtedt", "TextDocument"),
    (
        "http://v8.1c.ru/8.2/data/spreadsheet",
        "SpreadsheetDocument",
    ),
    ("http://v8.1c.ru/8.2/data/chart", "Chart"),
    (
        "http://v8.1c.ru/8.2/data/formatted-document",
        "FormattedDocument",
    ),
    (
        "http://v8.1c.ru/8.1/data-composition-system/settings",
        "SettingsComposer",
    ),
    (
        "http://v8.1c.ru/8.1/data-composition-system/settings",
        "Filter",
    ),
];

#[derive(Debug)]
pub struct FormTypeOption {
    pub choice: PropertyTypeChoice,
    pub member: &'static str,
}

impl FormTypeOption {
    fn new(namespace: &str, name: String, owner: Option<ObjectSummary>, set: bool) -> Self {
        Self {
            choice: PropertyTypeChoice {
                key: PropertyKey {
                    namespace: Some(namespace.to_owned()),
                    name,
                },
                object: owner,
            },
            member: if set { "TypeSet" } else { "Type" },
        }
    }

    /// The same primitive qualifiers and defaults used by metadata property editing.
    #[must_use]
    pub fn qualifier(&self) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
        types::qualifier(&self.choice.key)
    }
}

pub fn type_options(objects: &BTreeMap<ObjectId, ObjectSummary>) -> Vec<FormTypeOption> {
    let mut options: Vec<_> = types::PRIMITIVES
        .iter()
        .chain(BUILTINS)
        .map(|(namespace, name)| FormTypeOption::new(namespace, (*name).to_owned(), None, false))
        .collect();
    options.extend(
        ["AnyRef", "AnyIBRef"]
            .into_iter()
            .chain(types::REFERENCE_TYPES.iter().map(|(family, _)| *family))
            .map(|family| FormTypeOption::new(types::CFG, family.to_owned(), None, true)),
    );
    for kind in MetadataKind::ALL.iter().copied() {
        let families: Vec<_> = generated_types(kind)
            .into_iter()
            .filter(|(family, _)| family.starts_with(kind.designer_tag()) || set_family(family))
            .collect();
        if families.is_empty() {
            continue;
        }
        for object in objects
            .values()
            .filter(|object| object.parent.is_none() && object.kind == kind)
        {
            options.extend(families.iter().map(|(family, _)| {
                FormTypeOption::new(
                    types::CFG,
                    format!("{family}.{}", object.name),
                    Some(object.clone()),
                    set_family(family),
                )
            }));
        }
    }
    options
}

fn set_family(family: &str) -> bool {
    matches!(family, "DefinedType" | "Characteristic")
}

pub fn type_reference(key: &PropertyKey) -> Option<ObjectId> {
    if key.namespace.as_deref() != Some(types::CFG) {
        return None;
    }
    let (family, name) = key.name.split_once('.')?;
    validate_name(name).ok()?;
    let kind = match family {
        "DefinedType" => Some(MetadataKind::DefinedType),
        "Characteristic" => Some(MetadataKind::ChartOfCharacteristicTypes),
        _ => MetadataKind::ALL.iter().copied().find(|kind| {
            family.starts_with(kind.designer_tag())
                && generated_types(*kind)
                    .iter()
                    .any(|(candidate, _)| candidate == family)
        }),
    };
    kind.map(|kind| ObjectId::from_parts(None, kind.as_str(), name))
        .or_else(|| crate::project::forms::metadata_type_id(key))
}

pub(super) fn validate(node: Node<'_, '_>) -> Result<(), FormEditError> {
    if node.attributes().len() != 0
        || node
            .descendants()
            .any(|node| node.is_comment() || node.is_pi())
        || node.children().any(|child| {
            child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
        })
    {
        return Err(invalid("type", "invalid_shape"));
    }
    types::validate_members(node, supported)?;
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

fn supported(key: &PropertyKey, set: bool) -> bool {
    if let Some((family, _)) = key.name.split_once('.') {
        return set == set_family(family) && type_reference(key).is_some();
    }
    if set {
        return key.namespace.as_deref() == Some(types::CFG)
            && (matches!(key.name.as_str(), "AnyRef" | "AnyIBRef")
                || types::REFERENCE_TYPES
                    .iter()
                    .any(|(family, _)| *family == key.name));
    }
    types::PRIMITIVES
        .iter()
        .chain(BUILTINS)
        .any(|(namespace, name)| key.namespace.as_deref() == Some(namespace) && key.name == *name)
        || key.namespace.as_deref() == Some(CORE) && key.name == "ValueList"
}
