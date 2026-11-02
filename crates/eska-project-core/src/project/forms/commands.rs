//! Static desktop Configurator commands; preview buttons never execute application actions.

use super::presentation::{definition_type, field_type, literal, metadata_property};
use super::{
    ComposedForm, EffectiveElement, ElementKind, FormResource, PresentationText, PropertySource,
};
use crate::project::metadata_model::{MetadataKind, PropertyKey};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCommandItem {
    pub key: String,
    pub caption: PresentationText,
    pub primary: bool,
    pub origin: Option<PropertySource>,
}

pub(super) fn visible(command: Option<&str>) -> bool {
    // These commands require a distributed/standalone server context. The ordinary desktop
    // Configurator profile retains their XML nodes but excludes them from the visual bar.
    !matches!(
        command,
        Some(
            "Form.StandardCommand.OpenFromMainServer"
                | "Form.StandardCommand.OpenFromStandaloneServer"
        )
    )
}

pub(super) fn automatic(form: &ComposedForm, resources: &[FormResource]) -> Vec<AutoCommandItem> {
    if !autofill(&form.root) {
        return Vec::new();
    }
    let Some(attribute) = form.attributes.iter().find(|attribute| {
        attribute.properties.iter().any(|value| {
            value.property.key.name == "MainAttribute"
                && value.property.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
                && super::appearance::text(&value.property.value) == Some("true")
        })
    }) else {
        return Vec::new();
    };
    let types = definition_type(attribute);
    let Some(owner_type) = types.types.iter().find(|kind| {
        kind.namespace.as_deref() == Some(super::references::CONFIG)
            && kind.name.contains("Object.")
    }) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    if owner_type.name.starts_with("DocumentObject.") {
        result.extend([
            standard("PostAndClose", true),
            standard("Write", false),
            standard("Post", false),
        ]);
    } else if owner_type.name.starts_with("CatalogObject.") {
        result.extend([standard("WriteAndClose", true), standard("Write", false)]);
    }
    metadata_commands(owner_type, resources, &mut result);
    result
}

fn autofill(element: &EffectiveElement) -> bool {
    element
        .auxiliary
        .iter()
        .find(|node| node.kind == Some(ElementKind::AutoCommandBar))
        .and_then(|bar| super::presentation::property(bar, "Autofill"))
        .and_then(|value| super::appearance::text(&value.property.value))
        != Some("false")
}

pub(super) fn table(
    element: &EffectiveElement,
    value_type: &super::FieldType,
) -> Vec<AutoCommandItem> {
    if !autofill(element) {
        return Vec::new();
    }
    // ReadOnly does not remove these design-time buttons; no preview command executes.
    let dynamic = value_type.types.iter().any(|kind| {
        kind.namespace.as_deref() == Some(super::references::CONFIG) && kind.name == "DynamicList"
    });
    let commands: &[&str] = if dynamic {
        &["Create", "Copy"]
    } else {
        &["Add", "MoveUp", "MoveDown"]
    };
    commands
        .iter()
        .map(|name| AutoCommandItem {
            key: format!("Table.StandardCommand.{name}"),
            caption: PresentationText::Standard {
                key: format!("command.{name}"),
            },
            primary: false,
            origin: None,
        })
        .collect()
}

fn standard(name: &str, primary: bool) -> AutoCommandItem {
    AutoCommandItem {
        key: format!("Form.StandardCommand.{name}"),
        caption: PresentationText::Standard {
            key: format!("command.{name}"),
        },
        primary,
        origin: None,
    }
}

