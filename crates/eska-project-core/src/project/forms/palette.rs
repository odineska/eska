//! The Designer palette describes applicable settings separately from declared XML.

mod button;
mod checkbox;
mod command;
mod common;
mod display_field;
mod input_field;
mod pages;
mod root_form;
mod table;

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::{
    EffectiveElement, ElementIdentity, ElementKind, FORM_NAMESPACE, FormPreview, SourcedProperty,
    SourcedQualifier, appearance, commands, handlers,
};
use crate::project::metadata_model::MetadataValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaletteCategory {
    Main,
    Usage,
    Appearance,
    Layout,
    Events,
}

impl PaletteCategory {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Usage => "usage",
            Self::Appearance => "appearance",
            Self::Layout => "layout",
            Self::Events => "events",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaletteEditor {
    Text,
    Localized,
    Boolean,
    Enum,
    Dialog,
    Binding,
    Reference,
    Shortcut,
    Color,
    Font,
    Number,
    Picture,
    Event,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum PlatformPropertyValue {
    Auto,
    AutoStretch,
    Boolean(bool),
    Number(i32),
    Choice(&'static str),
    Empty,
    Dialog,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct PaletteDefinition {
    pub id: &'static str,
    pub category: PaletteCategory,
    pub editor: PaletteEditor,
    source: SettingSource,
    absent: PlatformPropertyValue,
    enum_captions: &'static [(&'static str, &'static str)],
}

/// A display caption leaves the raw XML value available through its source property.
#[derive(Clone, Copy, Debug)]
pub enum PaletteCaption<'a> {
    Platform(&'static str),
    Identifier(&'a str),
    ScriptMember { item: &'a str, member: &'static str },
}

#[derive(Clone, Copy, Debug)]
enum SettingSource {
    Property(&'static str),
    Attribute(&'static str),
    Name,
    Kind,
    Event,
    Unmapped,
}

impl PaletteDefinition {
    const fn new(
        id: &'static str,
        category: PaletteCategory,
        editor: PaletteEditor,
        absent: PlatformPropertyValue,
    ) -> Self {
        Self {
            id,
            category,
            editor,
            source: SettingSource::Property(id),
            absent,
            enum_captions: &[],
        }
    }

    const fn with_source(mut self, source: SettingSource) -> Self {
        self.source = source;
        self
    }

    const fn with_enum_captions(
        mut self,
        captions: &'static [(&'static str, &'static str)],
    ) -> Self {
        self.enum_captions = captions;
        self
    }

    /// A display caption for a recognized Designer value.
    #[must_use]
    pub fn value_caption(self, value: &MetadataValue) -> Option<PaletteCaption<'_>> {
        if matches!(self.source, SettingSource::Property("AdditionSource")) {
            return command::source_caption(value);
        }
        self.text_caption(appearance::text(value)?)
    }

    #[must_use]
    pub fn text_caption(self, text: &str) -> Option<PaletteCaption<'_>> {
        if self.id == "CommandName" {
            return commands::local_command_name(text).map(PaletteCaption::Identifier);
        }
        if self.id == "CommandSource" {
            return text
                .strip_prefix("Item.")
                .filter(|name| !name.is_empty())
                .map(PaletteCaption::Identifier);
        }
        self.enum_captions.iter().find_map(|(token, caption)| {
            (*token == text).then_some(PaletteCaption::Platform(caption))
        })
    }
}

#[derive(Debug)]
pub enum PaletteValue<'a> {
    /// Points into this revision's effective XML properties. The occurrence is within the `QName`.
    Declared {
        index: usize,
        property: &'a SourcedProperty,
        occurrence: usize,
        extension: bool,
    },
    /// An entry of the effective Events property, with navigation to the real XML wrapper.
    DeclaredEvent {
        index: usize,
        entry_index: usize,
        occurrence: usize,
        extension: bool,
    },
    Qualifier {
        attribute: &'a SourcedQualifier,
        extension: bool,
    },
    Name(Option<&'a str>),
    Kind(ElementKind),
    Absent(PlatformPropertyValue),
}

#[derive(Debug)]
pub struct PaletteRow<'a> {
    pub definition: Option<&'static PaletteDefinition>,
    pub value: PaletteValue<'a>,
    pub available: bool,
}

#[derive(Debug)]
pub struct PropertyPalette<'a> {
    /// None signals that a native palette for this element context is not yet established.
    pub rows: Option<Vec<PaletteRow<'a>>>,
}

impl FormPreview {
    /// Inspect one selection without adding synthesized defaults to any form element.
    #[must_use]
    pub fn property_palette(&self, identity: &ElementIdentity) -> Option<PropertyPalette<'_>> {
        let element = self.form.root.find(identity)?;
        let presentation = || {
            self.presentation
                .elements
                .iter()
                .find(|item| &item.identity == identity)
        };
        let rows = match element.kind {
            Some(ElementKind::Form) if identity == &self.form.root.identity => {
                root_form::Context::new(self).map(|context| {
                    self.palette_rows(element, root_form::definitions(context, element))
                })
            }
            Some(ElementKind::InputField) => presentation()
                .and_then(|item| input_field::Context::new(item, element))
                .map(|context| self.palette_rows(element, input_field::definitions(context))),
            Some(ElementKind::LabelField | ElementKind::PictureField) => presentation()
                .map(|item| self.palette_rows(element, display_field::definitions(item, element))),
            Some(ElementKind::ContextMenu) => {
                Some(self.palette_rows(element, CONTEXT_MENU.iter().map(|row| (row, true))))
            }
            Some(ElementKind::Button) => presentation()
                .and_then(|item| button::Context::new(self, item, element))
                .map(|context| self.palette_rows(element, button::definitions(context))),
            Some(ElementKind::CheckBoxField) => presentation()
                .and_then(|item| checkbox::Context::new(item, element))
                .map(|context| self.palette_rows(element, checkbox::definitions(context))),
            Some(ElementKind::Table) => presentation()
                .and_then(|item| table::table_definitions(item, element))
                .map(|definitions| self.palette_rows(element, definitions)),
            Some(ElementKind::ColumnGroup) => presentation()
                .and_then(|item| table::column_group_definitions(item, element))
                .map(|definitions| self.palette_rows(element, definitions)),
            Some(kind @ (ElementKind::Pages | ElementKind::Page)) => {
                Some(self.palette_rows(element, pages::definitions(kind == ElementKind::Pages)))
            }
            Some(ElementKind::UsualGroup) => usual_group_definitions(element)
                .map(|definitions| self.palette_rows(element, definitions)),
            Some(ElementKind::AutoCommandBar) => presentation()
                .and_then(command::auto_bar_definitions)
                .map(|definitions| self.palette_rows(element, definitions)),
            Some(ElementKind::CommandBar | ElementKind::ButtonGroup | ElementKind::Popup) => {
                presentation()
                    .and_then(|item| command::group_definitions(item, element))
                    .map(|definitions| self.palette_rows(element, definitions))
            }
            Some(
                ElementKind::SearchStringAddition
                | ElementKind::SearchControlAddition
                | ElementKind::ViewStatusAddition,
            ) => presentation()
                .and_then(|item| command::addition_definitions(item, element))
                .map(|definitions| self.palette_rows(element, definitions)),
            _ => None,
        };
        Some(PropertyPalette { rows })
    }

    fn palette_rows<'a>(
        &'a self,
        element: &'a EffectiveElement,
        catalog: impl IntoIterator<Item = (&'static PaletteDefinition, bool)>,
    ) -> Vec<PaletteRow<'a>> {
        let extension_sources: HashSet<_> = self
            .dependencies
            .iter()
            .filter(|dependency| dependency.extension_form)
            .map(|dependency| dependency.id.as_str())
            .collect();
        let properties: Vec<_> = self.form.inspectable_properties(element).collect();
        let mut taken_properties = vec![false; properties.len()];
        let mut taken_attributes = vec![false; element.qualifiers.len()];
        let declared = |index: usize, occurrence| PaletteValue::Declared {
            index,
            property: properties[index],
            occurrence,
            extension: extension_sources.contains(properties[index].origin.source.as_str()),
        };
        let qualifier = |attribute: &'a SourcedQualifier| PaletteValue::Qualifier {
            attribute,
            extension: extension_sources.contains(attribute.origin.source.as_str()),
        };
        let mut rows = Vec::new();
        for (definition, available) in catalog {
            let missing = || PaletteValue::Absent(definition.absent);
            let value = match definition.source {
                SettingSource::Property(name) => properties
                    .iter()
                    .position(|property| property.has_form_name(name))
                    .map_or_else(missing, |index| {
                        taken_properties[index] = true;
                        declared(index, 0)
                    }),
                SettingSource::Attribute(name) => element
                    .qualifiers
                    .iter()
                    .enumerate()
                    .find(|(_, value)| value.key.namespace.is_none() && value.key.name == name)
                    .map_or_else(missing, |(index, attribute)| {
                        taken_attributes[index] = true;
                        qualifier(attribute)
                    }),
                SettingSource::Name => PaletteValue::Name(element.name.as_deref()),
                SettingSource::Kind => element.kind.map_or_else(missing, PaletteValue::Kind),
                SettingSource::Event => event_entry(element, definition.id).map_or_else(
                    missing,
                    |(index, occurrence, entry_index)| PaletteValue::DeclaredEvent {
                        index,
                        occurrence,
                        entry_index,
                        extension: extension_sources
                            .contains(element.properties[index].origin.source.as_str()),
                    },
                ),
                SettingSource::Unmapped => missing(),
            };
            rows.push(PaletteRow {
                definition: Some(definition),
                value,
                available,
            });
        }
        let mut occurrences = HashMap::new();
        for (index, property) in properties.iter().enumerate() {
            let key = &property.property.key;
            let occurrence = occurrences
                .entry((key.namespace.as_deref(), key.name.as_str()))
                .or_insert(0);
            if !taken_properties[index] {
                rows.push(PaletteRow {
                    definition: None,
                    value: declared(index, *occurrence),
                    available: true,
                });
            }
            *occurrence += 1;
        }
        rows.extend(
            element
                .qualifiers
                .iter()
                .enumerate()
                .filter_map(|(index, attribute)| {
                    let key = &attribute.key;
                    if taken_attributes[index]
                        || (key.namespace.is_none() && matches!(key.name.as_str(), "name" | "id"))
                    {
                        return None;
                    }
                    Some(PaletteRow {
                        definition: None,
                        value: qualifier(attribute),
                        available: true,
                    })
                }),
        );
        rows
    }
}

