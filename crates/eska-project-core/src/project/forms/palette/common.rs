//! Settings shared by the Designer palettes with the same source and default.

use super::{
    PaletteCategory::{Appearance, Layout, Main, Usage},
    PaletteDefinition as Row,
    PaletteEditor::{Boolean, Color, Dialog, Enum, Font, Localized, Number, Reference, Text},
    PlatformPropertyValue::{self, Auto, Empty, Unknown},
    SettingSource::{self, Attribute, Property},
};

pub(super) const CHECKED: PlatformPropertyValue = PlatformPropertyValue::Boolean(true);
pub(super) const UNCHECKED: PlatformPropertyValue = PlatformPropertyValue::Boolean(false);
pub(super) const ZERO: PlatformPropertyValue = PlatformPropertyValue::Number(0);
pub(super) const OPEN: PlatformPropertyValue = PlatformPropertyValue::Dialog;
pub(super) const TRISTATE: &[(&str, &str)] = &[("true", "yes"), ("false", "no")];
pub(super) const GROUPING: &[(&str, &str)] = &[
    ("Vertical", "vertical"),
    ("HorizontalIfPossible", "horizontalIfPossible"),
    ("AlwaysHorizontal", "alwaysHorizontal"),
];
pub(super) const NAME: Row = Row::new("Name", Main, Text, Unknown).with_source(SettingSource::Name);
pub(super) const TITLE: Row = Row::new("Title", Main, Localized, Empty);
pub(super) const VISIBLE: Row = Row::new("Visible", Main, Boolean, CHECKED);
pub(super) const USER_VISIBILITY: Row =
    Row::new("UserVisibility", Main, Dialog, OPEN).with_source(Property("UserVisible"));
pub(super) const ENABLED: Row = Row::new("Enabled", Main, Boolean, CHECKED);
pub(super) const READ_ONLY: Row = Row::new("ReadOnly", Main, Boolean, UNCHECKED);
pub(super) const ENABLE_CONTENT_CHANGE: Row =
    Row::new("EnableContentChange", Main, Boolean, CHECKED);
pub(super) const IMPORTANCE: Row = Row::new("DisplayImportance", Main, Enum, Auto)
    .with_source(Attribute("DisplayImportance"))
    .with_enum_captions(&[("High", "highImportance")]);
pub(super) const CURRENT_ROW_USE: Row = Row::new("CurrentRowUse", Main, Enum, Auto)
    .with_enum_captions(&[("Use", "usesCurrentRow"), ("DontUse", "ignoresCurrentRow")]);
pub(super) const ASSOCIATED_TABLE: Row = Row::new("AssociatedTable", Main, Reference, Empty)
    .with_source(Property("AssociatedTableElementId"));
pub(super) const TOOLTIP: Row = Row::new("ToolTip", Usage, Localized, Empty);
pub(super) const TOOLTIP_REPRESENTATION: Row = Row::new("ToolTipRepresentation", Usage, Enum, Auto);
pub(super) const TITLE_TEXT_COLOR: Row = Row::new("TitleTextColor", Appearance, Color, Auto);
pub(super) const TITLE_FONT: Row = Row::new("TitleFont", Appearance, Font, Auto);
pub(super) const WIDTH: Row = Row::new("Width", Layout, Number, ZERO);
pub(super) const AUTO_FILL: Row =
    Row::new("AutoFill", Main, Boolean, CHECKED).with_source(Property("Autofill"));

pub(super) const fn event(id: &'static str) -> Row {
    Row::new(
        id,
        super::PaletteCategory::Events,
        super::PaletteEditor::Event,
        Empty,
    )
    .with_source(SettingSource::Event)
}
