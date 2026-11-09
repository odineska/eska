//! Command group and form item addition profiles observed in Designer 8.3.27.

use super::{
    PaletteCaption,
    PaletteCategory::{Appearance, Layout, Main, Usage},
    PaletteDefinition as Row,
    PaletteEditor::{Boolean, Color, Dialog, Enum, Font, Number, Picture, Reference},
    PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty},
    SettingSource::{self, Kind, Property},
    common::{
        CHECKED, ENABLED, IMPORTANCE, NAME, READ_ONLY, TITLE, TITLE_FONT, TITLE_TEXT_COLOR,
        TOOLTIP, TOOLTIP_REPRESENTATION, TRISTATE, USER_VISIBILITY, VISIBLE, WIDTH, ZERO,
    },
};
use crate::project::{
    forms::{EffectiveElement, ElementKind, ElementPresentation, appearance, is_form_property},
    metadata_model::MetadataValue,
};

pub(super) fn auto_bar_definitions(
    view: &ElementPresentation,
) -> Option<impl Iterator<Item = (&'static Row, bool)>> {
    matches!(
        view.parent_kind,
        Some(ElementKind::Form | ElementKind::Table)
    )
    .then(|| AUTO_COMMAND_BAR.iter().map(|row| (row, true)))
}

pub(super) fn group_definitions(
    view: &ElementPresentation,
    element: &EffectiveElement,
) -> Option<impl Iterator<Item = (&'static Row, bool)>> {
    use ElementKind::{AutoCommandBar, ButtonGroup, CommandBar, Form, Popup};
    let kind = element.kind?;
    let type_row = match (kind, view.parent_kind) {
        (CommandBar, _) | (ButtonGroup | Popup, Some(CommandBar | AutoCommandBar)) => &TYPE,
        (ButtonGroup | Popup, Some(Form)) => &EMPTY_TYPE,
        _ => return None,
    };
    Some(COMMAND_GROUP.iter().filter_map(move |row| {
        let row = match row.id {
            "Type" => type_row,
            "View" if kind != ButtonGroup => return None,
            "Representation"
            | "Picture"
            | "Shape"
            | "ShapeRepresentation"
            | "BackColor"
            | "BorderColor"
                if kind != Popup =>
            {
                return None;
            }
            "HorizontalAlign" if kind != CommandBar => return None,
            "HorizontalAlignInGroup" | "VerticalAlignInGroup" if kind == Popup => return None,
            _ => row,
        };
        Some((row, true))
    }))
}

pub(super) fn addition_definitions(
    view: &ElementPresentation,
    element: &EffectiveElement,
) -> Option<impl Iterator<Item = (&'static Row, bool)>> {
    let status = element.kind == Some(ElementKind::ViewStatusAddition);
    let standalone = element.in_child_items;
    if !(standalone
        || view.parent_kind == Some(ElementKind::Table)
        || status && view.parent_kind == Some(ElementKind::PdfDocumentField))
    {
        return None;
    }
    let general = if standalone { STANDALONE } else { OWNED };
    let appearance = if status {
        STATUS_APPEARANCE
    } else {
        SEARCH_APPEARANCE
    };
    let before_size: &[Row] = match (standalone, status) {
        (true, true) => &[GROUP_H_ALIGN, GROUP_V_ALIGN, STATUS_ALIGN],
        (true, false) => &[GROUP_H_ALIGN, GROUP_V_ALIGN],
        _ => &[],
    };
    let after_size: &[Row] = if status && !standalone {
        &[STATUS_ALIGN]
    } else {
        &[]
    };
    let manual = matches!(element.scalar("AutoMaxWidth"), Some("false" | "0"));
    Some(
        general
            .iter()
            .chain(ADDITION_USAGE)
            .chain(appearance)
            .chain(before_size)
            .chain(ADDITION_SIZE)
            .chain(after_size)
            .filter(move |row| row.id != "MaxWidth" || manual)
            .map(|row| (row, true)),
    )
}

pub(super) fn source_caption(value: &MetadataValue) -> Option<PaletteCaption<'_>> {
    let MetadataValue::Record(fields) = value else {
        return None;
    };
    if fields.len() != 2 {
        return None;
    }
    let get = |name| {
        fields
            .iter()
            .find(|field| is_form_property(&field.key, name))
            .and_then(|field| appearance::text(&field.value))
    };
    let item = get("Item").filter(|item| !item.is_empty())?;
    let member = match get("Type")? {
        "SearchStringRepresentation" => "searchStringMember",
        "SearchControl" => "searchControlMember",
        "ViewStatusRepresentation" => "viewStatusMember",
        _ => return None,
    };
    Some(PaletteCaption::ScriptMember { item, member })
}

