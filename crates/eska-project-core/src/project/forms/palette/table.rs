//! Designer 8.3.27 palettes for tables and column groups.

use super::common::event;
use super::{
    PaletteCategory, PaletteDefinition as Row, PaletteEditor, PlatformPropertyValue, SettingSource,
    common::{
        CHECKED, ENABLED, IMPORTANCE, NAME, OPEN, READ_ONLY, TITLE, TITLE_FONT, TITLE_TEXT_COLOR,
        TOOLTIP, TOOLTIP_REPRESENTATION, TRISTATE, UNCHECKED, USER_VISIBILITY, VISIBLE, WIDTH,
        ZERO,
    },
    unbound_data_path,
};
use crate::project::{
    forms::{EffectiveElement, ElementPresentation, FieldValueKind, commands},
    metadata_model::PropertyKey,
};

pub(super) fn table_definitions(
    view: &ElementPresentation,
    element: &EffectiveElement,
) -> Option<impl Iterator<Item = (&'static Row, bool)>> {
    let binding = &view.value_type;
    let dynamic = commands::is_dynamic_list(binding);
    let core_type =
        |kind: &PropertyKey| kind.namespace.as_deref() == Some("http://v8.1c.ru/8.1/data/core");
    let supported = match (binding.kind, binding.types.as_slice()) {
        (FieldValueKind::Unknown, []) => unbound_data_path(element),
        // A metadata tabular section has a table kind and no scalar type.
        (FieldValueKind::Table, []) => true,
        (FieldValueKind::Table, [kind]) => {
            dynamic
                || core_type(kind)
                    && matches!(kind.name.as_str(), "ValueTable" | "ValueTree" | "ValueList")
        }
        (FieldValueKind::Document, [kind]) => core_type(kind) && kind.name == "ValueListType",
        _ => false,
    };
    if !supported {
        return None;
    }
    let manual_max = move |name| matches!(element.scalar(name), Some("false" | "0"));
    Some(TABLE.iter().filter_map(move |definition| {
        let available = match definition.id {
            "MaxWidth" if !manual_max("AutoMaxWidth") => return None,
            "MaxHeight" if !manual_max("AutoMaxHeight") => return None,
            "MaxHeightInTableRows" if !manual_max("AutoMaxRowsCount") => return None,
            "AutoRefresh"
            | "AutoRefreshPeriod"
            | "ChoiceFoldersAndItems"
            | "RestoreCurrentRow"
            | "ShowRoot"
            | "AllowRootChoice"
            | "AllowGettingCurrentRowURL"
            | "UpdateOnDataChange"
            | "UserSettingsGroup"
            | "OnSaveUserSettingsAtServer"
            | "BeforeLoadUserSettingsAtServer"
            | "OnLoadUserSettingsAtServer"
            | "OnUpdateUserSettingSetAtServer"
            | "URLGetProcessing"
            | "URLListGetProcessing"
            | "OnGetDataAtServer"
                if !dynamic =>
            {
                return None;
            }
            "AutofillColumns" => false,
            "FileDragMode" => matches!(element.scalar("EnableDrag"), Some("true" | "1")),
            _ => true,
        };
        Some((definition, available))
    }))
}

pub(super) fn column_group_definitions(
    view: &ElementPresentation,
    element: &EffectiveElement,
) -> Option<impl Iterator<Item = (&'static Row, bool)>> {
    if !view.in_table
        || !matches!(
            element.scalar("Group"),
            None | Some("Vertical" | "Horizontal" | "InCell")
        )
    {
        return None;
    }
    let header = matches!(element.scalar("ShowInHeader"), Some("true" | "1"));
    Some(COLUMN_GROUP.iter().filter_map(move |definition| {
        if !header
            && matches!(
                definition.id,
                "HeaderDataPath"
                    | "ShowTitle"
                    | "HeaderPicture"
                    | "TitleBackColor"
                    | "HeaderHorizontalAlign"
            )
        {
            return None;
        }
        Some((definition, true))
    }))
}

use PaletteCategory::{Appearance, Layout, Main, Usage};
use PaletteEditor::{
    Binding, Boolean, Color, Dialog, Enum, Font, Number, Picture, Reference, Shortcut,
};
use PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty, Unknown};
use SettingSource::Property;

const ONE: PlatformPropertyValue = PlatformPropertyValue::Number(1);
const DEFAULT: PlatformPropertyValue = Choice("default");
const HEIGHT: Row = Row::new("Height", Layout, Number, ZERO);

