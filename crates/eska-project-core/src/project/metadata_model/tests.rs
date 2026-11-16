use std::collections::BTreeSet;

use super::*;
use crate::project::{ProjectConfiguration, ProjectName, ProjectType, SourceFormat};

#[test]
fn semantic_values_reject_unknown_json_fields_at_every_nested_boundary() {
    use serde_json::json;
    let key = json!({"namespace":null,"name":"AnyPreservedXmlName"});
    let property = json!({"key":key,"qualifiers":[],"value":{"Text":"unchanged"}});
    assert!(serde_json::from_value::<MetadataProperty>(property.clone()).is_ok());
    let mut unknown_property = property.clone();
    unknown_property["caption"] = json!("must not disappear silently");
    assert!(serde_json::from_value::<MetadataProperty>(unknown_property).is_err());
    let mut unknown_key = property;
    unknown_key["key"]["ns"] = json!("misspelled namespace");
    assert!(serde_json::from_value::<MetadataProperty>(unknown_key).is_err());
    for value in [
        json!({"QualifiedText":{"text":"x:Any","key":key,"extra":true}}),
        json!({"TypedText":{"text":"value","key":key,"extra":true}}),
        json!({"Localized":[{"language":"ru","content":"Текст","extra":true}]}),
    ] {
        assert!(serde_json::from_value::<MetadataValue>(value).is_err());
    }
}

/// Construct an object with an intentionally repeated UUID, as real fixtures allow.
fn object(kind: MetadataKind, name: &str, parent: Option<ObjectId>) -> MetadataObject {
    MetadataObject::new(kind, name.to_owned(), "same-uuid".to_owned(), parent)
        .expect("nonempty name")
}

/// Escaping preserves the CLI contract and prevents separators from changing ancestry.
#[test]
fn hierarchical_identity_preserves_escaping_and_distinguishes_names() {
    let owner = object(MetadataKind::Catalog, "Контрагенты/%:A", None);
    let section = object(
        MetadataKind::TabularSection,
        "Контакты",
        Some(owner.id().clone()),
    );
    let attribute = object(
        MetadataKind::Attribute,
        "Телефон",
        Some(section.id().clone()),
    );
    assert_eq!(
        attribute.id().as_str(),
        "catalog:Контрагенты%2F%25%3AA/tabular-section:Контакты/attribute:Телефон"
    );
    assert_ne!(
        owner.id(),
        object(MetadataKind::Catalog, "Контрагенты%2F%:A", None).id()
    );
    assert_ne!(
        attribute.id(),
        object(MetadataKind::Attribute, "Телефон", Some(owner.id().clone())).id()
    );
    assert_eq!(attribute.parent(), Some(section.id()));
    assert!(
        MetadataObject::new(MetadataKind::Catalog, String::new(), String::new(), None).is_err()
    );
}

/// UUIDs, input order and translated display labels do not participate in identity.
#[test]
fn identities_are_logical_not_uuid_based() {
    let first = object(MetadataKind::Document, "Продажа", None);
    let second = object(MetadataKind::Document, "Возврат", None);
    let alternate = MetadataObject::new(
        MetadataKind::Document,
        "Продажа".into(),
        "other".into(),
        None,
    )
    .unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!(first.id(), alternate.id());
    assert_eq!(
        BTreeSet::from([first.id(), second.id()]),
        BTreeSet::from([second.id(), first.id()])
    );
}

/// Virtual groups, modules and real objects occupy disjoint project-scoped namespaces.
#[test]
fn node_and_project_scopes_do_not_collide() {
    let owner = object(MetadataKind::CommonModule, "Модули", None);
    let real = NodeId::Object(owner.id().clone());
    let module = NodeId::Module {
        owner: owner.id().clone(),
        role: ModuleRole::Module,
    };
    let group = NodeId::Collection {
        owner: owner.id().clone(),
        kind: CollectionKind::Modules,
    };
    assert_eq!(BTreeSet::from([real.clone(), module, group]).len(), 3);
    let first = ScopedNodeId {
        project: ProjectScope::Member(ProjectName::parse("first".into()).unwrap()),
        node: real.clone(),
    };
    let second = ScopedNodeId {
        project: ProjectScope::Member(ProjectName::parse("second".into()).unwrap()),
        node: real,
    };
    assert_ne!(first, second);
}

/// External descriptors share object kinds while project types retain their distinction.
#[test]
fn registry_handles_aliases_and_unknown_types_explicitly() {
    assert_eq!(
        MetadataKind::from_xml_tag("ExternalReport"),
        Ok(MetadataKind::Report)
    );
    assert_eq!(
        MetadataKind::from_xml_tag("ExternalDataProcessor"),
        Ok(MetadataKind::DataProcessor)
    );
    assert_eq!(
        MetadataKind::from_xml_tag("FutureType").unwrap_err().tag,
        "FutureType"
    );
    let keys: BTreeSet<_> = MetadataKind::ALL.iter().map(|kind| kind.as_str()).collect();
    assert_eq!(keys.len(), MetadataKind::ALL.len());
}

/// The manifest chooses the project schema; root XML must agree for all four types.
#[test]
fn root_type_is_checked_against_manifest_without_build_settings() {
    for (kind, tag, properties) in [
        (ProjectType::Configuration, "Configuration", ""),
        (
            ProjectType::Extension,
            "Configuration",
            "<ConfigurationExtensionPurpose>Patch</ConfigurationExtensionPurpose>",
        ),
        (ProjectType::Processing, "ExternalDataProcessor", ""),
        (ProjectType::Report, "ExternalReport", ""),
    ] {
        let xml = format!(
            "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><{tag}><Properties>{properties}</Properties></{tag}></MetaDataObject>"
        );
        let settings = ProjectConfiguration::new(kind, SourceFormat::DesignerXml);
        assert_eq!(
            MetadataProject::from_root_descriptor(&settings, &xml)
                .unwrap()
                .project_type(),
            kind
        );
        let wrong = if kind == ProjectType::Report {
            ProjectType::Processing
        } else {
            ProjectType::Report
        };
        assert!(
            matches!(MetadataProject::from_root_descriptor(&ProjectConfiguration::new(wrong, SourceFormat::DesignerXml), &xml), Err(MetadataProjectError::TypeMismatch { expected, actual }) if expected == wrong && actual == kind)
        );
    }
    let settings = ProjectConfiguration::new(ProjectType::Configuration, SourceFormat::DesignerXml);
    assert!(matches!(
        MetadataProject::from_root_descriptor(&settings, "<broken>"),
        Err(MetadataProjectError::InvalidXml(_))
    ));
    assert!(matches!(
        MetadataProject::from_root_descriptor(&settings, "<root/>"),
        Err(MetadataProjectError::UnsupportedRoot)
    ));
}
