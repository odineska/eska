//! Reviewed metadata reference lists share one shape, domain policy and namespace-safe writer.

use super::{
    EditError, EditPlan, Replacement, ScalarSchema, patch, references,
    schema::{MD, READABLE, XSI},
    types::{indentation, newline},
};
use crate::project::metadata_model::MetadataKind;
use roxmltree::Node;

#[derive(Clone, Copy)]
pub struct Rule {
    pub owner: &'static str,
    pub property: &'static str,
    pub domain: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        owner: "Document",
        property: "RegisterRecords",
        domain: "BasicRegister",
    },
    Rule {
        owner: "Sequence",
        property: "RegisterRecords",
        domain: "BasicRegister",
    },
    Rule {
        owner: "Sequence",
        property: "Documents",
        domain: "Document",
    },
    Rule {
        owner: "DocumentJournal",
        property: "RegisteredDocuments",
        domain: "Document",
    },
    Rule {
        owner: "ChartOfCalculationTypes",
        property: "BaseCalculationTypes",
        domain: "ChartOfCalculationTypes",
    },
    Rule {
        owner: "FunctionalOptionsParameter",
        property: "Use",
        domain: "FunctionalOptionParameterUse",
    },
    Rule {
        owner: "Subsystem",
        property: "Content",
        domain: "MdObject",
    },
    Rule {
        owner: "FunctionalOption",
        property: "Content",
        domain: "MdObject",
    },
    Rule {
        owner: "FilterCriterion",
        property: "Content",
        domain: "MdObject",
    },
    Rule {
        owner: "Configuration",
        property: "Content",
        domain: "MdObject",
    },
];

pub fn rules(owner: &str) -> impl Iterator<Item = Rule> + '_ {
    RULES
        .iter()
        .copied()
        .filter(move |rule| rule.owner == owner)
}

pub fn rule(owner: &str, property: &str) -> Option<Rule> {
    rules(owner).find(|rule| rule.property == property)
}

pub fn accepts(domain: &str, parts: &[(MetadataKind, &str)]) -> bool {
    references::accepts(domain, parts)
}

pub fn validate(domain: &str, values: &[String]) -> Result<(), EditError> {
    if values.len() > 100_000 {
        return Err(EditError::InvalidValue);
    }
    let mut unique = std::collections::BTreeSet::new();
    for value in values {
        if !references::parts(value).is_some_and(|parts| accepts(domain, &parts))
            || !unique.insert(value.to_lowercase())
        {
            return Err(EditError::InvalidValue);
        }
    }
    Ok(())
}

pub fn schema(rule: Rule, node: Option<Node<'_, '_>>) -> Result<ScalarSchema, EditError> {
    let mut values = Vec::new();
    if let Some(node) = node {
        if node.attributes().len() != 0 {
            return Err(EditError::UnsupportedValue);
        }
        for child in node.children() {
            if child.is_text() && child.text().is_some_and(|value| value.trim().is_empty()) {
                continue;
            }
            if !child.has_tag_name((READABLE, "Item"))
                || child.children().any(|node| !node.is_text())
                || child.attributes().len() != 1
            {
                return Err(EditError::UnsupportedValue);
            }
            let kind = child
                .attribute((XSI, "type"))
                .and_then(|value| value.split_once(':'));
            if !kind.is_some_and(|(prefix, name)| {
                name == "MDObjectRef" && child.lookup_namespace_uri(Some(prefix)) == Some(READABLE)
            }) {
                return Err(EditError::UnsupportedValue);
            }
            values.push(child.text().ok_or(EditError::InvalidValue)?.to_owned());
        }
    }
    validate(rule.domain, &values)?;
    Ok(ScalarSchema::References {
        domain: rule.domain.to_owned(),
        values,
    })
}

