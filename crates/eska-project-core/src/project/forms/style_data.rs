//! Read the native Designer style payload separately from its metadata descriptor.

use std::collections::HashSet;

use roxmltree::{Document, Node};

use super::{FormReference, PropertySource, SourcedProperty};
use crate::project::metadata_parser;

const NAMESPACE: &str = "http://v8.1c.ru/8.3/xcf/extrnprops";

#[derive(Debug)]
pub struct FormStyleData {
    pub source: String,
    pub references: Vec<FormReference>,
    pub items: Vec<StyleValue>,
}

#[derive(Debug)]
pub struct StyleValue {
    pub name: String,
    pub value: SourcedProperty,
}

pub(super) fn parse(source: &str, document: &Document<'_>) -> Option<FormStyleData> {
    let root = document.root_element();
    if !root.has_tag_name((NAMESPACE, "Style")) || structural_text(root) {
        return None;
    }
    let mut names = HashSet::new();
    let mut items = Vec::new();
    for item in root.children().filter(Node::is_element) {
        if !item.has_tag_name((NAMESPACE, "Item")) || structural_text(item) {
            return None;
        }
        let name = item.attribute("name")?;
        if name.is_empty() || name.contains(['/', '\\', '\0']) || !names.insert(name) {
            return None;
        }
        let mut children = item.children().filter(Node::is_element);
        let value = children.next()?;
        if children.next().is_some()
            || value.tag_name().namespace() != Some(NAMESPACE)
            || !matches!(value.tag_name().name(), "Font" | "Color")
            || value.children().any(|child| child.is_element())
        {
            return None;
        }
        let mut diagnostics = Vec::new();
        let property = metadata_parser::values::property(value, &mut diagnostics);
        if !diagnostics.is_empty() {
            return None;
        }
        items.push(StyleValue {
            name: name.into(),
            value: SourcedProperty {
                origin: PropertySource {
                    source: source.into(),
                    range: value.range(),
                },
                property,
            },
        });
    }
    Some(FormStyleData {
        source: source.into(),
        references: super::references::collect(root),
        items,
    })
}

fn structural_text(node: Node<'_, '_>) -> bool {
    node.children()
        .filter(Node::is_text)
        .any(|child| !child.text().unwrap_or_default().trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::metadata_model::MetadataValue;

    #[test]
    fn native_payload_preserves_namespaces_font_qualifiers_and_source_ranges() {
        let xml = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/forms/native-style-values/Styles/ProbeStyleValues/Ext/Style.xml"
        ));
        let document = Document::parse(xml).unwrap();
        let result = parse("style-data", &document).unwrap();
        assert_eq!(result.items.len(), 6);
        let font = result
            .items
            .iter()
            .find(|item| item.name == "StyleItem.ProbeFont")
            .unwrap();
        assert!(xml[font.value.origin.range.clone()].starts_with("<Font"));
        assert_eq!(font.value.origin.source, "style-data");
        assert!(
            font.value
                .property
                .qualifiers
                .iter()
                .any(|(key, value)| key.name == "height" && value == "18")
        );
        assert!(result.references.iter().any(|reference| reference.range
            == font.value.origin.range
            && reference.target.namespace.as_deref()
                == Some(super::super::references::SYSTEM_FONT)
            && reference.target.name == "DefaultGUIFont"));
        let color = result
            .items
            .iter()
            .find(|item| item.name == "StyleItem.ProbeColor")
            .unwrap();
        assert_eq!(
            color.value.property.value,
            MetadataValue::Text("#9800FF".into())
        );
    }

    #[test]
    fn invalid_style_structure_cannot_become_an_implicit_override() {
        for body in [
            "<Other/>",
            "<Style xmlns='foreign'/>",
            "<Style xmlns='http://v8.1c.ru/8.3/xcf/extrnprops'><Item><Color>#000000</Color></Item></Style>",
            "<Style xmlns='http://v8.1c.ru/8.3/xcf/extrnprops'><Item name='A'><Color>#000000</Color><Font/></Item></Style>",
            "<Style xmlns='http://v8.1c.ru/8.3/xcf/extrnprops'><Item name='A'><Font><Nested/></Font></Item></Style>",
            "<Style xmlns='http://v8.1c.ru/8.3/xcf/extrnprops'><Item name='A'><Color>#000000</Color></Item><Item name='A'><Color>#ffffff</Color></Item></Style>",
        ] {
            let document = Document::parse(body).unwrap();
            assert!(parse("style-data", &document).is_none(), "{body}");
        }
    }
}
