use super::{
    PaletteCategory, PaletteDefinition, PaletteEditor, PlatformPropertyValue, SettingSource,
};
use crate::project::forms::{
    EffectiveElement, ElementPresentation, FieldValueKind, appearance, presentation,
};

#[derive(Clone, Copy)]
pub(super) struct Context {
    kind: FieldValueKind,
    in_table: bool,
    manual_max_width: bool,
    manual_max_height: bool,
}

impl Context {
    pub(super) fn new(view: &ElementPresentation, element: &EffectiveElement) -> Option<Self> {
        let kind = view.value_type.kind;
        let in_table = view.in_table;
        let manual_max = |name| {
            presentation::property(element, name)
                .and_then(|property| appearance::text(&property.property.value))
                .is_some_and(|value| matches!(value, "false" | "No" | "0"))
        };
        match (kind, in_table) {
            (FieldValueKind::String | FieldValueKind::Number, _)
            | (FieldValueKind::Date | FieldValueKind::Boolean, false) => Some(Self {
                kind,
                in_table,
                manual_max_width: manual_max("AutoMaxWidth"),
                manual_max_height: manual_max("AutoMaxHeight"),
            }),
            _ => None,
        }
    }
}

pub(super) fn definitions(context: Context) -> impl Iterator<Item = &'static PaletteDefinition> {
    INPUT_FIELD
        .iter()
        .filter(move |setting| setting.rule.applies(context))
        .map(|setting| &setting.definition)
}

#[derive(Clone, Copy)]
enum Rule {
    Always,
    Text,
    NonText,
    Numeric,
    TextOrBoolean,
    Column,
    OutsideTable,
    ManualMaxWidth,
    ManualMaxHeight,
}

impl Rule {
    fn applies(self, context: Context) -> bool {
        match self {
            Self::Always => true,
            Self::Text => context.kind == FieldValueKind::String,
            Self::NonText => context.kind != FieldValueKind::String,
            Self::Numeric => context.kind == FieldValueKind::Number,
            Self::TextOrBoolean => matches!(
                context.kind,
                FieldValueKind::String | FieldValueKind::Boolean
            ),
            Self::Column => context.in_table,
            Self::OutsideTable => !context.in_table,
            Self::ManualMaxWidth => context.manual_max_width,
            Self::ManualMaxHeight => context.manual_max_height,
        }
    }
}

struct Setting {
    definition: PaletteDefinition,
    rule: Rule,
}

const fn setting(
    id: &'static str,
    category: PaletteCategory,
    editor: PaletteEditor,
    default: PlatformPropertyValue,
) -> Setting {
    Setting {
        definition: PaletteDefinition::new(id, category, editor, default),
        rule: Rule::Always,
    }
}

impl Setting {
    const fn on(mut self, rule: Rule) -> Self {
        self.rule = rule;
        self
    }

    const fn from(mut self, source: SettingSource) -> Self {
        self.definition = self.definition.with_source(source);
        self
    }

    const fn captions(mut self, captions: &'static [(&'static str, &'static str)]) -> Self {
        self.definition = self.definition.with_enum_captions(captions);
        self
    }
}

const fn event(id: &'static str) -> Setting {
    setting(
        id,
        PaletteCategory::Events,
        PaletteEditor::Event,
        PlatformPropertyValue::Empty,
    )
    .from(SettingSource::Event)
}

