//! Dynamic lists own DCS field names and keep list preferences alongside the rules.

use std::collections::BTreeSet;

use roxmltree::{Document, Node};
use serde::Serialize;

use super::{
    AppearanceRule, AppearanceSchema, CORE, FORM_NAMESPACE, FormEditError, SETTINGS, XSI, codec,
    document, inspect_rules, invalid, render, render_rules, replace_rules, rule_nodes,
};
use crate::project::{
    metadata_edit::{Replacement, types::CFG},
    metadata_form_edit::FormTarget,
};

const SCHEMA: &str = "http://v8.1c.ru/8.1/data-composition-system/schema";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAppearanceSchema {
    pub attribute: String,
    pub fields: Vec<String>,
    pub appearance: Option<AppearanceSchema>,
}

struct ListSettings<'a, 'input> {
    attribute: Node<'a, 'input>,
    settings: Option<Node<'a, 'input>>,
    list: Option<Node<'a, 'input>>,
    appearance: Option<Node<'a, 'input>>,
}

impl<'a, 'input> ListSettings<'a, 'input> {
    fn read(attribute: Node<'a, 'input>) -> Result<Self, FormEditError> {
        let settings = codec::one(attribute, FORM_NAMESPACE, "Settings")?;
        if settings.is_some_and(|node| !codec::typed(node, FORM_NAMESPACE, "DynamicList")) {
            return Err(invalid(
                "conditionalAppearance",
                "unsupported_list_settings",
            ));
        }
        let list = child(settings, FORM_NAMESPACE, "ListSettings")?;
        let appearance = child(list, SETTINGS, "conditionalAppearance")?;
        Ok(Self {
            attribute,
            settings,
            list,
            appearance,
        })
    }

    fn change(
        &self,
        input: &str,
        edits: &[AppearanceRule],
    ) -> Result<Vec<Replacement>, FormEditError> {
        let scope = self.list.or(self.settings).unwrap_or(self.attribute);
        let originals = rule_nodes(self.appearance, true)?;
        let rules = render_rules(
            input,
            self.appearance.unwrap_or(scope),
            &originals,
            edits,
            codec::path,
        )?;
        if let Some(block) = self.appearance {
            return replace_rules(input, block, &originals, &rules);
        }
        if rules.is_empty() {
            return Ok(Vec::new());
        }
        let block = codec::fragment(scope, "dcsset:conditionalAppearance", &rules.join(""));
        if let Some(list) = self.list {
            let before = list.children().find(|node| {
                let tag = node.tag_name();
                tag.namespace() == Some(SETTINGS)
                    && matches!(tag.name(), "itemsViewMode" | "itemsUserSettingID")
            });
            return Ok(vec![document::insert(input, list, &block, before)?]);
        }
        let tag = render::tag(scope, "ListSettings");
        let list = format!("<{tag}>{block}</{tag}>");
        if let Some(settings) = self.settings {
            return Ok(vec![document::insert(input, settings, &list, None)?]);
        }
        let tag = render::tag(scope, "Settings");
        let kind = render::tag(scope, "DynamicList");
        let settings = format!("<{tag} xmlns:xsi=\"{XSI}\" xsi:type=\"{kind}\">{list}</{tag}>");
        Ok(vec![document::insert(
            input,
            self.attribute,
            &settings,
            None,
        )?])
    }
}

fn child<'a, 'input>(
    parent: Option<Node<'a, 'input>>,
    namespace: &str,
    name: &str,
) -> Result<Option<Node<'a, 'input>>, FormEditError> {
    parent.map_or(Ok(None), |node| codec::one(node, namespace, name))
}

fn dynamic_list(node: Node<'_, '_>) -> bool {
    node.has_tag_name((FORM_NAMESPACE, "Attribute"))
        && node
            .parent()
            .is_some_and(|parent| parent.has_tag_name((FORM_NAMESPACE, "Attributes")))
        && document::child(node, "Type").is_some_and(|description| {
            description.children().any(|child| {
                child.has_tag_name((CORE, "Type"))
                    && child.text().map(str::trim).is_some_and(|value| {
                        let (prefix, name) = value
                            .split_once(':')
                            .map_or((None, value), |(prefix, name)| (Some(prefix), name));
                        name == "DynamicList" && child.lookup_namespace_uri(prefix) == Some(CFG)
                    })
            })
        })
}

fn fields(root: Node<'_, '_>, attribute: Node<'_, '_>, name: &str) -> Vec<String> {
    let prefix = format!("{name}.");
    let bound = root
        .descendants()
        .filter(|node| node.has_tag_name((FORM_NAMESPACE, "DataPath")))
        .filter_map(|node| node.text()?.trim().strip_prefix(&prefix));
    let query = document::child(attribute, "Settings")
        .into_iter()
        .flat_map(|node| {
            node.children()
                .filter(|node| node.has_tag_name((FORM_NAMESPACE, "Field")))
        })
        .filter_map(|node| {
            node.children()
                .find(|node| node.has_tag_name((SCHEMA, "dataPath")))
        })
        .filter_map(|node| node.text().map(str::trim));
    bound
        .chain(query)
        .filter(|path| codec::path(path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(in crate::project::metadata_form_edit) fn inspect_lists(
    root: Node<'_, '_>,
) -> Vec<ListAppearanceSchema> {
    document::child(root, "Attributes")
        .into_iter()
        .flat_map(|attributes| attributes.children().filter(|node| dynamic_list(*node)))
        .filter_map(|node| {
            let name = node.attribute("name")?;
            Some(ListAppearanceSchema {
                attribute: name.to_owned(),
                fields: fields(root, node, name),
                appearance: ListSettings::read(node)
                    .ok()
                    .and_then(|settings| inspect_rules(settings.appearance, true)),
            })
        })
        .collect()
}

pub(in crate::project::metadata_form_edit) fn change_list(
    input: &str,
    xml: &Document<'_>,
    name: &str,
    rules: &[AppearanceRule],
) -> Result<Vec<Replacement>, FormEditError> {
    let attribute = document::target(
        xml,
        &FormTarget::Attribute {
            name: name.to_owned(),
        },
    )?;
    if !dynamic_list(attribute) {
        return Err(invalid("conditionalAppearance", "not_dynamic_list"));
    }
    ListSettings::read(attribute)?.change(input, rules)
}
