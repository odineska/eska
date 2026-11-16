//! Minimal Designer declarations use native type families and stable semantic identity slots.

use super::{ObjectError, invalid, xml};
use crate::project::{metadata_edit::escape_xml, metadata_model::MetadataKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

pub const fn required_properties(kind: MetadataKind) -> &'static [&'static str] {
    use MetadataKind as K;
    match kind {
        K::CommonCommand => &["Group"],
        K::EventSubscription => &["Source", "Event", "Handler"],
        K::FunctionalOption => &["Location"],
        K::FunctionalOptionsParameter => &["Use"],
        K::ScheduledJob => &["MethodName"],
        K::BusinessProcess => &["Task"],
        K::DocumentJournal => &["RegisteredDocuments"],
        K::Sequence => &["Documents"],
        K::Operation => &["ProcedureName"],
        K::Method => &["Handler"],
        K::Column => &["References"],
        _ => &[],
    }
}

pub const fn unique_property(kind: MetadataKind) -> Option<&'static str> {
    match kind {
        MetadataKind::HTTPService => Some("RootURL"),
        MetadataKind::WebService => Some("DescriptorFileName"),
        MetadataKind::URLTemplate => Some("Template"),
        MetadataKind::Method => Some("HTTPMethod"),
        _ => None,
    }
}

pub fn generated_types(kind: MetadataKind) -> Vec<(String, &'static str)> {
    use MetadataKind as K;
    if kind == K::TabularSection {
        return vec![
            ("TabularSection".to_owned(), "TabularSection"),
            ("TabularSectionRow".to_owned(), "TabularSectionRow"),
        ];
    }
    let categories: &[&str] = match kind {
        K::Catalog
        | K::Document
        | K::Task
        | K::ExchangePlan
        | K::ChartOfCalculationTypes
        | K::ChartOfCharacteristicTypes => &["Object", "Ref", "Selection", "List", "Manager"],
        K::BusinessProcess => &[
            "Object",
            "Ref",
            "Selection",
            "List",
            "Manager",
            "RoutePointRef",
        ],
        K::ChartOfAccounts => &[
            "Object",
            "Ref",
            "Selection",
            "List",
            "Manager",
            "ExtDimensionTypes",
            "ExtDimensionTypesRow",
        ],
        K::DataProcessor | K::Report => &["Object", "Manager"],
        K::Enum => &["Ref", "Manager", "List"],
        K::Constant => &["Manager", "ValueManager", "ValueKey"],
        K::DocumentJournal => &["Selection", "List", "Manager"],
        K::InformationRegister => &[
            "Record",
            "Manager",
            "Selection",
            "List",
            "RecordSet",
            "RecordKey",
            "RecordManager",
        ],
        K::AccumulationRegister | K::CalculationRegister => &[
            "Record",
            "Manager",
            "Selection",
            "List",
            "RecordSet",
            "RecordKey",
        ],
        K::AccountingRegister => &[
            "Record",
            "ExtDimensions",
            "RecordSet",
            "RecordKey",
            "Selection",
            "List",
            "Manager",
        ],
        K::Sequence | K::Recalculation => &["Record", "Manager", "RecordSet"],
        K::FilterCriterion => &["Manager", "List"],
        K::IntegrationService
        | K::IntegrationServiceChannel
        | K::SettingsStorage
        | K::WSReference => &["Manager"],
        K::ExternalDataSource => &["Manager", "TablesManager", "CubesManager"],
        _ => &[],
    };
    let mut result: Vec<_> = categories
        .iter()
        .map(|category| (format!("{}{category}", kind.designer_tag()), *category))
        .collect();
    if kind == K::DefinedType {
        result.push(("DefinedType".to_owned(), "DefinedType"));
    }
    if kind == K::CalculationRegister {
        result.push(("RecalculationsManager".to_owned(), "Recalcs"));
    }
    if kind == K::ChartOfCharacteristicTypes {
        result.push(("Characteristic".to_owned(), "Characteristic"));
    }
    if kind == K::ChartOfCalculationTypes {
        for category in [
            "DisplacingCalculationTypes",
            "DisplacingCalculationTypesRow",
            "BaseCalculationTypes",
            "BaseCalculationTypesRow",
            "LeadingCalculationTypes",
            "LeadingCalculationTypesRow",
        ] {
            result.push((category.to_owned(), category));
        }
    }
    result
}

pub fn identity_slots(kind: MetadataKind) -> BTreeSet<String> {
    let mut keys = BTreeSet::from(["object".to_owned()]);
    for (family, _) in generated_types(kind) {
        keys.insert(format!("{family}:type"));
        keys.insert(format!("{family}:value"));
    }
    if kind == MetadataKind::ExchangePlan {
        keys.insert("exchange-node".to_owned());
    }
    keys
}

