//! Static desktop Configurator commands; preview buttons never execute application actions.

use super::presentation::{definition_caption, literal, metadata_property};
use super::{
    AppearanceDiagnostic, ButtonRepresentation, ComposedForm, EffectiveDefinition,
    EffectiveElement, ElementFamily, ElementKind, FORM_NAMESPACE, FormPicture, FormResource,
    PresentationText, PropertySource, appearance::text, pictures,
};
use crate::project::{
    metadata_model::{MetadataValue, ObjectId},
    metadata_parser::ParsedObject,
};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCommandItem {
    pub key: String,
    pub caption: PresentationText,
    pub primary: bool,
    pub origin: Option<PropertySource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<FormPicture>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_representation: Option<ButtonRepresentation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by_caption: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Self>,
}

impl AutoCommandItem {
    pub(super) fn standard(scope: &str, name: &str, primary: bool) -> Self {
        Self {
            key: format!("{scope}.StandardCommand.{name}"),
            caption: PresentationText::Standard {
                key: format!("command.{name}"),
            },
            primary,
            origin: None,
            picture: None,
            button_representation: None,
            order_by_caption: None,
            items: Vec::new(),
        }
    }
}

pub(super) enum ResolvedCommand<'a> {
    Local(&'a EffectiveDefinition),
    Metadata(&'a ParsedObject),
    Standard(&'a str),
}

impl<'a> ResolvedCommand<'a> {
    pub(super) fn resolve(
        reference: &'a str,
        form: &'a ComposedForm,
        resources: &'a [FormResource],
    ) -> Option<Self> {
        if let Some(name) = local_command_name(reference) {
            return local_command(form, name).map(Self::Local);
        }
        if let Some(id) = super::references::metadata_command_id(reference) {
            return effective_metadata(&id, resources).map(|(_, command)| Self::Metadata(command));
        }
        reference
            .split_once(".StandardCommand.")
            .map(|(_, key)| Self::Standard(key))
    }

    pub(super) fn caption(&self, resources: &[FormResource]) -> Option<PresentationText> {
        match self {
            Self::Local(command) => definition_caption(command),
            Self::Metadata(command) => Some(metadata_caption(command, resources)),
            Self::Standard(key) => Some(PresentationText::Standard {
                key: format!("command.{key}"),
            }),
        }
    }

    pub(super) fn picture(
        &self,
        resources: &[FormResource],
        diagnostics: &mut Vec<AppearanceDiagnostic>,
    ) -> Option<FormPicture> {
        match self {
            Self::Local(command) => {
                pictures::form_picture(command.property("Picture")?, resources, diagnostics)
            }
            Self::Metadata(command) => pictures::metadata_picture(command, resources, diagnostics),
            Self::Standard(_) => None,
        }
    }

    pub(super) fn representation<'r>(&self, resources: &'r [FormResource]) -> Option<&'r str> {
        match self {
            Self::Metadata(command) => metadata_property(command, "Representation", resources)
                .and_then(|property| text(&property.value)),
            _ => None,
        }
    }

    pub(super) fn item(
        &self,
        key: String,
        origin: Option<PropertySource>,
        resources: &[FormResource],
        diagnostics: &mut Vec<AppearanceDiagnostic>,
    ) -> Option<AutoCommandItem> {
        let caption = self.caption(resources)?;
        let picture = self.picture(resources, diagnostics);
        let button_representation = picture.as_ref().map(|picture| {
            ButtonRepresentation::effective(
                Some(picture),
                None,
                self.representation(resources),
                ButtonRepresentation::Picture,
            )
        });
        Some(AutoCommandItem {
            key,
            caption,
            primary: false,
            origin,
            picture,
            button_representation,
            order_by_caption: None,
            items: Vec::new(),
        })
    }
}

pub(super) fn local_command_name(command: &str) -> Option<&str> {
    command
        .strip_prefix("Form.Command.")
        .filter(|name| !name.trim().is_empty())
}

pub(super) fn local_command<'a>(
    form: &'a ComposedForm,
    name: &str,
) -> Option<&'a EffectiveDefinition> {
    form.commands
        .iter()
        .find(|command| command.name.as_deref() == Some(name))
}