pub fn replace(
    input: &str,
    node: Node<'_, '_>,
    domain: &str,
    values: &[String],
) -> Result<EditPlan, EditError> {
    validate(domain, values)?;
    if node
        .children()
        .filter(Node::is_element)
        .filter_map(|node| node.text())
        .eq(values.iter().map(String::as_str))
    {
        return EditPlan::from_replacements(input, Vec::new());
    }
    let content = items(node, values)?;
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
    values: &[String],
) -> Result<EditPlan, EditError> {
    validate(rule.domain, values)?;
    if values.is_empty() {
        return EditPlan::from_replacements(input, Vec::new());
    }
    let prefix = if properties.default_namespace() == Some(MD) {
        String::new()
    } else {
        properties
            .lookup_prefix(MD)
            .map(|prefix| format!("{prefix}:"))
            .ok_or(EditError::InvalidXml)?
    };
    let tag = format!("{prefix}{}", rule.property);
    let content = items(properties, values)?;
    let fragment = format!("<{tag}>{content}</{tag}>");
    let source = &input[properties.range()];
    let offset = properties.range().start + source.rfind("</").ok_or(EditError::InvalidXml)?;
    let (range, text) = if let Some(indent) = indentation(input, properties.range().start) {
        let start = input[..offset].rfind('\n').map_or(0, |index| index + 1);
        if input[start..offset]
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
        {
            (
                start..offset,
                format!("{indent}\t{fragment}{}{indent}", newline(input)),
            )
        } else {
            (offset..offset, fragment)
        }
    } else {
        (offset..offset, fragment)
    };
    patch::replacements_plan(input, vec![Replacement { range, text }])
}

fn items(scope: Node<'_, '_>, values: &[String]) -> Result<String, EditError> {
    let xr = scope.lookup_prefix(READABLE).unwrap_or("eskaRef");
    let xsi = scope.lookup_prefix(XSI).unwrap_or("eskaType");
    let declarations = format!(
        "{}{}",
        if scope.lookup_namespace_uri(Some(xr)) == Some(READABLE) {
            String::new()
        } else {
            format!(" xmlns:{xr}=\"{READABLE}\"")
        },
        if scope.lookup_namespace_uri(Some(xsi)) == Some(XSI) {
            String::new()
        } else {
            format!(" xmlns:{xsi}=\"{XSI}\"")
        }
    );
    values
        .iter()
        .map(|value| {
            Ok(format!(
                "<{xr}:Item{declarations} {xsi}:type=\"{xr}:MDObjectRef\">{}</{xr}:Item>",
                patch::escape(value)?
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_calculation_types_use_native_reference_members_and_domain() {
        let rule = rule("ChartOfCalculationTypes", "BaseCalculationTypes").unwrap();
        let input = format!(
            "<BaseCalculationTypes xmlns='{MD}' xmlns:r='{READABLE}' xmlns:s='{XSI}'><r:Item s:type='r:MDObjectRef'>ChartOfCalculationTypes.Plan</r:Item></BaseCalculationTypes>"
        );
        let document = roxmltree::Document::parse(&input).unwrap();
        let property = document.root_element();
        assert!(
            matches!(schema(rule, Some(property)).unwrap(), ScalarSchema::References { domain, values }
            if domain == "ChartOfCalculationTypes" && values == ["ChartOfCalculationTypes.Plan"])
        );
        assert!(
            replace(
                &input,
                property,
                rule.domain,
                &["ChartOfCalculationTypes.Plan".to_owned()]
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            replace(
                &input,
                property,
                rule.domain,
                &["Document.Invoice".to_owned()]
            )
            .is_err()
        );
        let changed = replace(
            &input,
            property,
            rule.domain,
            &["ChartOfCalculationTypes.Other".to_owned()],
        )
        .unwrap();
        let document = roxmltree::Document::parse(changed.output()).unwrap();
        assert!(
            matches!(schema(rule, Some(document.root_element())).unwrap(), ScalarSchema::References { values, .. }
            if values == ["ChartOfCalculationTypes.Other"])
        );
        assert_eq!(
            changed
                .history()
                .replay(changed.output(), true)
                .unwrap()
                .output(),
            input
        );
    }
}