fn event_entry(element: &EffectiveElement, name: &str) -> Option<(usize, usize, usize)> {
    element
        .properties
        .iter()
        .enumerate()
        .filter(|(_, value)| {
            value.property.key.namespace.as_deref() == Some(FORM_NAMESPACE)
                && value.property.key.name == "Events"
        })
        .enumerate()
        .find_map(|(occurrence, (index, value))| {
            let MetadataValue::Record(events) = &value.property.value else {
                return None;
            };
            let entry_index = events.iter().position(|event| {
                matches!(event.value, MetadataValue::Text(_))
                    && handlers::event_name(event) == Some(name)
            })?;
            Some((index, occurrence, entry_index))
        })
}

fn usual_group_definitions(
    element: &EffectiveElement,
) -> Option<impl Iterator<Item = (&'static PaletteDefinition, bool)>> {
    let collapsible = match element.scalar("Behavior") {
        Some("Collapsible") => true,
        None | Some("Auto" | "Usual") => false,
        _ => return None,
    };
    Some(
        USUAL_GROUP
            .iter()
            .filter(move |row| match row.id {
                "CollapsedRepresentationTitle"
                | "Collapsed"
                | "ControlRepresentation"
                | "ShowLeftMargin"
                | "HiddenRepresentationTitleBackColor" => collapsible,
                "ThroughAlign" => !collapsible,
                _ => true,
            })
            .map(|row| (row, true)),
    )
}