pub(super) fn visible(tables: &TableIndex<'_>, command: Option<&str>) -> bool {
    // These commands require a distributed/standalone server context. The ordinary desktop
    // Configurator profile retains their XML nodes but excludes them from the visual bar.
    if matches!(
        command,
        Some(
            "Form.StandardCommand.OpenFromMainServer"
                | "Form.StandardCommand.OpenFromStandaloneServer"
        )
    ) {
        return false;
    }
    let Some((table, name)) = command
        .and_then(|value| value.strip_prefix("Form.Item."))
        .and_then(|value| value.split_once(".StandardCommand."))
    else {
        return true;
    };
    !tables.get(table).is_some_and(|table| excludes(table, name))
}

pub(super) fn autofill(element: &EffectiveElement) -> bool {
    !matches!(
        element
            .auxiliary
            .iter()
            .find(|node| node.kind == Some(ElementKind::AutoCommandBar))
            .and_then(|bar| bar.scalar("Autofill")),
        Some("false" | "0")
    )
}

pub(super) fn table(
    element: &EffectiveElement,
    value_type: &super::FieldType,
) -> Vec<AutoCommandItem> {
    if !autofill(element) {
        return Vec::new();
    }
    table_items(element, value_type)
}

pub(super) type TableIndex<'a> = HashMap<&'a str, &'a EffectiveElement>;

pub(super) fn table_index(root: &EffectiveElement) -> TableIndex<'_> {
    fn visit<'a>(element: &'a EffectiveElement, tables: &mut TableIndex<'a>) {
        if element.kind == Some(ElementKind::Table)
            && let Some(name) = element.name.as_deref()
        {
            tables.entry(name).or_insert(element);
        }
        for child in &element.children {
            visit(child, tables);
        }
    }

    let mut tables = TableIndex::new();
    visit(root, &mut tables);
    tables
}

fn declares_columns(element: &EffectiveElement) -> bool {
    element.children.iter().any(|child| {
        child.kind.is_some_and(|kind| {
            kind.family() == ElementFamily::Field
                || kind == ElementKind::ColumnGroup && declares_columns(child)
        })
    })
}

/// A standalone command bar can draw table actions independently of the table's own bar.
pub(super) fn table_source<'a>(
    bar: &EffectiveElement,
    tables: &TableIndex<'a>,
) -> Option<&'a EffectiveElement> {
    let name = bar.scalar("CommandSource")?.strip_prefix("Item.")?;
    tables
        .get(name)
        .copied()
        .filter(|table| declares_columns(table))
}

fn excludes(table: &EffectiveElement, command: &str) -> bool {
    let Some(set) = table.property("CommandSet") else {
        return false;
    };
    let MetadataValue::Record(fields) = &set.property.value else {
        return false;
    };
    fields.iter().any(|field| {
        field.key.namespace.as_deref() == Some(FORM_NAMESPACE)
            && field.key.name == "ExcludedCommand"
            && text(&field.value) == Some(command)
    })
}

pub(super) fn table_items(
    element: &EffectiveElement,
    value_type: &super::FieldType,
) -> Vec<AutoCommandItem> {
    // ReadOnly does not remove these design-time buttons; no preview command executes.
    let dynamic = value_type.types.iter().any(|kind| {
        kind.namespace.as_deref() == Some(super::references::CONFIG) && kind.name == "DynamicList"
    });
    let commands: &[&str] = if dynamic && element.scalar("SearchStringLocation") == Some("None") {
        &["Create", "Copy", "Find", "CancelSearch"]
    } else if dynamic {
        &["Create", "Copy"]
    } else {
        &["Add", "MoveUp", "MoveDown"]
    };
    commands
        .iter()
        .filter(|name| !excludes(element, name))
        .map(|name| AutoCommandItem::standard("Table", name, false))
        .collect()
}

pub(super) fn effective_metadata<'a>(
    id: &ObjectId,
    resources: &'a [FormResource],
) -> Option<(&'a FormResource, &'a ParsedObject)> {
    resources.iter().rev().find_map(|resource| {
        resource
            .descriptor
            .objects
            .iter()
            .find(|object| object.metadata.id() == id)
            .map(|object| (resource, object))
    })
}

