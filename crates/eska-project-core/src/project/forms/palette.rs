//! The Designer palette describes applicable settings separately from declared XML.

mod button;
mod checkbox;
mod input_field;
mod root_form;

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::{
    EffectiveElement, ElementIdentity, ElementKind, FORM_NAMESPACE, FormPreview, SourcedProperty,
    SourcedQualifier, appearance, commands, handlers, presentation,
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
        self.text_caption(appearance::text(value)?)
    }

    #[must_use]
    pub fn text_caption(self, text: &str) -> Option<PaletteCaption<'_>> {
        if self.id == "CommandName" {
            return commands::local_command_name(text).map(PaletteCaption::Identifier);
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
        let rows = match element.kind {
            Some(ElementKind::Form) if identity == &self.form.root.identity => {
                root_form::Context::new(self).map(|context| {
                    self.palette_rows(element, root_form::definitions(context, element))
                })
            }
            Some(ElementKind::InputField) => self
                .presentation
                .elements
                .iter()
                .find(|item| &item.identity == identity)
                .and_then(|item| input_field::Context::new(item, element))
                .map(|context| self.palette_rows(element, input_field::definitions(context))),
            Some(ElementKind::Button) => self
                .presentation
                .elements
                .iter()
                .find(|item| &item.identity == identity)
                .and_then(|item| button::Context::new(&self.form, item, element))
                .map(|context| self.palette_rows(element, button::definitions(context))),
            Some(ElementKind::CheckBoxField) => self
                .presentation
                .elements
                .iter()
                .find(|item| &item.identity == identity)
                .and_then(|item| checkbox::Context::new(item, element))
                .map(|context| self.palette_rows(element, checkbox::definitions(context))),
            _ => ordinary_vertical_group(element)
                .then(|| self.palette_rows(element, USUAL_GROUP.iter().map(|row| (row, true)))),
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
                    .position(|property| {
                        property.property.key.namespace.as_deref() == Some(FORM_NAMESPACE)
                            && property.property.key.name == name
                    })
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

fn ordinary_vertical_group(element: &EffectiveElement) -> bool {
    // This catalog was captured for an ordinary vertical group. Specialized
    // behavior can introduce other properties and change their availability.
    element.kind == Some(ElementKind::UsualGroup)
        && matches!(
            setting_text(element, "Behavior"),
            None | Some("Auto" | "Usual")
        )
        && setting_text(element, "Group") == Some("Vertical")
}

fn setting_text<'a>(element: &'a EffectiveElement, name: &str) -> Option<&'a str> {
    appearance::text(&presentation::property(element, name)?.property.value)
}

const USUAL_GROUP: &[PaletteDefinition] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteEditor::{
        Binding, Boolean, Color, Enum, Font, Localized, Number, Reference, Shortcut, Text,
    };
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
        PaletteDefinition::new(
            "EnableContentChange",
            Main,
            Boolean,
            PlatformPropertyValue::Boolean(true),
        ),
        PaletteDefinition::new("DisplayImportance", Main, Enum, Auto)
            .with_source(SettingSource::Attribute("DisplayImportance")),
        PaletteDefinition::new("Behavior", Main, Enum, Auto),
        PaletteDefinition::new("Representation", Main, Enum, Choice("lightMarking")),
        PaletteDefinition::new("Group", Main, Enum, Unknown),
        PaletteDefinition::new(
            "United",
            Main,
            Boolean,
            PlatformPropertyValue::Boolean(super::DEFAULT_GROUP_UNITED),
        ),
        PaletteDefinition::new("ThroughAlign", Main, Enum, Auto),
        PaletteDefinition::new("TitleDataPath", Main, Binding, Empty),
        PaletteDefinition::new("CurrentRowUse", Main, Enum, Auto),
        PaletteDefinition::new("AssociatedTable", Main, Reference, Empty)
            .with_source(SettingSource::Unmapped),
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
