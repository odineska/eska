//! Designer 8.3.27 checkbox palettes for form fields and direct Boolean table columns.

use super::common::{CHECKED, OPEN, TRISTATE, UNCHECKED, ZERO};
use super::{
    PaletteCategory, PaletteDefinition, PaletteEditor, PlatformPropertyValue, SettingSource,
    unbound_data_path,
};
use crate::project::forms::{EffectiveElement, ElementPresentation, FieldValueKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Context {
    Unbound,
    Checkbox,
    Tumbler { equal_width: bool },
    Switcher,
    Column,
}

impl Context {
    pub(super) fn new(view: &ElementPresentation, element: &EffectiveElement) -> Option<Self> {
        match (view.in_table, view.value_type.kind) {
            (true, FieldValueKind::Boolean) => Some(Self::Column),
            (false, FieldValueKind::Boolean) => match element.scalar("CheckBoxType") {
                None | Some("Auto" | "CheckBox") => Some(Self::Checkbox),
                Some("Tumbler") => Some(Self::Tumbler {
                    equal_width: !matches!(element.scalar("EqualItemsWidth"), Some("false" | "0")),
                }),
                Some("Switcher") => Some(Self::Switcher),
                _ => None,
            },
            (false, FieldValueKind::Unknown) if unbound_data_path(element) => Some(Self::Unbound),
            _ => None,
        }
    }
}

pub(super) fn definitions(
    context: Context,
) -> impl Iterator<Item = (&'static PaletteDefinition, bool)> {
    let column = context == Context::Column;
    let unbound = context == Context::Unbound;
    let tumbler = matches!(context, Context::Tumbler { .. });
    let styled = tumbler || context == Context::Switcher;
    CHECKBOX.iter().filter_map(move |definition| {
        let definition = match definition.id {
            "Type" if unbound => &UNBOUND_TYPE,
            "CheckBoxType" if column || unbound => return None,
            "EditFormat" | "ItemTitleHeight" | "ItemHeight" | "EqualItemsWidth" if !tumbler => {
                return None;
            }
            "ItemWidth" if context != (Context::Tumbler { equal_width: true }) => return None,
            "FooterText"
            | "FooterDataPath"
            | "EditMode"
            | "AutoCellHeight"
            | "CellHyperlink"
            | "FixingInTable"
            | "ShowInHeader"
            | "ShowInFooter"
            | "HeaderPicture"
            | "FooterPicture"
            | "TitleBackColor"
            | "FooterBackColor"
            | "FooterTextColor"
            | "FooterFont"
            | "HorizontalAlign"
            | "HeaderHorizontalAlign"
            | "FooterHorizontalAlign"
                if !column =>
            {
                return None;
            }
            "HorizontalAlignInGroup" | "VerticalAlignInGroup" if column => return None,
            "BackColor" if !(column || styled) => return None,
            "TextColor" | "Font" if !styled => return None,
            _ => definition,
        };
        Some((definition, true))
    })
}

const UNBOUND_TYPE: PaletteDefinition = PaletteDefinition::new(
    "Type",
    PaletteCategory::Main,
    PaletteEditor::Enum,
    PlatformPropertyValue::Empty,
)
.with_source(SettingSource::Unmapped);

const CHECKBOX: &[PaletteDefinition] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteDefinition as Row;
    use PaletteEditor::{
        Binding, Boolean, Color, Dialog, Enum, Font, Localized, Number, Picture, Shortcut, Text,
    };
    use PlatformPropertyValue::{Auto, Choice, Empty, Unknown};
    use SettingSource::{Attribute, Property};

    const DEFAULT: PlatformPropertyValue = Choice("default");
    &[
        Row::new("Name", Main, Text, Unknown).with_source(SettingSource::Name),
        Row::new("Title", Main, Localized, Empty),
        Row::new("Type", Main, Enum, Unknown).with_source(SettingSource::Kind),
        Row::new("DataPath", Main, Binding, Empty),
        Row::new("TitleLocation", Main, Enum, Auto).with_enum_captions(&[("Top", "top")]),
        Row::new("Visible", Main, Boolean, CHECKED),
        Row::new("UserVisibility", Main, Dialog, OPEN).with_source(Property("UserVisible")),
        Row::new("Enabled", Main, Boolean, CHECKED),
        Row::new("ReadOnly", Main, Boolean, UNCHECKED),
        Row::new("SkipOnInput", Main, Enum, DEFAULT).with_enum_captions(TRISTATE),
        Row::new("DefaultItem", Main, Boolean, UNCHECKED),
        Row::new("DisplayImportance", Main, Enum, Auto)
            .with_source(Attribute("DisplayImportance"))
            .with_enum_captions(&[("High", "highImportance")]),
        Row::new("OnMainServerUnavailableBehavior", Main, Enum, Auto)
            .with_source(Property("OnMainServerUnavalableBehavior"))
            .with_enum_captions(&[("MakeDisable", "disableAvailability")]),
        Row::new("CheckBoxType", Main, Enum, Auto).with_enum_captions(&[
            ("Auto", "auto"),
            ("CheckBox", "checkbox"),
            ("Tumbler", "tumblers"),
            ("Switcher", "switcher"),
        ]),
        Row::new("EditFormat", Main, Dialog, Empty),
        Row::new("FooterText", Main, Localized, Empty),
        Row::new("FooterDataPath", Main, Binding, Empty),
        Row::new("WarningOnEditRepresentation", Usage, Enum, Auto),
        Row::new("WarningOnEdit", Usage, Localized, Empty),
        Row::new("EditMode", Usage, Enum, Choice("enter")),
        Row::new("AutoCellHeight", Usage, Boolean, UNCHECKED),
        Row::new("CellHyperlink", Usage, Boolean, UNCHECKED),
        Row::new("FixingInTable", Usage, Enum, Choice("no")),
        Row::new("Shortcut", Usage, Shortcut, Empty),
        Row::new("CommandSet", Usage, Dialog, OPEN),
        Row::new("ShowInHeader", Usage, Boolean, CHECKED),
        Row::new("ShowInFooter", Usage, Boolean, CHECKED),
        Row::new("ToolTip", Usage, Localized, Empty),
        Row::new("ToolTipRepresentation", Usage, Enum, Auto),
        Row::new("HeaderPicture", Appearance, Picture, Empty),
        Row::new("FooterPicture", Appearance, Picture, Empty),
        Row::new("TitleBackColor", Appearance, Color, Auto),
        Row::new("TitleTextColor", Appearance, Color, Auto),
        Row::new("TitleFont", Appearance, Font, Auto),
        Row::new("FooterBackColor", Appearance, Color, Auto),
        Row::new("FooterTextColor", Appearance, Color, Auto),
        Row::new("FooterFont", Appearance, Font, Auto),
        Row::new("BackColor", Appearance, Color, Auto),
        Row::new("BorderColor", Appearance, Color, Auto),
        Row::new("TextColor", Appearance, Color, Auto),
        Row::new("Font", Appearance, Font, Auto),
        Row::new("HorizontalAlignInGroup", Layout, Enum, Auto)
            .with_source(Property("GroupHorizontalAlign")),
        Row::new("VerticalAlignInGroup", Layout, Enum, Auto)
            .with_source(Property("GroupVerticalAlign")),
        Row::new("HorizontalAlign", Layout, Enum, Auto),
        Row::new("HeaderHorizontalAlign", Layout, Enum, Choice("left")),
        Row::new("FooterHorizontalAlign", Layout, Enum, Auto),
        Row::new("VerticalAlign", Layout, Enum, Auto),
        Row::new("ItemTitleHeight", Layout, Number, ZERO),
        Row::new("ItemHeight", Layout, Number, ZERO),
        Row::new("EqualItemsWidth", Layout, Enum, DEFAULT).with_enum_captions(TRISTATE),
        Row::new("ItemWidth", Layout, Number, ZERO),
        Row::new("TitleHeight", Layout, Number, ZERO),
        super::common::event("OnChange"),
    ]
};
