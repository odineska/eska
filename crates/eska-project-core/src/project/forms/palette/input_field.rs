use super::common::{CHECKED, OPEN, TRISTATE, UNCHECKED, ZERO};
use super::{
    PaletteCategory, PaletteDefinition, PaletteEditor, PlatformPropertyValue, SettingSource,
    unbound_data_path,
};
use crate::project::forms::{
    EffectiveElement, ElementPresentation, FieldValueKind, presentation, references,
};
use crate::project::metadata_model::PropertyKey;

#[derive(Clone, Copy, Eq, PartialEq)]
enum InputBinding {
    Unbound { serialized_path: bool },
    String,
    Number,
    Date,
    Boolean,
    Reference,
    Composite { has_reference: bool },
    ValueList { extended: bool },
}

impl InputBinding {
    fn new(view: &ElementPresentation, element: &EffectiveElement) -> Option<Self> {
        let types = &view.value_type.types;
        Some(match view.value_type.kind {
            FieldValueKind::String => Self::String,
            FieldValueKind::Number => Self::Number,
            FieldValueKind::Date => Self::Date,
            FieldValueKind::Boolean => Self::Boolean,
            FieldValueKind::Reference if types.iter().any(metadata_reference) => Self::Reference,
            FieldValueKind::Composite => Self::composite(types)?,
            FieldValueKind::Document
                if types.iter().any(|key| {
                    key.namespace.as_deref() == Some("http://v8.1c.ru/8.1/data/core")
                        && key.name == "ValueListType"
                }) =>
            {
                let extended = match element.scalar("ExtendedEditMultipleValues") {
                    None | Some("false" | "0") => false,
                    Some("true" | "1") => true,
                    _ => return None,
                };
                Self::ValueList { extended }
            }
            FieldValueKind::Unknown => {
                let serialized_path = element
                    .scalar("DataPath")
                    .is_some_and(serialized_unresolved_data_path);
                if !serialized_path && !unbound_data_path(element) {
                    return None;
                }
                Self::Unbound { serialized_path }
            }
            _ => return None,
        })
    }

    fn composite(types: &[PropertyKey]) -> Option<Self> {
        let mut has_reference = false;
        for key in types {
            if metadata_reference(key) {
                has_reference = true;
            } else if !matches!(
                presentation::field_value_kind(std::slice::from_ref(key)),
                FieldValueKind::String
                    | FieldValueKind::Number
                    | FieldValueKind::Date
                    | FieldValueKind::Boolean
            ) {
                return None;
            }
        }
        Some(Self::Composite { has_reference })
    }
}

fn metadata_reference(key: &PropertyKey) -> bool {
    key.namespace.as_deref() == Some(references::CONFIG)
        && key
            .name
            .split_once('.')
            .is_some_and(|(kind, _)| kind.ends_with("Ref") && references::type_kind(kind).is_some())
}

fn serialized_unresolved_data_path(text: &str) -> bool {
    // Designer preserves unresolved metadata references in this form on export.
    text.strip_prefix("1/").is_some_and(|path| {
        let references = path.strip_suffix("/-2").unwrap_or(path);
        references.split('/').all(|part| {
            part.strip_prefix("0:").is_some_and(|uuid| {
                uuid.len() == 36
                    && uuid.bytes().enumerate().all(|(index, byte)| {
                        if matches!(index, 8 | 13 | 18 | 23) {
                            byte == b'-'
                        } else {
                            byte.is_ascii_hexdigit()
                        }
                    })
            })
        })
    })
}

#[derive(Clone, Copy)]
pub(super) struct Context {
    binding: InputBinding,
    in_table: bool,
    manual_max_width: bool,
    manual_max_height: bool,
}

impl Context {
    pub(super) fn new(view: &ElementPresentation, element: &EffectiveElement) -> Option<Self> {
        let binding = InputBinding::new(view, element)?;
        let manual_max = |name| {
            element
                .scalar(name)
                .is_some_and(|value| matches!(value, "false" | "No" | "0"))
        };
        Some(Self {
            binding,
            in_table: view.in_table,
            manual_max_width: manual_max("AutoMaxWidth"),
            manual_max_height: manual_max("AutoMaxHeight"),
        })
    }
}

pub(super) fn definitions(
    context: Context,
) -> impl Iterator<Item = (&'static PaletteDefinition, bool)> {
    INPUT_FIELD
        .iter()
        .filter(move |setting| setting.rule.applies(context))
        .map(move |setting| {
            let available = setting
                .disabled_on
                .is_none_or(|rule| !rule.applies(context));
            // The native row is blank. The source property remains in the extra XML rows.
            let definition = if matches!(
                context.binding,
                InputBinding::Unbound {
                    serialized_path: true
                }
            ) && setting.definition.id == "DataPath"
            {
                &EMPTY_DATA_PATH
            } else {
                &setting.definition
            };
            (definition, available)
        })
}

#[derive(Clone, Copy)]
enum Rule {
    Always,
    Bound,
    Unbound,
    Text,
    Format,
    EditFormat,
    Numeric,
    Wrap,
    InputHint,
    ReferenceChoices,
    ChoiceForm,
    ValueList,
    ExtendedValueList,
    Column,
    OutsideTable,
    ManualMaxWidth,
    ManualMaxHeight,
}