pub(super) fn metadata_caption(
    command: &ParsedObject,
    resources: &[FormResource],
) -> PresentationText {
    metadata_property(command, "Synonym", resources)
        .and_then(|value| literal(&value.value))
        .unwrap_or_else(|| PresentationText::Name {
            text: command.metadata.name().into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{
        forms::{compose, parse, root_commands},
        metadata_parser::{self, PropertiesMode},
    };
    fn resource(source: &str, commands: &str) -> FormResource {
        let xml = format!(
            "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\" xmlns:cfg=\"{}\"><Catalog uuid=\"owner\"><Properties><Name>Demo</Name></Properties><ChildObjects>{commands}</ChildObjects></Catalog></MetaDataObject>",
            super::super::references::CONFIG
        );
        FormResource {
            layer_source: "base".into(),
            style: None,
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
    fn application_actions_precede_object_actions_in_the_main_bar() {
        let parse_descriptor = |source: &str, object: &str| {
            FormResource {
                descriptor: metadata_parser::parse(
                    &format!(r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="{}">{object}</MetaDataObject>"#, super::super::references::CONFIG),
                    None,
                    PropertiesMode::All,
                ).unwrap(),
                ..resource(source, "")
            }
        };
        let resources = [
            parse_descriptor(
                "root",
                r#"<Configuration uuid="base"><Properties><Name>Base</Name></Properties><ChildObjects><CommonCommand>Global</CommonCommand></ChildObjects></Configuration>"#,
            ),
            parse_descriptor(
                "global",
                r#"<CommonCommand uuid="global"><Properties><Name>Global</Name><Group>FormCommandBarImportant</Group><CommandParameterType><v8:Type>cfg:CatalogRef.Demo</v8:Type></CommandParameterType></Properties></CommonCommand>"#,
            ),
            resource(
                "owner",
                &command(
                    "Owned",
                    "Объект",
                    "FormCommandBarImportant",
                    "cfg:CatalogRef.Demo",
                ),
            ),
        ];
        let commands = root_commands::project(&form(""), &resources, &mut Vec::new());
        assert_eq!(commands.inline[2].key, "common-command:Global");
        assert_eq!(commands.inline[3].key, "catalog:Demo/command:Owned");
    }

    #[test]
    fn object_bar_matches_parameter_type_group_and_inherited_command_properties() {
        let commands = [
            command(
                "Run",
                "Выполнить",
                "FormCommandBarImportant",
                "cfg:CatalogRef.Demo",
            )
            .replace(
                "</Properties>",
                "<Picture><xr:Ref>StdPicture.Post</xr:Ref></Picture></Properties>",
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
                "<Command uuid=\"Run\"><Properties><Name>Run</Name><Synonym><v8:item><v8:lang>ru</v8:lang><v8:content>Переопределено</v8:content></v8:item></Synonym><Representation>PictureAndText</Representation></Properties></Command>",
            ),
        ];
        let composed = form("");
        let auto =
            |form: &ComposedForm| root_commands::project(form, &resources, &mut Vec::new()).inline;
        let result = auto(&composed);
        assert_eq!(result.len(), 3);
        assert!(result[0].primary);
        assert!(
            matches!(&result[2].caption,PresentationText::Literal {items} if items[0].content=="Переопределено")
        );
        assert_eq!(result[2].origin.as_ref().unwrap().source, "extension");
        assert!(
            matches!(&result[2].picture, Some(FormPicture::Standard { name }) if name == "Post")
        );
        assert_eq!(
            result[2].button_representation,
            Some(ButtonRepresentation::PictureAndText)
        );
        assert!(matches!(
            ResolvedCommand::resolve("Catalog.Demo.Command.Run", &composed, &resources)
                .and_then(|command| command.caption(&resources)),
            Some(PresentationText::Literal { items }) if items[0].content == "Переопределено"
        ));
        assert!(
            ResolvedCommand::resolve("Report.Demo.Command.Run", &composed, &resources).is_none()
        );
        assert!(auto(&form("<Autofill>false</Autofill>")).is_empty());
        assert_eq!(
            auto(&form("<AutoFill>false</AutoFill>")).len(),
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