const TABLE: &[Row] = &[
    NAME,
    TITLE,
    Row::new("DataPath", Main, Binding, Empty),
    Row::new("TitleLocation", Main, Enum, Choice("none")).with_enum_captions(&[
        ("None", "none"),
        ("Top", "top"),
        ("Left", "left"),
    ]),
    VISIBLE,
    USER_VISIBILITY,
    ENABLED,
    READ_ONLY,
    // Designer displays this design-time command as a disabled checkbox for these bindings.
    Row::new("AutofillColumns", Main, Boolean, UNCHECKED).with_source(SettingSource::Unmapped),
    Row::new("SkipOnInput", Main, Enum, DEFAULT).with_enum_captions(TRISTATE),
    Row::new("DefaultItem", Main, Boolean, UNCHECKED),
    Row::new("ChoiceMode", Main, Boolean, UNCHECKED),
    Row::new("MultipleChoice", Main, Boolean, UNCHECKED),
    IMPORTANCE,
    Row::new("OnMainServerUnavailableBehavior", Main, Enum, Auto)
        .with_source(Property("OnMainServerUnavalableBehavior"))
        .with_enum_captions(&[("MakeDisable", "disableAvailability")]),
    Row::new("CommandBarLocation", Usage, Enum, Auto).with_enum_captions(&[
        ("None", "none"),
        ("Top", "top"),
        ("Bottom", "bottom"),
    ]),
    Row::new("ChangeRowSet", Usage, Boolean, CHECKED),
    Row::new("ChangeRowOrder", Usage, Boolean, CHECKED),
    Row::new("CommandSet", Usage, Dialog, OPEN),
    Row::new("Representation", Usage, Enum, Choice("hierarchicalList")).with_enum_captions(&[
        ("HierarchicalList", "hierarchicalList"),
        ("List", "list"),
        ("Tree", "tree"),
    ]),
    Row::new("RowInputMode", Usage, Enum, Choice("endOfList")),
    Row::new("SelectionMode", Usage, Enum, Choice("multipleSelection")),
    Row::new("RowSelectionMode", Usage, Enum, Choice("cellSelection")),
    Row::new("Header", Usage, Boolean, CHECKED),
    Row::new("Footer", Usage, Boolean, UNCHECKED),
    Row::new("Output", Usage, Enum, Auto),
    Row::new("AutoInsertNewRow", Usage, Boolean, UNCHECKED),
    Row::new("AutoAddIncomplete", Usage, Enum, DEFAULT).with_enum_captions(TRISTATE),
    Row::new("AutoMarkIncomplete", Usage, Enum, DEFAULT).with_enum_captions(TRISTATE),
    Row::new("SearchOnInput", Usage, Enum, Auto),
    Row::new("Shortcut", Usage, Shortcut, Empty),
    Row::new("EnableStartDrag", Usage, Boolean, UNCHECKED),
    Row::new("EnableDrag", Usage, Boolean, UNCHECKED),
    Row::new("FileDragMode", Usage, Enum, Choice("asFileReference")),
    TOOLTIP,
    TOOLTIP_REPRESENTATION,
    Row::new("AutoRefresh", Usage, Boolean, UNCHECKED),
    Row::new(
        "AutoRefreshPeriod",
        Usage,
        Number,
        PlatformPropertyValue::Number(60),
    ),
    Row::new("ChoiceFoldersAndItems", Usage, Enum, Choice("items")).with_enum_captions(&[
        ("Items", "items"),
        ("Folders", "folders"),
        ("FoldersAndItems", "foldersAndItems"),
    ]),
    Row::new("RestoreCurrentRow", Usage, Boolean, UNCHECKED),
    Row::new("ShowRoot", Usage, Boolean, CHECKED),
    Row::new("AllowRootChoice", Usage, Boolean, UNCHECKED),
    Row::new("AllowGettingCurrentRowURL", Usage, Boolean, CHECKED),
    Row::new("UpdateOnDataChange", Usage, Enum, Auto),
    Row::new("UserSettingsGroup", Usage, Reference, Empty),
    Row::new("RowPictureDataPath", Usage, Binding, Empty),
    Row::new("RowsPicture", Usage, Picture, Empty),
    Row::new(
        "HorizontalScrollBar",
        Usage,
        Enum,
        Choice("useAutomatically"),
    ),
    Row::new("VerticalScrollBar", Usage, Enum, Choice("useAutomatically")),
    Row::new("InitialListView", Usage, Enum, Auto),
    Row::new("InitialTreeView", Usage, Enum, Choice("doNotExpand")),
    Row::new("SearchStringLocation", Usage, Enum, Auto),
    Row::new("ViewStatusLocation", Usage, Enum, Auto),
    Row::new("SearchControlLocation", Usage, Enum, Auto),
    Row::new("RefreshRequest", Usage, Enum, Choice("none")),
    Row::new("CurrentRowUse", Usage, Enum, Auto),
    Row::new("BehaviorOnHorizontalCompression", Usage, Enum, Auto),
    Row::new("HorizontalLines", Appearance, Boolean, CHECKED),
    Row::new("VerticalLines", Appearance, Boolean, CHECKED),
    Row::new("UseAlternationRowColor", Appearance, Boolean, UNCHECKED),
    Row::new("BackColor", Appearance, Color, Auto),
    Row::new("TextColor", Appearance, Color, Auto),
    Row::new("BorderColor", Appearance, Color, Auto),
    Row::new("Font", Appearance, Font, Auto),
    TITLE_TEXT_COLOR,
    TITLE_FONT,
    WIDTH,
    Row::new("AutoMaxWidth", Layout, Boolean, CHECKED),
    Row::new("MaxWidth", Layout, Number, ZERO),
    Row::new("HeightControlVariant", Layout, Enum, Auto).with_enum_captions(&[
        ("UseHeightInTableRows", "useHeightInTableRows"),
        ("UseHeightInFormRows", "useHeightInFormRows"),
        ("UseContentHeight", "useContentHeight"),
    ]),
    HEIGHT,
    Row::new("AutoMaxHeight", Layout, Boolean, CHECKED),
    Row::new("MaxHeight", Layout, Number, ZERO),
    Row::new("HeightInTableRows", Layout, Number, ZERO),
    Row::new("AutoMaxHeightInTableRows", Layout, Boolean, CHECKED)
        .with_source(Property("AutoMaxRowsCount")),
    Row::new("MaxHeightInTableRows", Layout, Number, ZERO).with_source(Property("MaxRowsCount")),
    Row::new("HorizontalAlignInGroup", Layout, Enum, Auto)
        .with_source(Property("GroupHorizontalAlign")),
    Row::new("VerticalAlignInGroup", Layout, Enum, Auto)
        .with_source(Property("GroupVerticalAlign")),
    Row::new("HorizontalStretch", Layout, Boolean, CHECKED),
    Row::new("VerticalStretch", Layout, Boolean, CHECKED),
    Row::new("HeaderHeight", Layout, Number, ONE),
    Row::new("FooterHeight", Layout, Number, ONE),
    Row::new("TitleHeight", Layout, Number, ZERO),
    event("OnChange"),
    event("Selection"),
    event("OnActivateRow"),
    event("ValueChoice"),
    event("OnActivateField"),
    event("OnActivateCell"),
    event("BeforeAddRow"),
    event("BeforeRowChange"),
    event("BeforeDeleteRow"),
    event("OnStartEdit"),
    event("BeforeEditEnd"),
    event("OnEditEnd"),
    event("ChoiceProcessing"),
    event("BeforeExpand"),
    event("BeforeCollapse"),
    event("AfterDeleteRow"),
    event("OnCurrentParentChange"),
    event("NewWriteProcessing"),
    event("OnSaveUserSettingsAtServer"),
    event("BeforeLoadUserSettingsAtServer"),
    event("OnLoadUserSettingsAtServer"),
    event("OnUpdateUserSettingSetAtServer"),
    event("URLGetProcessing"),
    event("URLListGetProcessing"),
    event("RefreshRequestProcessing"),
    event("OnGetDataAtServer"),
    event("DragStart"),
    event("DragCheck"),
    event("DragEnd"),
    event("Drag"),
];

