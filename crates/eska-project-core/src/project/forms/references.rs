//! Resolve `QName` references while their XML namespace scope is still available.

use crate::project::metadata_model::PropertyKey;
use roxmltree::Node;
use serde::Serialize;
use std::ops::Range;

pub(super) const STYLE: &str = "http://v8.1c.ru/8.1/data/ui/style";
pub(super) const SYSTEM_FONT: &str = "http://v8.1c.ru/8.1/data/ui/fonts/system";
pub(super) const CONFIG: &str = "http://v8.1c.ru/8.1/data/enterprise/current-config";
pub(super) const WEB_COLOR: &str = "http://v8.1c.ru/8.1/data/ui/colors/web";
pub(super) const WINDOWS_COLOR: &str = "http://v8.1c.ru/8.1/data/ui/colors/windows";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceSlot {
    Text,
    Ref,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormReference {
    pub range: Range<usize>,
    pub property: PropertyKey,
    pub slot: ReferenceSlot,
    pub target: PropertyKey,
}

pub(super) fn collect(root: Node<'_, '_>) -> Vec<FormReference> {
    let mut output = Vec::new();
    for node in root.descendants().filter(Node::is_element) {
        for (slot, text) in [
            (ReferenceSlot::Ref, node.attribute("ref")),
            (
                ReferenceSlot::Text,
                node.text()
                    .filter(|_| !node.children().any(|child| child.is_element())),
            ),
        ] {
            let Some((prefix, name)) = text.and_then(|text| text.trim().split_once(':')) else {
                continue;
            };
            let Some(namespace) = node.lookup_namespace_uri(Some(prefix)) else {
                continue;
            };
            if name.is_empty()
                || name.contains(':')
                || name.contains(char::is_whitespace)
                || !matches!(
                    namespace,
                    STYLE | SYSTEM_FONT | CONFIG | WEB_COLOR | WINDOWS_COLOR
                )
            {
                continue;
            }
            output.push(FormReference {
                range: node.range(),
                property: PropertyKey {
                    namespace: node.tag_name().namespace().map(str::to_owned),
                    name: node.tag_name().name().into(),
                },
                slot,
                target: PropertyKey {
                    namespace: Some(namespace.into()),
                    name: name.into(),
                },
            });
        }
    }
    output
}