impl Rule {
    fn applies(self, context: Context) -> bool {
        use InputBinding as B;
        let binding = context.binding;
        let text = matches!(
            binding,
            B::String | B::Unbound { .. } | B::Composite { .. } | B::ValueList { .. }
        );
        match self {
            Self::Always => true,
            Self::Bound => !matches!(binding, B::Unbound { .. }),
            Self::Unbound => matches!(binding, B::Unbound { .. }),
            Self::Text => text,
            Self::Format => {
                matches!(binding, B::ValueList { .. })
                    || context.in_table && !matches!(binding, B::String | B::Reference)
            }
            Self::EditFormat => !matches!(binding, B::String | B::Reference),
            Self::Numeric => matches!(
                binding,
                B::Number | B::Unbound { .. } | B::Composite { .. } | B::ValueList { .. }
            ),
            Self::Wrap => text || binding == B::Boolean && !context.in_table,
            Self::InputHint => text || matches!(binding, B::Boolean | B::Reference),
            Self::ReferenceChoices => matches!(
                binding,
                B::Reference
                    | B::Composite {
                        has_reference: true
                    }
                    | B::ValueList { .. }
            ),
            Self::ChoiceForm => binding == B::Reference,
            Self::ValueList => matches!(binding, B::ValueList { .. }),
            Self::ExtendedValueList => matches!(binding, B::ValueList { extended: true }),
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
    disabled_on: Option<Rule>,
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
        disabled_on: None,
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

    const fn disabled_on(mut self, rule: Rule) -> Self {
        self.disabled_on = Some(rule);
        self
    }

    const fn captions(mut self, captions: &'static [(&'static str, &'static str)]) -> Self {
        self.definition = self.definition.with_enum_captions(captions);
        self
    }
}

const fn event(id: &'static str) -> Setting {
    Setting {
        definition: super::common::event(id),
        rule: Rule::Always,
        disabled_on: None,
    }
}

const DATA_PATH: Setting = setting(
    "DataPath",
    PaletteCategory::Main,
    PaletteEditor::Binding,
    PlatformPropertyValue::Empty,
);
const EMPTY_DATA_PATH: PaletteDefinition =
    DATA_PATH.definition.with_source(SettingSource::Unmapped);

// Native Configurator 8.3.27, Version8_3_27, in ordinary forms and value table columns.
// Keep the rows in native order. A rule omits settings absent in the selected context.
const INPUT_FIELD: &[Setting] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteEditor::{
        Binding, Boolean, Color, Dialog, Enum, Font, Localized, Number, Picture, Shortcut, Text,
    };
    use PlatformPropertyValue::{Auto, AutoStretch, Choice, Empty, Unknown};
    use Rule::{Column, ExtendedValueList, Numeric, OutsideTable, ReferenceChoices, ValueList};
    use SettingSource::{Attribute, Property};

    const DEFAULT: PlatformPropertyValue = Choice("default");
    &[
        setting("Name", Main, Text, Unknown).from(SettingSource::Name),
        setting("Title", Main, Localized, Empty),
        setting("Type", Main, Enum, Unknown)
            .from(SettingSource::Kind)
            .on(Rule::Bound),
        setting("Type", Main, Enum, Empty)
            .from(SettingSource::Unmapped)
            .on(Rule::Unbound),
        DATA_PATH,
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
        setting("MultipleValuesExtendedEdit", Main, Boolean, UNCHECKED)
            .from(Property("ExtendedEditMultipleValues"))
            .on(ValueList),
        setting("Format", Main, Dialog, Empty).on(Rule::Format),
        setting("EditFormat", Main, Dialog, Empty).on(Rule::EditFormat),
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
        setting("ChoiceList", Usage, Dialog, Empty).disabled_on(ExtendedValueList),
        setting(
            "IncompleteChoiceMode",
            Usage,
            Enum,
            Choice("onPressingEnter"),
        ),
        setting("TextEdit", Usage, Boolean, CHECKED),
        setting("EditTextUpdate", Usage, Enum, Auto),
        setting("Wrap", Usage, Boolean, CHECKED).on(Rule::Wrap),
        setting("MarkNegatives", Usage, Enum, DEFAULT)
            .on(Numeric)
            .captions(TRISTATE),
        setting("AutoMarkIncomplete", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("AutoChoiceIncomplete", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ChoiceFoldersAndItems", Usage, Enum, Auto),
        setting("ChoiceParameterLinks", Usage, Dialog, Empty).on(ReferenceChoices),
        setting("ChoiceParameters", Usage, Dialog, Empty).on(ReferenceChoices),
        setting("QuickChoice", Usage, Enum, DEFAULT).captions(TRISTATE),
        setting("ChoiceHistoryOnInput", Usage, Enum, Auto),
        setting("ChoiceForm", Usage, Dialog, Empty).on(Rule::ChoiceForm),
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
        setting("InputHint", Usage, Localized, Empty).on(Rule::InputHint),
        setting("AllowInputEmptyMultipleValues", Usage, Boolean, UNCHECKED).on(ExtendedValueList),
        setting("AllowMultipleValuesDuplicates", Usage, Boolean, UNCHECKED).on(ExtendedValueList),
        setting(
            "ShowCheckBoxesInDropListWhenInputMultipleValues",
            Usage,
            Enum,
            DEFAULT,
        )
        .on(ExtendedValueList)
        .from(Property("ShowCheckBoxesInDropList"))
        .captions(TRISTATE),
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
        setting("MultipleValuesBackColor", Appearance, Color, Auto).on(ValueList),
        setting("MultipleValuesTextColor", Appearance, Color, Auto).on(ValueList),
        setting("MultipleValuesFont", Appearance, Font, Auto).on(ValueList),
        setting("MultipleValuesHyperlink", Appearance, Enum, DEFAULT)
            .on(ExtendedValueList)
            .captions(TRISTATE),
        setting("MultipleValuePictureSize", Appearance, Enum, Auto).on(ValueList),
        setting("MultipleValuePictureShape", Appearance, Enum, Auto).on(ValueList),
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