const COLUMN_GROUP: &[Row] = &[
    NAME,
    TITLE,
    Row::new("Type", Main, Enum, Unknown).with_source(SettingSource::Kind),
    VISIBLE,
    USER_VISIBILITY,
    ENABLED,
    READ_ONLY,
    super::common::ENABLE_CONTENT_CHANGE,
    IMPORTANCE,
    Row::new("Group", Main, Enum, Choice("vertical")).with_enum_captions(&[
        ("Vertical", "vertical"),
        ("Horizontal", "horizontal"),
        ("InCell", "inCell"),
    ]),
    Row::new("HeaderDataPath", Main, Binding, Empty),
    TOOLTIP,
    TOOLTIP_REPRESENTATION,
    Row::new("ShowTitle", Usage, Boolean, CHECKED),
    Row::new("ShowInHeader", Usage, Boolean, UNCHECKED),
    Row::new("FixingInTable", Usage, Enum, Choice("no")),
    TITLE_TEXT_COLOR,
    TITLE_FONT,
    Row::new("HeaderPicture", Appearance, Picture, Empty),
    Row::new("TitleBackColor", Appearance, Color, Auto),
    WIDTH,
    HEIGHT,
    Row::new("HorizontalStretch", Layout, Enum, AutoStretch).with_enum_captions(TRISTATE),
    Row::new("VerticalStretch", Layout, Enum, AutoStretch).with_enum_captions(TRISTATE),
    Row::new("HeaderHorizontalAlign", Layout, Enum, Auto),
];