fn metadata_commands(
    owner_type: &PropertyKey,
    resources: &[FormResource],
    output: &mut Vec<AutoCommandItem>,
) {
    let Some((kind, name)) = owner_type.name.split_once('.') else {
        return;
    };
    let Some(kind) = super::resources::type_kind(kind) else {
        return;
    };
    let Some(owner) = resources
        .iter()
        .flat_map(|resource| &resource.descriptor.objects)
        .find(|object| object.metadata.kind() == kind && object.metadata.name() == name)
    else {
        return;
    };
    let parameter_type = owner_type.name.replacen("Object.", "Ref.", 1);
    let mut seen = HashSet::new();
    for resource in resources {
        for command in resource.descriptor.objects.iter().filter(|object| {
            object.metadata.kind() == MetadataKind::Command
                && object.metadata.parent() == Some(owner.metadata.id())
        }) {
            if !seen.insert(command.metadata.id()) {
                continue;
            }
            let group = metadata_property(command, "Group", resources)
                .and_then(|value| super::appearance::text(&value.value));
            if !matches!(group, Some("FormCommandBarImportant")) {
                continue;
            }
            let parameter = metadata_property(command, "CommandParameterType", resources)
                .map(|value| field_type(&value.value))
                .unwrap_or_default();
            if !parameter.types.is_empty()
                && !parameter.types.iter().any(|value| {
                    value.namespace == owner_type.namespace && value.name == parameter_type
                })
            {
                continue;
            }
            let effective = resources.iter().rev().find_map(|resource| {
                resource
                    .descriptor
                    .objects
                    .iter()
                    .find(|object| object.metadata.id() == command.metadata.id())
                    .map(|object| (resource, object))
            });
            let Some((resource, command)) = effective else {
                continue;
            };
            let caption = metadata_property(command, "Synonym", resources)
                .and_then(|value| literal(&value.value))
                .unwrap_or_else(|| PresentationText::Name {
                    text: command.metadata.name().into(),
                });
            output.push(AutoCommandItem {
                key: command.metadata.id().to_string(),
                caption,
                primary: false,
                origin: Some(PropertySource {
                    source: resource.source.clone(),
                    range: command.range.clone(),
                }),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{
        forms::{compose, parse},
        metadata_parser::{self, PropertiesMode},
    };
    fn resource(source: &str, commands: &str) -> FormResource {
        let xml = format!(
            "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"{}\"><Catalog uuid=\"owner\"><Properties><Name>Demo</Name></Properties><ChildObjects>{commands}</ChildObjects></Catalog></MetaDataObject>",
            super::super::references::CONFIG
        );
        FormResource {
            source: source.into(),
            descriptor: metadata_parser::parse(&xml, None, PropertiesMode::All).unwrap(),
            references: Vec::new(),
            picture: None,
            retained_bytes: 0,
        }
    }
    fn command(name: &str, title: &str, group: &str, parameter: &str) -> String {
        format!(
            "<Command uuid=\"{name}\"><Properties><Name>{name}</Name><Synonym><v8:item><v8:lang>ru</v8:lang><v8:content>{title}</v8:content></v8:item></Synonym><Group>{group}</Group><CommandParameterType><v8:Type>{parameter}</v8:Type></CommandParameterType></Properties></Command>"
        )
    }
    fn form(bar: &str) -> ComposedForm {
        let xml = format!(
            "<Form xmlns=\"{}\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"{}\"><AutoCommandBar name=\"Bar\" id=\"-1\">{bar}</AutoCommandBar><Attributes><Attribute name=\"Object\" id=\"1\"><Type><v8:Type>cfg:CatalogObject.Demo</v8:Type></Type><MainAttribute>true</MainAttribute></Attribute></Attributes></Form>",
            super::super::FORM_NAMESPACE,
            super::super::references::CONFIG
        );
        compose(&parse("form", xml).unwrap(), &[])
    }
    #[test]
    fn object_bar_matches_parameter_type_group_and_inherited_command_properties() {
        let commands = [
            command(
                "Run",
                "Выполнить",
                "FormCommandBarImportant",
                "cfg:CatalogRef.Demo",
            ),
            command(
                "Wrong",
                "Другой",
                "FormCommandBarImportant",
                "cfg:DocumentRef.Other",
            ),
            command(
                "Navigate",
                "Переход",
                "FormNavigationPanelGoTo",
                "cfg:CatalogRef.Demo",
            ),
        ]
        .join("");
        let resources = [
            resource("base", &commands),
            resource(
                "extension",
                "<Command uuid=\"Run\"><Properties><Name>Run</Name><Synonym><v8:item><v8:lang>ru</v8:lang><v8:content>Переопределено</v8:content></v8:item></Synonym></Properties></Command>",
            ),
        ];
        let result = automatic(&form(""), &resources);
        assert_eq!(result.len(), 3);
        assert!(result[0].primary);
        assert!(
            matches!(&result[2].caption,PresentationText::Literal {items} if items[0].content=="Переопределено")
        );
        assert_eq!(result[2].origin.as_ref().unwrap().source, "extension");
        assert!(automatic(&form("<Autofill>false</Autofill>"), &resources).is_empty());
        assert_eq!(
            automatic(&form("<AutoFill>false</AutoFill>"), &resources).len(),
            3,
            "the platform ignores this non-Designer spelling"
        );
    }
    #[test]
    fn unavailable_server_commands_keep_tree_identity_but_not_desktop_pixels() {
        let raw = form(
            "<ChildItems><Button name=\"Server\" id=\"1\"><CommandName>Form.StandardCommand.OpenFromMainServer</CommandName></Button></ChildItems>",
        );
        let presented = super::super::present(&raw, &[], &[]);
        assert_eq!(
            raw.root.auxiliary[0].children[0].name.as_deref(),
            Some("Server")
        );
        assert!(
            !presented
                .elements
                .iter()
                .find(|value| value.identity.key == super::super::ElementKey::Xml(1))
                .unwrap()
                .displayed
        );
    }
}
