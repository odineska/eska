//! Label and picture fields share a palette; rules select their native differences.

use super::{
    PaletteCategory::{Appearance, Layout, Main, Usage},
    PaletteDefinition as Row,
    PaletteEditor::{
        Binding, Boolean, Color, Dialog, Enum, Font, Localized, Number, Picture, Shortcut,
    },
    PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty, Unknown},
    SettingSource,
    common::{
        CHECKED, ENABLED, IMPORTANCE, NAME, OPEN, READ_ONLY, TITLE, TITLE_FONT, TITLE_TEXT_COLOR,
        TOOLTIP, TOOLTIP_REPRESENTATION, UNCHECKED, USER_VISIBILITY, VISIBLE, WIDTH, ZERO, event,
    },
};
use crate::project::forms::{EffectiveElement, ElementKind, ElementPresentation, FieldValueKind};

pub(super) fn definitions<'a>(
    view: &'a ElementPresentation,
    element: &'a EffectiveElement,
) -> impl Iterator<Item = (&'static Row, bool)> + 'a {
    let picture = element.kind == Some(ElementKind::PictureField);
    ROWS.iter()
        .filter(move |(_, rule)| match rule {
            Rule::Always => true,
            Rule::Label => !picture,
            Rule::Picture => picture,
            Rule::Column => view.in_table,
            Rule::Outside => !view.in_table,
            Rule::Format => {
                !picture
                    && matches!(
                        view.value_type.kind,
                        FieldValueKind::Number
                            | FieldValueKind::Date
                            | FieldValueKind::Boolean
                            | FieldValueKind::Composite
                    )
            }
            Rule::Numeric => {
                !picture
                    && matches!(
                        view.value_type.kind,
                        FieldValueKind::Number | FieldValueKind::Composite
                    )
            }
            Rule::MaxWidth => matches!(element.scalar("AutoMaxWidth"), Some("false" | "0")),
            Rule::MaxHeight => matches!(element.scalar("AutoMaxHeight"), Some("false" | "0")),
        })
        .map(move |(row, _)| {
            (
                row,
                row.id != "FileDragMode"
                    || matches!(element.scalar("EnableDrag"), Some("true" | "1")),
            )
        })
}

enum Rule {
    Always,
    Label,
    Picture,
    Column,
    Outside,
    Format,
    Numeric,
    MaxWidth,
    MaxHeight,
}

const ROWS: &[(Row, Rule)] = &[
    (NAME, Rule::Always),
    (TITLE, Rule::Always),
    (
        Row::new("Type", Main, Enum, Unknown).with_source(SettingSource::Kind),
        Rule::Always,
    ),
    (Row::new("DataPath", Main, Binding, Empty), Rule::Always),
    (Row::new("TitleLocation", Main, Enum, Auto), Rule::Always),
    (VISIBLE, Rule::Always),
    (USER_VISIBILITY, Rule::Always),
    (ENABLED, Rule::Always),
    (READ_ONLY, Rule::Always),
    (Row::new("SkipOnInput", Main, Enum, Auto), Rule::Always),
    (
        Row::new("DefaultItem", Main, Boolean, UNCHECKED),
        Rule::Always,
    ),
    (IMPORTANCE, Rule::Always),
    (
        Row::new("OnMainServerUnavailableBehavior", Main, Enum, Auto)
            .with_source(SettingSource::Property("OnMainServerUnavalableBehavior")),
        Rule::Always,
    ),
    (Row::new("Format", Main, Dialog, Empty), Rule::Format),
    (
        Row::new("ValuesPicture", Main, Picture, Empty),
        Rule::Picture,
    ),
    (
        Row::new("Hyperlink", Main, Boolean, UNCHECKED),
        Rule::Always,
    ),
    (Row::new("FooterText", Main, Localized, Empty), Rule::Column),
    (
        Row::new("FooterDataPath", Main, Binding, Empty),
        Rule::Column,
    ),
    (
        Row::new("WarningOnEditRepresentation", Usage, Enum, Auto),
        Rule::Always,
    ),
    (
        Row::new("WarningOnEdit", Usage, Localized, Empty),
        Rule::Always,
    ),
    (
        Row::new("EditMode", Usage, Enum, Choice("enter")),
        Rule::Column,
    ),
    (
        Row::new("AutoCellHeight", Usage, Boolean, UNCHECKED),
        Rule::Column,
    ),
    (
        Row::new("CellHyperlink", Usage, Boolean, UNCHECKED),
        Rule::Column,
    ),
    (
        Row::new("FixingInTable", Usage, Enum, Choice("no")),
        Rule::Column,
    ),
    (Row::new("Shortcut", Usage, Shortcut, Empty), Rule::Always),
    (Row::new("CommandSet", Usage, Dialog, OPEN), Rule::Always),
    (Row::new("MarkNegatives", Usage, Enum, Auto), Rule::Numeric),
    (
        Row::new("Zoomable", Usage, Boolean, UNCHECKED),
        Rule::Picture,
    ),
    (
        Row::new("NonselectedPictureText", Usage, Localized, Empty),
        Rule::Picture,
    ),
    (
        Row::new("EnableStartDrag", Usage, Boolean, UNCHECKED),
        Rule::Picture,
    ),
    (
        Row::new("EnableDrag", Usage, Boolean, UNCHECKED),
        Rule::Picture,
    ),
    (
        Row::new("FileDragMode", Usage, Enum, Choice("asFile")),
        Rule::Picture,
    ),
    (
        Row::new("ShowInHeader", Usage, Boolean, CHECKED),
        Rule::Column,
    ),
    (
        Row::new("ShowInFooter", Usage, Boolean, CHECKED),
        Rule::Column,
    ),
    (TOOLTIP, Rule::Always),
    (TOOLTIP_REPRESENTATION, Rule::Always),
    (
        Row::new("HeaderPicture", Appearance, Picture, Empty),
        Rule::Column,
    ),
    (
        Row::new("FooterPicture", Appearance, Picture, Empty),
        Rule::Column,
    ),
    (
        Row::new("TitleBackColor", Appearance, Color, Auto),
        Rule::Column,
    ),
    (TITLE_TEXT_COLOR, Rule::Always),
    (TITLE_FONT, Rule::Always),
    (
        Row::new("FooterBackColor", Appearance, Color, Auto),
        Rule::Column,
    ),
    (
        Row::new("FooterTextColor", Appearance, Color, Auto),
        Rule::Column,
    ),
    (Row::new("FooterFont", Appearance, Font, Auto), Rule::Column),
    (
        Row::new("PictureSize", Appearance, Enum, Choice("realSize")),
        Rule::Picture,
    ),
    (Row::new("BackColor", Appearance, Color, Auto), Rule::Label),
    (Row::new("TextColor", Appearance, Color, Auto), Rule::Always),
    (Row::new("Font", Appearance, Font, Auto), Rule::Picture),
    (
        Row::new("BorderColor", Appearance, Color, Auto),
        Rule::Always,
    ),
    (
        Row::new("Border", Appearance, Dialog, Choice("noBorder")),
        Rule::Always,
    ),
    (Row::new("Font", Appearance, Font, Auto), Rule::Label),
    (
        Row::new("HorizontalAlignInGroup", Layout, Enum, Auto)
            .with_source(SettingSource::Property("GroupHorizontalAlign")),
        Rule::Outside,
    ),
    (
        Row::new("VerticalAlignInGroup", Layout, Enum, Auto)
            .with_source(SettingSource::Property("GroupVerticalAlign")),
        Rule::Outside,
    ),
    (Row::new("HorizontalAlign", Layout, Enum, Auto), Rule::Label),
    (
        Row::new("HeaderHorizontalAlign", Layout, Enum, Choice("left")),
        Rule::Column,
    ),
    (
        Row::new("FooterHorizontalAlign", Layout, Enum, Auto),
        Rule::Column,
    ),
    (Row::new("VerticalAlign", Layout, Enum, Auto), Rule::Label),
    (WIDTH, Rule::Always),
    (Row::new("Height", Layout, Number, ZERO), Rule::Always),
    (
        Row::new("AutoMaxWidth", Layout, Boolean, CHECKED),
        Rule::Always,
    ),
    (Row::new("MaxWidth", Layout, Number, ZERO), Rule::MaxWidth),
    (
        Row::new("AutoMaxHeight", Layout, Boolean, CHECKED),
        Rule::Always,
    ),
    (Row::new("MaxHeight", Layout, Number, ZERO), Rule::MaxHeight),
    (
        Row::new("HorizontalStretch", Layout, Enum, AutoStretch),
        Rule::Label,
    ),
    (
        Row::new("VerticalStretch", Layout, Enum, AutoStretch),
        Rule::Label,
    ),
    (
        Row::new("HorizontalStretch", Layout, Boolean, UNCHECKED),
        Rule::Picture,
    ),
    (
        Row::new("VerticalStretch", Layout, Boolean, UNCHECKED),
        Rule::Picture,
    ),
    (Row::new("TitleHeight", Layout, Number, ZERO), Rule::Always),
    (event("OnChange"), Rule::Always),
    (event("Click"), Rule::Always),
    (event("URLProcessing"), Rule::Label),
    (event("DragStart"), Rule::Picture),
    (event("DragCheck"), Rule::Picture),
    (event("DragEnd"), Rule::Picture),
    (event("Drag"), Rule::Picture),
];