pub fn allocate(
    kind: MetadataKind,
    saved: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, ObjectError> {
    allocate_slots(identity_slots(kind), saved)
}

pub fn allocate_slots(
    slots: BTreeSet<String>,
    saved: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, ObjectError> {
    if let Some(saved) = saved {
        if saved.keys().cloned().collect::<BTreeSet<_>>() != slots
            || saved.values().any(|value| !xml::valid_new_uuid(value))
            || saved.values().collect::<BTreeSet<_>>().len() != saved.len()
        {
            return Err(invalid("identities", "invalid_mapping"));
        }
        return Ok(saved.clone());
    }
    slots
        .into_iter()
        .map(|slot| {
            Ok((
                slot,
                crate::project::metadata_create::allocate_uuid().map_err(ObjectError::Identity)?,
            ))
        })
        .collect()
}

pub fn descriptor(
    kind: MetadataKind,
    name: &str,
    identities: &BTreeMap<String, String>,
    parent_kind: MetadataKind,
    ancestry: &[(String, String)],
) -> Result<String, ObjectError> {
    let get = |key: &str| {
        identities
            .get(key)
            .ok_or_else(|| invalid("identities", "missing_slot"))
    };
    let name = escape_xml(name)?;
    let tag = kind.designer_tag();
    let uuid = get("object")?;
    let mut internal = String::new();
    let names = if parent_kind == MetadataKind::Configuration {
        name.clone()
    } else {
        ancestry
            .iter()
            .map(|(_, name)| name.as_str())
            .chain(std::iter::once(name.as_str()))
            .collect::<Vec<_>>()
            .join(".")
    };
    for (family, category) in generated_types(kind) {
        let generated_family = if kind == MetadataKind::TabularSection {
            format!("{}{family}", parent_kind.designer_tag())
        } else {
            family.clone()
        };
        let _ = write!(
            internal,
            "<xr:GeneratedType name=\"{generated_family}.{names}\" category=\"{category}\"><xr:TypeId>{}</xr:TypeId><xr:ValueId>{}</xr:ValueId></xr:GeneratedType>",
            get(&format!("{family}:type"))?,
            get(&format!("{family}:value"))?
        );
    }
    if kind == MetadataKind::ExchangePlan {
        let _ = write!(
            internal,
            "<xr:ThisNode>{}</xr:ThisNode>",
            get("exchange-node")?
        );
    }
    if !internal.is_empty() {
        internal = format!("<InternalInfo>{internal}</InternalInfo>");
    }
    let properties = defaults(kind, &name);
    let children = if !crate::project::configurator::ConfiguratorSchema::collections(kind)
        .is_empty()
        || kind == MetadataKind::ExternalDataSource
    {
        "<ChildObjects/>"
    } else {
        ""
    };
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MetaDataObject xmlns=\"{}\" xmlns:xr=\"{}\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" version=\"2.20\">\n\t<{tag} uuid=\"{uuid}\">{internal}<Properties><Name>{name}</Name><Synonym/><Comment/>{properties}</Properties>{children}</{tag}>\n</MetaDataObject>\n",
        xml::MD,
        xml::XR
    ))
}

fn defaults(kind: MetadataKind, name: &str) -> String {
    use MetadataKind as K;
    match kind {
        K::CommonForm | K::Form => "<FormType>Managed</FormType>".to_owned(),
        K::CommonModule => "<Global>false</Global><Server>true</Server>".to_owned(),
        K::HTTPService => format!("<RootURL>{name}</RootURL>"),
        K::WebService => format!("<Namespace>urn:eska:{name}</Namespace><DescriptorFileName>{name}.1cws</DescriptorFileName>"),
        K::Language => format!("<LanguageCode>{name}</LanguageCode>"),
        K::XDTOPackage => format!("<Namespace>urn:eska:{name}</Namespace>"),
        K::Parameter => "<XDTOValueType>xs:string</XDTOValueType>".to_owned(),
        K::Constant | K::SessionParameter | K::DefinedType | K::CommonAttribute | K::Attribute | K::Dimension | K::Resource | K::AddressingAttribute | K::AccountingFlag | K::ExtDimensionAccountingFlag => "<Type><v8:Type>xs:string</v8:Type><v8:StringQualifiers><v8:Length>0</v8:Length><v8:AllowedLength>Variable</v8:AllowedLength></v8:StringQualifiers></Type>".to_owned(),
        _ => String::new(),
    }
}
