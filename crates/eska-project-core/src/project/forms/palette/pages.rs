//! Properties of page groups and individual pages in Designer 8.3.27.

use super::{
    PaletteCategory::{Appearance, Layout, Main, Usage},
    PaletteDefinition as Row,
    PaletteEditor::{Binding, Boolean, Color, Enum, Number, Picture, Reference, Shortcut},
    PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty},
    SettingSource::{Kind, Property},
    common::{
        CHECKED, ENABLE_CONTENT_CHANGE, ENABLED, IMPORTANCE, NAME, READ_ONLY, TITLE, TITLE_FONT,
        TITLE_TEXT_COLOR, TOOLTIP, TOOLTIP_REPRESENTATION, TRISTATE, UNCHECKED, USER_VISIBILITY,
        VISIBLE, WIDTH, ZERO, event,
    },
};

pub(super) fn definitions(is_group: bool) -> impl Iterator<Item = (&'static Row, bool)> {
    let properties = if is_group { PAGES } else { PAGE };
    GENERAL.iter().chain(properties).map(|row| (row, true))
}

const GENERAL: &[Row] = &[
    NAME,
    TITLE,
    Row::new("Type", Main, Enum, Empty).with_source(Kind),
    VISIBLE,
    USER_VISIBILITY,
    ENABLED,
    READ_ONLY,
    ENABLE_CONTENT_CHANGE,
    IMPORTANCE,
];

const SHORTCUT: Row = Row::new("Shortcut", Usage, Shortcut, Empty);
const HEIGHT: Row = Row::new("Height", Layout, Number, ZERO);
const H_STRETCH: Row =
    Row::new("HorizontalStretch", Layout, Enum, AutoStretch).with_enum_captions(TRISTATE);
const V_STRETCH: Row =
    Row::new("VerticalStretch", Layout, Enum, AutoStretch).with_enum_captions(TRISTATE);
const H_ALIGN: &[(&str, &str)] = &[
    ("Auto", "auto"),
    ("Left", "left"),
    ("Center", "center"),
    ("Right", "right"),
];
const V_ALIGN: &[(&str, &str)] = &[
    ("Auto", "auto"),
    ("Top", "top"),
    ("Center", "center"),
    ("Bottom", "bottom"),
];
const TOOLTIP_MODE: Row = TOOLTIP_REPRESENTATION.with_enum_captions(&[
    ("Auto", "auto"),
    ("None", "no"),
    ("Balloon", "tooltipPopUp"),
    ("Button", "tooltipButton"),
    ("ShowAuto", "tooltipDisplayAutomatically"),
    ("ShowLeft", "tooltipDisplayLeft"),
    ("ShowTop", "tooltipDisplayAbove"),
    ("ShowRight", "tooltipDisplayRight"),
    ("ShowBottom", "tooltipDisplayBelow"),
]);
const SPACING: &[(&str, &str)] = &[
    ("Auto", "auto"),
    ("None", "none"),
    ("Half", "halfSpacing"),
    ("Single", "singleSpacing"),
    ("OneAndHalf", "oneAndHalfSpacing"),
    ("Double", "doubleSpacing"),
];

const PAGES: &[Row] = &[
    Row::new("CurrentRowUse", Main, Enum, Auto)
        .with_enum_captions(&[("Use", "usesCurrentRow"), ("DontUse", "ignoresCurrentRow")]),
    Row::new("AssociatedTable", Main, Reference, Empty)
        .with_source(Property("AssociatedTableElementId")),
    SHORTCUT,
    TOOLTIP,
    TOOLTIP_MODE,
    Row::new("PagesRepresentation", Usage, Enum, Auto).with_enum_captions(&[
        ("Auto", "auto"),
        ("None", "none"),
        ("TabsOnTop", "tabsAbove"),
        ("TabsOnBottom", "tabsBelow"),
        ("TabsOnLeftHorizontal", "tabsOnLeftHorizontally"),
        ("TabsOnRightHorizontal", "tabsOnRightHorizontally"),
        ("Swipe", "swipeTouchScreen"),
    ]),
    TITLE_TEXT_COLOR,
    TITLE_FONT,
    WIDTH,
    HEIGHT,
    Row::new("HorizontalAlignInGroup", Layout, Enum, Auto)
        .with_source(Property("GroupHorizontalAlign"))
        .with_enum_captions(H_ALIGN),
    Row::new("VerticalAlignInGroup", Layout, Enum, Auto)
        .with_source(Property("GroupVerticalAlign"))
        .with_enum_captions(V_ALIGN),
    H_STRETCH,
    V_STRETCH,
    event("OnCurrentPageChange"),
];

const PAGE: &[Row] = &[
    Row::new("Group", Main, Enum, Choice("vertical")).with_enum_captions(&[
        ("Vertical", "vertical"),
        ("HorizontalIfPossible", "horizontalIfPossible"),
        ("AlwaysHorizontal", "alwaysHorizontal"),
    ]),
    Row::new("TitleDataPath", Main, Binding, Empty),
    SHORTCUT,
    TOOLTIP,
    TOOLTIP_MODE,
    Row::new("ShowTitle", Usage, Boolean, CHECKED),
    Row::new("Picture", Usage, Picture, Empty),
    Row::new("ItemsAndTitlesAlign", Usage, Enum, Auto)
        .with_source(Property("ChildrenAlign"))
        .with_enum_captions(&[
            ("Auto", "auto"),
            ("None", "none"),
            ("ItemsLeftTitlesLeft", "itemsLeftTitlesLeft"),
            ("ItemsRightTitlesLeft", "itemsRightTitlesLeft"),
            ("ItemsLeftTitlesRight", "itemsLeftTitlesRight"),
            ("ItemsRightTitlesRight", "itemsRightTitlesRight"),
            ("TitlesLeftDataAuto", "itemsAutoTitlesLeft"),
        ]),
    TITLE_TEXT_COLOR,
    TITLE_FONT,
    Row::new("HorizontalSpacing", Appearance, Enum, Auto).with_enum_captions(SPACING),
    Row::new("VerticalSpacing", Appearance, Enum, Auto).with_enum_captions(SPACING),
    Row::new("BackColor", Appearance, Color, Auto),
    WIDTH,
    HEIGHT,
    H_STRETCH,
    V_STRETCH,
    Row::new("ChildItemsHorizontalAlign", Layout, Enum, Auto)
        .with_source(Property("HorizontalAlign"))
        .with_enum_captions(H_ALIGN),
    Row::new("ChildItemsVerticalAlign", Layout, Enum, Auto)
        .with_source(Property("VerticalAlign"))
        .with_enum_captions(V_ALIGN),
    Row::new("VerticalScrollOnReduceSize", Layout, Boolean, UNCHECKED)
        .with_source(Property("ScrollOnCompress")),
];