const CONTEXT_MENU: &[PaletteDefinition] = &[common::IMPORTANCE, common::AUTO_FILL];

fn unbound_data_path(element: &EffectiveElement) -> bool {
    element
        .property("DataPath")
        .is_none_or(|property| property.text() == Some(""))
}

const USUAL_GROUP: &[PaletteDefinition] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteEditor::{Binding, Boolean, Color, Enum, Font, Localized, Number, Shortcut, Text};
    use PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty, Unknown};
    &[
        PaletteDefinition::new("Name", Main, Text, Unknown).with_source(SettingSource::Name),
        PaletteDefinition::new("Title", Main, Localized, Unknown),
        PaletteDefinition::new("Type", Main, Enum, Unknown).with_source(SettingSource::Kind),
        PaletteDefinition::new(
            "Visible",
            Main,
            Boolean,
            PlatformPropertyValue::Boolean(true),
        ),
        PaletteDefinition::new(
            "UserVisibility",
            Main,
            PaletteEditor::Dialog,
            PlatformPropertyValue::Dialog,
        )
        .with_source(SettingSource::Property("UserVisible")),
        PaletteDefinition::new(
            "Enabled",
            Main,
            Boolean,
            PlatformPropertyValue::Boolean(true),
        ),
        PaletteDefinition::new(
            "ReadOnly",
            Main,
            Boolean,
            PlatformPropertyValue::Boolean(false),
        ),
        common::ENABLE_CONTENT_CHANGE,
        PaletteDefinition::new("DisplayImportance", Main, Enum, Auto)
            .with_source(SettingSource::Attribute("DisplayImportance")),
        PaletteDefinition::new("Behavior", Main, Enum, Auto).with_enum_captions(&[
            ("Auto", "auto"),
            ("Usual", "regularGroupBehavior"),
            ("Collapsible", "collapsibleGroupBehavior"),
        ]),
        PaletteDefinition::new("CollapsedRepresentationTitle", Main, Localized, Empty),
        PaletteDefinition::new("Collapsed", Main, Boolean, common::UNCHECKED),
        PaletteDefinition::new(
            "ControlRepresentation",
            Main,
            Enum,
            Choice("titleHyperlink"),
        )
        .with_enum_captions(&[("Picture", "groupControlPicture")]),
        PaletteDefinition::new("Representation", Main, Enum, Choice("lightMarking"))
            .with_enum_captions(&[
                ("None", "none"),
                ("NormalSeparation", "normalMarking"),
                ("StrongSeparation", "strongMarking"),
            ]),
        PaletteDefinition::new("ShowLeftMargin", Main, Boolean, common::CHECKED),
        PaletteDefinition::new("Group", Main, Enum, Choice("horizontalIfPossible"))
            .with_enum_captions(common::GROUPING),
        PaletteDefinition::new(
            "United",
            Main,
            Boolean,
            PlatformPropertyValue::Boolean(super::DEFAULT_GROUP_UNITED),
        ),
        PaletteDefinition::new("ThroughAlign", Main, Enum, Auto),
        PaletteDefinition::new("TitleDataPath", Main, Binding, Empty),
        common::CURRENT_ROW_USE,
        common::ASSOCIATED_TABLE,
        PaletteDefinition::new("Shortcut", Usage, Shortcut, Empty),
        PaletteDefinition::new("ToolTip", Usage, Localized, Empty),
        PaletteDefinition::new("ToolTipRepresentation", Usage, Enum, Auto),
        PaletteDefinition::new("ItemsAndTitlesAlign", Usage, Enum, Auto)
            .with_source(SettingSource::Property("ChildrenAlign")),
        PaletteDefinition::new(
            "ShowTitle",
            Usage,
            Boolean,
            PlatformPropertyValue::Boolean(true),
        ),
        PaletteDefinition::new("TitleTextColor", Appearance, Color, Auto),
        PaletteDefinition::new("TitleFont", Appearance, Font, Auto),
        PaletteDefinition::new("HorizontalSpacing", Appearance, Enum, Auto),
        PaletteDefinition::new("VerticalSpacing", Appearance, Enum, Auto),
        PaletteDefinition::new("BackColor", Appearance, Color, Auto),
        PaletteDefinition::new(
            "HiddenRepresentationTitleBackColor",
            Appearance,
            Color,
            Auto,
        )
        .with_source(SettingSource::Property("HiddenStateTitleBackColor")),
        PaletteDefinition::new("Width", Layout, Number, PlatformPropertyValue::Number(0)),
        PaletteDefinition::new("Height", Layout, Number, PlatformPropertyValue::Number(0)),
        PaletteDefinition::new("HorizontalAlignInGroup", Layout, Enum, Auto)
            .with_source(SettingSource::Property("GroupHorizontalAlign")),
        PaletteDefinition::new("VerticalAlignInGroup", Layout, Enum, Auto)
            .with_source(SettingSource::Property("GroupVerticalAlign")),
        PaletteDefinition::new("ChildItemsHorizontalAlign", Layout, Enum, Auto)
            .with_source(SettingSource::Property("HorizontalAlign")),
        PaletteDefinition::new("ChildItemsVerticalAlign", Layout, Enum, Auto)
            .with_source(SettingSource::Property("VerticalAlign")),
        PaletteDefinition::new("HorizontalStretch", Layout, Enum, AutoStretch),
        PaletteDefinition::new("VerticalStretch", Layout, Enum, AutoStretch),
    ]
};