const TYPE: Row = Row::new("Type", Main, Enum, Empty).with_source(Kind);
const EMPTY_TYPE: Row = Row::new("Type", Main, Enum, Empty).with_source(SettingSource::Unmapped);
const BACK_COLOR: Row = Row::new("BackColor", Appearance, Color, Auto);
const BORDER_COLOR: Row = Row::new("BorderColor", Appearance, Color, Auto);
const TEXT_COLOR: Row = Row::new("TextColor", Appearance, Color, Auto);
const FONT: Row = Row::new("Font", Appearance, Font, Auto);
const HORIZONTAL_ALIGN: &[(&str, &str)] = &[
    ("Auto", "auto"),
    ("Left", "left"),
    ("Center", "center"),
    ("Right", "right"),
];
const GROUP_H_ALIGN: Row = Row::new("HorizontalAlignInGroup", Layout, Enum, Auto)
    .with_source(Property("GroupHorizontalAlign"))
    .with_enum_captions(HORIZONTAL_ALIGN);
const GROUP_V_ALIGN: Row = Row::new("VerticalAlignInGroup", Layout, Enum, Auto)
    .with_source(Property("GroupVerticalAlign"))
    .with_enum_captions(&[
        ("Auto", "auto"),
        ("Top", "top"),
        ("Center", "center"),
        ("Bottom", "bottom"),
    ]);
const HORIZONTAL_STRETCH: Row =
    Row::new("HorizontalStretch", Layout, Enum, AutoStretch).with_enum_captions(TRISTATE);
const STATUS_ALIGN: Row = Row::new("HorizontalAlign", Layout, Enum, Auto)
    .with_source(Property("HorizontalLocation"))
    .with_enum_captions(HORIZONTAL_ALIGN);

const AUTO_COMMAND_BAR: &[Row] = &[
    IMPORTANCE,
    Row::new("AutoFill", Main, Boolean, CHECKED).with_source(Property("Autofill")),
    Row::new("HorizontalAlign", Main, Enum, Choice("left")).with_enum_captions(HORIZONTAL_ALIGN),
];

const COMMAND_GROUP: &[Row] = &[
    NAME,
    TITLE,
    TYPE,
    Row::new("CommandSource", Main, Reference, Empty),
    Row::new("View", Main, Enum, Auto)
        .with_source(Property("Representation"))
        .with_enum_captions(&[("Compact", "compactButtonGroup")]),
    VISIBLE,
    USER_VISIBILITY,
    ENABLED,
    READ_ONLY,
    Row::new("EnableContentChange", Main, Boolean, CHECKED),
    IMPORTANCE,
    Row::new("Representation", Main, Enum, Auto).with_enum_captions(&[
        ("Auto", "auto"),
        ("Text", "text"),
        ("Picture", "icon"),
    ]),
    TOOLTIP,
    TOOLTIP_REPRESENTATION,
    Row::new("HorizontalAlign", Usage, Enum, Choice("left"))
        .with_source(Property("HorizontalLocation"))
        .with_enum_captions(HORIZONTAL_ALIGN),
    Row::new("Picture", Usage, Picture, Empty),
    TITLE_TEXT_COLOR,
    TITLE_FONT,
    Row::new("Shape", Appearance, Enum, Auto)
        .with_enum_captions(&[("Usual", "standardShape"), ("Oval", "roundedShape")]),
    Row::new("ShapeRepresentation", Appearance, Enum, Auto),
    BACK_COLOR,
    BORDER_COLOR,
    WIDTH,
    GROUP_H_ALIGN,
    GROUP_V_ALIGN,
    HORIZONTAL_STRETCH,
    Row::new("VerticalStretch", Layout, Enum, AutoStretch).with_enum_captions(TRISTATE),
];

const OWNED: &[Row] = &[TITLE, ENABLED, IMPORTANCE];
const STANDALONE: &[Row] = &[
    NAME,
    TITLE,
    TYPE,
    Row::new("Source", Main, Reference, Empty).with_source(Property("AdditionSource")),
    VISIBLE,
    USER_VISIBILITY,
    ENABLED,
    IMPORTANCE,
];
const ADDITION_USAGE: &[Row] = &[TOOLTIP, TOOLTIP_REPRESENTATION];
const SEARCH_APPEARANCE: &[Row] = &[BACK_COLOR, TEXT_COLOR, BORDER_COLOR, FONT];
const STATUS_APPEARANCE: &[Row] = &[
    BACK_COLOR,
    Row::new("ButtonBackColor", Appearance, Color, Auto),
    TEXT_COLOR,
    FONT,
    TITLE_TEXT_COLOR,
    TITLE_FONT,
    BORDER_COLOR,
    Row::new("Border", Appearance, Dialog, Choice("noBorder")),
];
const ADDITION_SIZE: &[Row] = &[
    WIDTH,
    Row::new("AutoMaxWidth", Layout, Boolean, CHECKED),
    Row::new("MaxWidth", Layout, Number, ZERO),
    HORIZONTAL_STRETCH,
];
