//! Authoring reads the same property vocabulary as the native inspector.

use super::{
    ElementKind, PaletteDefinition, PaletteEditor, PlatformPropertyValue, SettingSource, button,
    checkbox, command, common, display_field, input_field, pages, root_form, table,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthoringSource {
    Property,
    Qualifier,
    Name,
    Kind,
    Event,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringProperty {
    pub id: &'static str,
    pub name: &'static str,
    pub source: AuthoringSource,
    pub editor: PaletteEditor,
    pub default: PlatformPropertyValue,
    pub values: Vec<&'static str>,
    pub category: super::PaletteCategory,
    #[serde(skip)]
    pub enum_captions: &'static [(&'static str, &'static str)],
}

/// Includes conditional settings so clients may build a new element before it has a binding.
/// Applicability remains context dependent; existing undeclared defaults are never written eagerly.
#[must_use]
pub fn authoring_properties(kind: ElementKind) -> Vec<AuthoringProperty> {
    let definitions = definitions(kind);
    let mut output = Vec::new();
    for definition in definitions {
        let (source, name) = match definition.source {
            SettingSource::Property(name) => (AuthoringSource::Property, name),
            SettingSource::Attribute(name) => (AuthoringSource::Qualifier, name),
            SettingSource::Name => (AuthoringSource::Name, "name"),
            SettingSource::Kind => (AuthoringSource::Kind, "Type"),
            SettingSource::Event => (AuthoringSource::Event, definition.id),
            SettingSource::Unmapped => continue,
        };
        if output
            .iter()
            .any(|value: &AuthoringProperty| value.name == name && value.source == source)
        {
            continue;
        }
        let mut values: Vec<_> = definition
            .enum_captions
            .iter()
            .map(|(token, _)| *token)
            .collect();
        if matches!(
            definition.absent,
            PlatformPropertyValue::Auto | PlatformPropertyValue::AutoStretch
        ) && !values.is_empty()
            && !values.contains(&"Auto")
        {
            values.push("Auto");
        }
        output.push(AuthoringProperty {
            id: definition.id,
            name,
            source,
            editor: definition.editor,
            default: definition.absent,
            values,
            category: definition.category,
            enum_captions: definition.enum_captions,
        });
    }
    output
}

fn definitions(kind: ElementKind) -> Vec<&'static PaletteDefinition> {
    match kind {
        ElementKind::Form => root_form::authoring(),
        ElementKind::InputField => input_field::authoring(),
        ElementKind::CheckBoxField => checkbox::authoring(),
        ElementKind::LabelField | ElementKind::PictureField => display_field::authoring(),
        ElementKind::Button => button::authoring(),
        ElementKind::Table | ElementKind::ColumnGroup => {
            table::authoring(kind == ElementKind::Table)
        }
        ElementKind::Page | ElementKind::Pages => pages::definitions(kind == ElementKind::Pages)
            .map(|(row, _)| row)
            .collect(),
        ElementKind::UsualGroup => super::USUAL_GROUP.iter().collect(),
        ElementKind::ContextMenu => super::CONTEXT_MENU.iter().collect(),
        ElementKind::CommandBar
        | ElementKind::AutoCommandBar
        | ElementKind::ButtonGroup
        | ElementKind::Popup
        | ElementKind::SearchControlAddition
        | ElementKind::SearchStringAddition
        | ElementKind::ViewStatusAddition => command::authoring(kind),
        _ => vec![
            &common::NAME,
            &common::TITLE,
            &common::VISIBLE,
            &common::ENABLED,
            &common::READ_ONLY,
            &common::TOOLTIP,
            &common::WIDTH,
        ],
    }
}
