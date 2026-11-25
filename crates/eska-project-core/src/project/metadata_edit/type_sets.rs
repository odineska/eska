//! Type-description membership changes reuse the scalar type and qualifier contract.

use std::{collections::BTreeSet, fmt::Write};

use roxmltree::Node;
use serde::Serialize;

use super::{EditError, EditPlan, Replacement, ScalarSchema, catalog, patch, schema, types};
use crate::project::metadata_model::{MetadataKind, ObjectId, PropertyKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TypeDomain {
    Value,
    Reference,
    EventSource,
}

#[derive(Clone, Copy)]
pub struct Rule {
    pub property: &'static str,
    pub domain: TypeDomain,
}

/// The reviewed property catalog is also the source of missing optional type containers.
pub fn rules(id: &ObjectId) -> impl Iterator<Item = Rule> {
    let owner = schema::object_class(id);
    let event_subscription = owner == "EventSubscription";
    catalog::fields(&owner)
        .iter()
        .filter(|field| field.namespace == schema::MD && field.property)
        .filter_map(move |field| {
            let domain = match field.model_type {
                "TypeDescription" if event_subscription && field.name == "Source" => {
                    TypeDomain::EventSource
                }
                "TypeDescription" => TypeDomain::Value,
                "SourceReferenceTypeDescription" | "MdObjectReferenceTypeDescription" => {
                    TypeDomain::Reference
                }
                _ => return None,
            };
            Some(Rule {
                property: field.name,
                domain,
            })
        })
}

pub fn rule(id: &ObjectId, property: &str) -> Option<Rule> {
    rules(id).find(|rule| rule.property == property)
}

/// Event sources expose native generated object/manager families, rather than arbitrary `QNames`.
pub fn families(domain: TypeDomain) -> Vec<(String, MetadataKind)> {
    if domain != TypeDomain::EventSource {
        return types::REFERENCE_TYPES
            .iter()
            .filter_map(|(family, tag)| {
                MetadataKind::from_xml_tag(tag)
                    .ok()
                    .map(|kind| ((*family).to_owned(), kind))
            })
            .collect();
    }
    MetadataKind::ALL
        .iter()
        .copied()
        .filter(|kind| {
            matches!(
                kind,
                MetadataKind::Catalog
                    | MetadataKind::Document
                    | MetadataKind::ExchangePlan
                    | MetadataKind::ChartOfAccounts
                    | MetadataKind::ChartOfCalculationTypes
                    | MetadataKind::ChartOfCharacteristicTypes
                    | MetadataKind::BusinessProcess
                    | MetadataKind::Task
                    | MetadataKind::InformationRegister
                    | MetadataKind::AccumulationRegister
                    | MetadataKind::AccountingRegister
                    | MetadataKind::CalculationRegister
                    | MetadataKind::Constant
                    | MetadataKind::DefinedType
                    | MetadataKind::DataProcessor
                    | MetadataKind::Report
                    | MetadataKind::DocumentJournal
                    | MetadataKind::Sequence
                    | MetadataKind::Recalculation
            )
        })
        .flat_map(|kind| {
            crate::project::metadata_objects::templates::generated_types(kind)
                .into_iter()
                .filter(move |(_, category)| event_category(kind, category))
                .map(move |(family, _)| (family, kind))
        })
        .collect()
}

fn event_category(kind: MetadataKind, category: &str) -> bool {
    match category {
        "Object" => !matches!(kind, MetadataKind::DataProcessor | MetadataKind::Report),
        "Manager" => !matches!(
            kind,
            MetadataKind::Constant
                | MetadataKind::ExchangePlan
                | MetadataKind::Sequence
                | MetadataKind::Recalculation
        ),
        "RecordSet" | "ValueManager" | "DefinedType" => true,
        _ => false,
    }
}

pub fn target(domain: TypeDomain, key: &PropertyKey) -> Option<Vec<(MetadataKind, &str)>> {
    if key.namespace.as_deref() != Some(types::CFG) {
        return None;
    }
    let (family, name) = key.name.split_once('.')?;
    let kind = families(domain)
        .into_iter()
        .find_map(|(candidate, kind)| (candidate == family).then_some(kind))?;
    if kind == MetadataKind::Recalculation {
        let (owner, name) = name.split_once('.')?;
        Some(vec![
            (MetadataKind::CalculationRegister, owner),
            (kind, name),
        ])
    } else {
        Some(vec![(kind, name)])
    }
}

fn accepts(domain: TypeDomain, key: &PropertyKey) -> bool {
    if domain == TypeDomain::Value && types::supported(key) {
        return true;
    }
    if key.namespace.as_deref() != Some(types::CFG) {
        return false;
    }
    let (family, name) = key
        .name
        .split_once('.')
        .map_or((key.name.as_str(), None), |(family, name)| {
            (family, Some(name))
        });
    if name.is_none() && (domain != TypeDomain::EventSource || family == "DefinedType") {
        return false;
    }
    if name.is_some_and(|name| {
        let parts: Vec<_> = name.split('.').collect();
        let expected = if family == "RecalculationRecordSet" {
            2
        } else {
            1
        };
        parts.len() != expected
            || parts
                .iter()
                .any(|part| crate::project::metadata_rename::validate_name(part).is_err())
    }) {
        return false;
    }
    families(domain)
        .iter()
        .any(|(candidate, _)| candidate == family)
}

pub fn validate(domain: TypeDomain, keys: &[PropertyKey]) -> Result<(), EditError> {
    if keys.len() > 64 || (domain == TypeDomain::EventSource && keys.is_empty()) {
        return Err(EditError::InvalidValue);
    }
    let mut unique = BTreeSet::new();
    for key in keys {
        if !accepts(domain, key)
            || !unique.insert((key.namespace.as_deref(), key.name.to_lowercase()))
        {
            return Err(EditError::InvalidValue);
        }
    }
    Ok(())
}

fn type_set(domain: TypeDomain, key: &PropertyKey) -> bool {
    domain == TypeDomain::EventSource && !key.name.contains('.')
}

pub fn schema(rule: Rule, node: Option<Node<'_, '_>>) -> Result<ScalarSchema, EditError> {
    let keys = if let Some(node) = node {
        if node.attributes().len() != 0
            || node
                .descendants()
                .any(|node| node.is_comment() || node.is_pi())
            || node.children().any(|node| {
                node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
            })
        {
            return Err(EditError::UnsupportedValue);
        }
        types::validate_members(node, |key, set| {
            accepts(rule.domain, key) && type_set(rule.domain, key) == set
        })?;
        let keys: Vec<_> = node
            .children()
            .filter(|node| {
                node.has_tag_name((schema::CORE, "Type"))
                    || node.has_tag_name((schema::CORE, "TypeSet"))
            })
            .filter_map(types::key)
            .collect();
        if !keys.is_empty() {
            validate(rule.domain, &keys)?;
        }
        keys
    } else {
        Vec::new()
    };
    Ok(ScalarSchema::Types {
        domain: rule.domain,
        keys,
    })
}

/// Membership updates preserve still-applicable qualifier bytes and discard obsolete groups.
pub fn replace(
    input: &str,
    node: Node<'_, '_>,
    domain: TypeDomain,
    keys: &[PropertyKey],
) -> Result<EditPlan, EditError> {
    validate(domain, keys)?;
    let rule = Rule {
        property: "",
        domain,
    };
    let ScalarSchema::Types { keys: current, .. } = schema(rule, Some(node))? else {
        return Err(EditError::UnsupportedValue);
    };
    if current == keys {
        return EditPlan::from_replacements(input, Vec::new());
    }
    let content = content(input, node, Some(node), domain, keys)?;
    let replacement = if let Some(first) = node.first_child() {
        let last = node.last_child().ok_or(EditError::InvalidXml)?;
        Replacement {
            range: first.range().start..last.range().end,
            text: content,
        }
    } else {
        patch::empty_content(input, node, content)?
    };
    patch::replacements_plan(input, vec![replacement])
}

pub fn insert(
    input: &str,
    properties: Node<'_, '_>,
    rule: Rule,
    keys: &[PropertyKey],
) -> Result<EditPlan, EditError> {
    validate(rule.domain, keys)?;
    if keys.is_empty() {
        return EditPlan::from_replacements(input, Vec::new());
    }
    let (tag, declaration) = types::qname(
        properties,
        &PropertyKey {
            namespace: Some(schema::MD.to_owned()),
            name: rule.property.to_owned(),
        },
    )?;
    let content = content(input, properties, None, rule.domain, keys)?;
    let fragment = format!(
        "<{tag}{}>{content}</{tag}>",
        declaration.unwrap_or_default()
    );
    let replacement = if properties.first_child().is_none() {
        patch::empty_content(input, properties, fragment)?
    } else {
        // Designer reads dependent fields in order: changing Source after Event clears the event.
        let dependent = properties.children().find(|child| {
            rule.domain == TypeDomain::EventSource
                && child.tag_name().namespace() == Some(schema::MD)
                && matches!(child.tag_name().name(), "Event" | "Handler")
        });
        let offset = if let Some(dependent) = dependent {
            dependent.range().start
        } else {
            properties.range().start
                + input[properties.range()]
                    .rfind("</")
                    .ok_or(EditError::InvalidXml)?
        };
        Replacement {
            range: offset..offset,
            text: fragment,
        }
    };
    patch::replacements_plan(input, vec![replacement])
}

fn content(
    input: &str,
    scope: Node<'_, '_>,
    existing: Option<Node<'_, '_>>,
    domain: TypeDomain,
    keys: &[PropertyKey],
) -> Result<String, EditError> {
    let mut fragments = Vec::new();
    for key in keys {
        let found = existing
            .into_iter()
            .flat_map(|node| node.children())
            .find(|node| {
                (node.has_tag_name((schema::CORE, "Type"))
                    || node.has_tag_name((schema::CORE, "TypeSet")))
                    && types::key(*node).as_ref() == Some(key)
            });
        fragments.push(if let Some(node) = found {
            input[node.range()].to_owned()
        } else {
            member(domain, key)?
        });
    }
    let mut qualifiers: Vec<_> = keys.iter().filter_map(types::qualifier).collect();
    qualifiers.sort_unstable_by_key(|(name, _)| types::order(name));
    qualifiers.dedup_by_key(|(name, _)| *name);
    for (name, fields) in qualifiers {
        let existing = existing
            .into_iter()
            .flat_map(|node| node.children())
            .find(|node| node.has_tag_name((schema::CORE, name)));
        fragments.push(if let Some(node) = existing {
            input[node.range()].to_owned()
        } else {
            qualifier(input, scope, name, fields)?
        });
    }
    Ok(fragments.concat())
}

fn member(domain: TypeDomain, key: &PropertyKey) -> Result<String, EditError> {
    // A local QName binding avoids prefix collisions with the element's CORE namespace.
    let tag = if type_set(domain, key) {
        "TypeSet"
    } else {
        "Type"
    };
    let namespace = key.namespace.as_deref().ok_or(EditError::InvalidValue)?;
    Ok(format!(
        "<v8:{tag} xmlns:v8=\"{}\" xmlns:t=\"{}\">t:{}</v8:{tag}>",
        schema::CORE,
        patch::escape_attribute(namespace)?,
        patch::escape(&key.name)?
    ))
}

fn qualifier(
    input: &str,
    scope: Node<'_, '_>,
    name: &str,
    fields: &[(&str, &str)],
) -> Result<String, EditError> {
    if scope.lookup_prefix(schema::CORE).is_some()
        || scope.default_namespace() == Some(schema::CORE)
    {
        // The existing renderer owns numeric/string/date qualifier structure and defaults.
        return types::qualifier_xml(input, scope, name, fields);
    }
    qualifier_local(name, fields)
}

fn qualifier_local(name: &str, fields: &[(&str, &str)]) -> Result<String, EditError> {
    let wrapper = format!("<root xmlns:v8=\"{}\"/>", schema::CORE);
    let document = roxmltree::Document::parse(&wrapper).map_err(|_| EditError::InvalidXml)?;
    let fragment = types::qualifier_xml(&wrapper, document.root_element(), name, fields)?;
    let mut output = fragment;
    let offset = output.find('>').ok_or(EditError::InvalidXml)?;
    let mut declaration = String::new();
    let _ = write!(declaration, " xmlns:v8=\"{}\"", schema::CORE);
    output.insert_str(offset, &declaration);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::metadata_edit::{EditingDocument, PropertyChange, snapshot};

    fn key(namespace: &str, name: &str) -> PropertyKey {
        PropertyKey {
            namespace: Some(namespace.to_owned()),
            name: name.to_owned(),
        }
    }

    #[test]
    fn membership_preserves_applicable_qualifiers_and_removes_stale_groups() {
        let input = format!(
            "\u{feff}<root xmlns:v8='{}' xmlns:xs='{}'><Type><v8:Type>xs:string</v8:Type><v8:Type>xs:decimal</v8:Type><v8:NumberQualifiers><v8:Digits>17</v8:Digits><v8:FractionDigits>3</v8:FractionDigits><v8:AllowedSign>Any</v8:AllowedSign></v8:NumberQualifiers><v8:StringQualifiers><v8:Length>77</v8:Length><v8:AllowedLength>Variable</v8:AllowedLength></v8:StringQualifiers></Type><Other>untouched</Other></root>",
            schema::CORE,
            schema::XS
        );
        let doc = roxmltree::Document::parse(&input).unwrap();
        let node = doc.root_element().first_element_child().unwrap();
        let keys = vec![key(schema::XS, "decimal"), key(schema::XS, "boolean")];
        let plan = replace(&input, node, TypeDomain::Value, &keys).unwrap();
        assert!(
            plan.output()
                .contains("<v8:Digits>17</v8:Digits><v8:FractionDigits>3</v8:FractionDigits>")
        );
        assert!(!plan.output().contains("StringQualifiers"));
        assert!(plan.output().ends_with("<Other>untouched</Other></root>"));
        assert_eq!(
            plan.history().replay(plan.output(), true).unwrap().output(),
            input
        );
        let parsed = roxmltree::Document::parse(plan.output()).unwrap();
        let node = parsed.root_element().first_element_child().unwrap();
        assert!(
            replace(plan.output(), node, TypeDomain::Value, &keys)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn new_primitive_members_use_existing_qualifier_defaults() {
        let input = "<root><Type/></root>";
        let doc = roxmltree::Document::parse(input).unwrap();
        let node = doc.root_element().first_element_child().unwrap();
        let plan = replace(
            input,
            node,
            TypeDomain::Value,
            &[
                key(schema::XS, "decimal"),
                key(schema::XS, "string"),
                key(schema::XS, "dateTime"),
            ],
        )
        .unwrap();
        let doc = roxmltree::Document::parse(plan.output()).unwrap();
        let node = doc.root_element().first_element_child().unwrap();
        types::validate_shape(node).unwrap();
        assert!(plan.output().contains("<v8:Digits>10</v8:Digits>"));
        assert!(plan.output().contains("<v8:Length>0</v8:Length>"));
        assert!(
            plan.output()
                .contains("<v8:DateFractions>DateTime</v8:DateFractions>")
        );
    }

    #[test]
    fn missing_source_is_discovered_and_writes_native_wildcard_and_object_types() {
        let input = format!(
            "<MetaDataObject xmlns='{}' version='2.20'><EventSubscription uuid='00000000-0000-4000-8000-000000000001'><Properties><Name>Hook</Name><Comment>unchanged</Comment><Event>BeforeWrite</Event><Handler>CommonModule.Hooks.BeforeWrite</Handler></Properties></EventSubscription></MetaDataObject>",
            schema::MD
        );
        let id = ObjectId::from_parts(None, "event-subscription", "Hook");
        let document = EditingDocument {
            input: &input,
            id: &id,
            parent: None,
            predefined: false,
            profile: super::super::EditingProfile::Base,
        };
        let editing = document.inspect().unwrap();
        let field = editing
            .fields
            .iter()
            .find(|field| field.path.len() == 1 && field.path[0].key.name == "Source")
            .unwrap();
        let change = PropertyChange::Types {
            keys: vec![
                key(types::CFG, "CatalogObject"),
                key(types::CFG, "DocumentObject.Invoice"),
            ],
        };
        let plan = document
            .update(&snapshot(&input), &field.path, &change)
            .unwrap();
        let parsed = roxmltree::Document::parse(plan.output()).unwrap();
        assert!(
            parsed
                .descendants()
                .any(|node| node.has_tag_name((schema::CORE, "TypeSet"))
                    && types::key(node) == Some(key(types::CFG, "CatalogObject")))
        );
        assert!(
            parsed
                .descendants()
                .any(|node| node.has_tag_name((schema::CORE, "Type"))
                    && types::key(node) == Some(key(types::CFG, "DocumentObject.Invoice")))
        );
        assert!(plan.output().contains("<Comment>unchanged</Comment>"));
        assert!(plan.output().find("<Source>").unwrap() < plan.output().find("<Event>").unwrap());
    }

    #[test]
    fn domains_reject_unknown_types_duplicates_and_opaque_source_content() {
        assert!(validate(TypeDomain::Value, &[key(types::CFG, "DocumentObject.Doc")]).is_err());
        assert!(validate(TypeDomain::EventSource, &[key(schema::XS, "string")]).is_err());
        assert!(
            validate(
                TypeDomain::Value,
                &[key(schema::XS, "boolean"), key(schema::XS, "boolean")]
            )
            .is_err()
        );
        assert!(
            validate(
                TypeDomain::EventSource,
                &[key(types::CFG, "DocumentObject.Bad.Name")]
            )
            .is_err()
        );
        let input = format!(
            "<Source xmlns:v8='{}' xmlns:c='{}'><v8:Type c:hint='keep'>c:CatalogObject.Items</v8:Type></Source>",
            schema::CORE,
            types::CFG
        );
        let doc = roxmltree::Document::parse(&input).unwrap();
        assert!(
            replace(
                &input,
                doc.root_element(),
                TypeDomain::EventSource,
                &[key(types::CFG, "DocumentObject.Doc")]
            )
            .is_err()
        );
    }
}