// Native Configurator 8.3.27, Version8_3_27, in ordinary forms and value table columns.
// Keep the rows in native order. A rule omits settings absent in the selected context.
const INPUT_FIELD: &[Setting] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteEditor::{
        Binding, Boolean, Color, Dialog, Enum, Font, Localized, Number, Picture, Shortcut, Text,
    };
    use PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty, Unknown};
    use Rule::{Column, NonText, Numeric, OutsideTable, TextOrBoolean};
    use SettingSource::{Attribute, Property};

    const CHECKED: PlatformPropertyValue = PlatformPropertyValue::Boolean(true);
    const UNCHECKED: PlatformPropertyValue = PlatformPropertyValue::Boolean(false);
    const ZERO: PlatformPropertyValue = PlatformPropertyValue::Number(0);
    const DEFAULT: PlatformPropertyValue = Choice("default");
    const OPEN: PlatformPropertyValue = PlatformPropertyValue::Dialog;
    const TRISTATE: &[(&str, &str)] = &[("true", "yes"), ("false", "no")];
    &[
        setting("Name", Main, Text, Unknown).from(SettingSource::Name),
        setting("Title", Main, Localized, Empty),
        setting("Type", Main, Enum, Unknown).from(SettingSource::Kind),
        setting("DataPath", Main, Binding, Empty),
        setting("TitleLocation", Main, Enum, Auto).captions(&[("Top", "top")]),
        setting("Visible", Main, Boolean, CHECKED),
        setting("UserVisibility", Main, Dialog, OPEN).from(Property("UserVisible")),
        setting("Enabled", Main, Boolean, CHECKED),
        setting("ReadOnly", Main, Boolean, UNCHECKED),
        setting("SkipOnInput", Main, Enum, DEFAULT).captions(TRISTATE),
        setting("DefaultItem", Main, Boolean, UNCHECKED),
        setting("DisplayImportance", Main, Enum, Auto).from(Attribute("DisplayImportance")),
        setting("OnMainServerUnavailableBehavior", Main, Enum, Auto)
            .from(Property("OnMainServerUnavalableBehavior"))
            .captions(&[("MakeDisable", "disableAvailability")]),
        setting("Mask", Main, Text, Empty).on(Rule::Text),
        setting("PasswordMode", Main, Enum, DEFAULT)
            .on(Rule::Text)
            .captions(TRISTATE),
        setting("MultiLine", Main, Enum, DEFAULT).captions(TRISTATE),
        setting("ExtendedEdit", Main, Enum, DEFAULT)
            .on(Rule::Text)
            .captions(TRISTATE),
        setting("EditFormat", Main, Dialog, Empty).on(NonText),
        setting("MinValue", Main, Number, Empty).on(Numeric),
        setting("MaxValue", Main, Number, Empty).on(Numeric),
        setting("FooterText", Main, Localized, Empty).on(Column),
        setting("FooterDataPath", Main, Binding, Empty).on(Column),
        setting("WarningOnEditRepresentation", Usage, Enum, Auto),
        setting("WarningOnEdit", Usage, Localized, Empty),
        setting("EditMode", Usage, Enum, Choice("enter")).on(Column),
        setting("AutoCellHeight", Usage, Boolean, UNCHECKED).on(Column),
        setting("CellHyperlink", Usage, Boolean, UNCHECKED).on(Column),
        setting("FixingInTable", Usage, Enum, Choice("no")).on(Column),
        setting("Shortcut", Usage, Shortcut, Empty),
        setting("CommandSet", Usage, Dialog, OPEN),
        setting("DropListButton", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ChoiceButton", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ClearButton", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("SpinButton", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("OpenButton", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("CreateButton", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ChoiceButtonRepresentation", Usage, Enum, Auto),
        setting("ListChoiceMode", Usage, Boolean, UNCHECKED),
        setting("ChooseType", Usage, Boolean, CHECKED),
        setting("ChoiceList", Usage, Dialog, Empty),
        setting(
            "IncompleteChoiceMode",
            Usage,
            Enum,
            Choice("onPressingEnter"),
        ),
        setting("TextEdit", Usage, Boolean, CHECKED),
        setting("EditTextUpdate", Usage, Enum, Auto),
        setting("Wrap", Usage, Boolean, CHECKED).on(TextOrBoolean),
        setting("MarkNegatives", Usage, Enum, DEFAULT)
            .on(Numeric)
            .captions(TRISTATE),
        setting("AutoMarkIncomplete", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("AutoChoiceIncomplete", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ChoiceFoldersAndItems", Usage, Enum, Auto),
        setting("QuickChoice", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ChoiceHistoryOnInput", Usage, Enum, Auto),
        setting("TypeLink", Usage, Dialog, Empty),
        setting("AutoShowClearButton", Usage, Enum, Auto)
            .from(Property("AutoShowClearButtonMode"))
            .captions(&[("Always", "always")]),
        setting("AutoShowOpenButton", Usage, Enum, Auto)
            .from(Property("AutoShowOpenButtonMode"))
            .captions(&[("Always", "always")]),
        setting("AutoCorrectionOnTextInput", Usage, Enum, Auto),
        setting("SpellCheckingOnTextInput", Usage, Enum, Auto),
        setting("AutoCapitalizationOnTextInput", Usage, Enum, Auto),
        setting("SpecialTextInputMode", Usage, Enum, Auto),
        setting("AutoFillHint", Usage, Enum, Choice("doNotUse")),
        setting("OnScreenKeyboardReturnKeyText", Usage, Enum, Auto),
        setting("ShowInHeader", Usage, Boolean, CHECKED).on(Column),
        setting("ShowInFooter", Usage, Boolean, CHECKED).on(Column),
        setting("ToolTip", Usage, Localized, Empty),
        setting("ToolTipRepresentation", Usage, Enum, Auto),
        setting("InputHint", Usage, Localized, Empty).on(TextOrBoolean),
        setting("HeaderPicture", Appearance, Picture, Empty).on(Column),
        setting("FooterPicture", Appearance, Picture, Empty).on(Column),
        setting("TitleBackColor", Appearance, Color, Auto).on(Column),
        setting("TitleTextColor", Appearance, Color, Auto),
        setting("TitleFont", Appearance, Font, Auto),
        setting("FooterBackColor", Appearance, Color, Auto).on(Column),
        setting("FooterTextColor", Appearance, Color, Auto).on(Column),
        setting("FooterFont", Appearance, Font, Auto).on(Column),
        setting("ChoiceButtonPicture", Appearance, Picture, Empty),
        setting("BackColor", Appearance, Color, Auto),
        setting("TextColor", Appearance, Color, Auto),
        setting("BorderColor", Appearance, Color, Auto),
        setting("Font", Appearance, Font, Auto),
        setting("HorizontalAlignInGroup", Layout, Enum, Auto)
            .from(Property("GroupHorizontalAlign"))
            .on(OutsideTable),
        setting("VerticalAlignInGroup", Layout, Enum, Auto)
            .from(Property("GroupVerticalAlign"))
            .on(OutsideTable),
        setting("HorizontalAlign", Layout, Enum, Auto),
        setting("HeaderHorizontalAlign", Layout, Enum, Choice("left")).on(Column),
        setting("FooterHorizontalAlign", Layout, Enum, Auto).on(Column),
        setting("Width", Layout, Number, ZERO),
        setting("Height", Layout, Number, ZERO),
        setting("AutoMaxWidth", Layout, Boolean, CHECKED),
        setting("MaxWidth", Layout, Number, ZERO).on(Rule::ManualMaxWidth),
        setting("AutoMaxHeight", Layout, Boolean, CHECKED),
        setting("MaxHeight", Layout, Number, ZERO).on(Rule::ManualMaxHeight),
        setting("HeightControlVariant", Layout, Enum, Auto),
        setting("ChoiceListHeight", Layout, Number, ZERO),
        setting("DropListWidth", Layout, Number, ZERO),
        setting("HorizontalStretch", Layout, Enum, AutoStretch).captions(TRISTATE),
        setting("VerticalStretch", Layout, Enum, AutoStretch).captions(TRISTATE),
        setting("TitleHeight", Layout, Number, ZERO),
        event("OnChange"),
        event("StartChoice"),
        event("StartListChoice"),
        event("Clearing"),
        event("Tuning"),
        event("Opening"),
        event("Creating"),
        event("ChoiceProcessing"),
        event("EditTextChange"),
        event("AutoComplete"),
        event("TextEditEnd"),
        event("CommandGenerateProcessing"),
        event("MultipleValuesAdd"),
        event("MultipleValuesDelete"),
        event("MultipleValueOpening"),
        event("MultipleValueURLProcessing"),
    ]
};
