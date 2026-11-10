//! Designer 8.3.27 button palettes share command resolution with rendering.

use super::common::{CHECKED, UNCHECKED, ZERO};
use super::{
    PaletteCategory, PaletteDefinition, PaletteEditor, PlatformPropertyValue, SettingSource,
};
use crate::project::forms::{
    EffectiveElement, ElementKind, ElementPresentation, FormPreview, command_types, commands,
    presentation,
};

#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    command_bar: bool,
    element: &'a EffectiveElement,
    parameterized: bool,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        preview: &FormPreview,
        view: &ElementPresentation,
        element: &'a EffectiveElement,
    ) -> Option<Self> {
        if view.in_table {
            return None;
        }
        let command_bar = match element.scalar("Type") {
            Some("UsualButton" | "Hyperlink") => false,
            Some("CommandBarButton" | "CommandBarHyperlink") => true,
            None => matches!(
                view.parent_kind,
                Some(
                    ElementKind::CommandBar
                        | ElementKind::AutoCommandBar
                        | ElementKind::ButtonGroup
                        | ElementKind::Popup
                )
            ),
            _ => return None,
        };
        let command = element.scalar("CommandName").and_then(|name| {
            commands::ResolvedCommand::resolve(name, &preview.form, &preview.resources)
        });
        let parameterized = match command {
            Some(commands::ResolvedCommand::Local(command)) => command
                .property("ParameterType")
                .is_some_and(|property| command_types::has_parameter(&property.property.value)),
            Some(commands::ResolvedCommand::Metadata(command)) => {
                presentation::metadata_property(command, "CommandParameterType", &preview.resources)
                    .is_some_and(|property| command_types::has_parameter(&property.value))
            }
            _ => false,
        };
        Some(Self {
            command_bar,
            element,
            parameterized,
        })
    }
}

pub(super) fn definitions(
    context: Context<'_>,
) -> impl Iterator<Item = (&'static PaletteDefinition, bool)> + '_ {
    BUTTON.iter().filter_map(move |definition| {
        let definition = match definition.id {
            "Type" if context.command_bar => &BAR_TYPE,
            "MaxWidth"
                if !matches!(context.element.scalar("AutoMaxWidth"), Some("false" | "0")) =>
            {
                return None;
            }
            "MaxHeight"
                if !matches!(context.element.scalar("AutoMaxHeight"), Some("false" | "0")) =>
            {
                return None;
            }
            "Data" | "Parameter" if !context.parameterized => return None,
            _ => definition,
        };
        Some((definition, true))
    })
}

const TYPE_CAPTIONS: &[(&str, &str)] = &[
    ("UsualButton", "commonButton"),
    ("Hyperlink", "hyperlink"),
    ("CommandBarButton", "commandBarButton"),
    ("CommandBarHyperlink", "commandBarHyperlink"),
];

const BAR_TYPE: PaletteDefinition = PaletteDefinition::new(
    "Type",
    PaletteCategory::Main,
    PaletteEditor::Enum,
    PlatformPropertyValue::Empty,
)
.with_enum_captions(TYPE_CAPTIONS);

const BUTTON: &[PaletteDefinition] = {
    use PaletteCategory::{Appearance, Layout, Main, Usage};
    use PaletteDefinition as Row;
    use PaletteEditor::{
        Boolean, Color, Dialog, Enum, Font, Localized, Number, Picture, Reference, Text,
    };
    use PlatformPropertyValue::{Auto, Choice, Empty, Unknown};
    use SettingSource::{Attribute, Property};

    &[
        Row::new("Name", Main, Text, Unknown).with_source(SettingSource::Name),
        Row::new("Title", Main, Localized, Empty),
        Row::new("Type", Main, Enum, Choice("commonButton")).with_enum_captions(TYPE_CAPTIONS),
        Row::new("CommandName", Main, Reference, Empty),
        Row::new("Visible", Main, Boolean, CHECKED),
        Row::new(
            "UserVisibility",
            Main,
            Dialog,
            PlatformPropertyValue::Dialog,
        )
        .with_source(Property("UserVisible")),
        Row::new("Enabled", Main, Boolean, CHECKED),
        Row::new("Data", Main, PaletteEditor::Binding, Empty).with_source(Property("DataPath")),
        Row::new("Parameter", Main, Dialog, Empty),
        Row::new("Representation", Main, Enum, Auto)
            .with_enum_captions(&[("Auto", "auto"), ("Text", "text")]),
        Row::new("Picture", Main, Picture, Empty),
        Row::new("DefaultButton", Main, Boolean, UNCHECKED),
        Row::new("SkipOnInput", Main, Enum, Choice("default"))
            .with_enum_captions(&[("true", "yes"), ("false", "no")]),
        Row::new("DefaultItem", Main, Boolean, UNCHECKED),
        Row::new("Check", Main, Boolean, UNCHECKED),
        Row::new("RepresentationInContextMenu", Main, Enum, Auto),
        Row::new("DisplayImportance", Main, Enum, Auto)
            .with_source(Attribute("DisplayImportance"))
            .with_enum_captions(&[("High", "highImportance")]),
        Row::new("LocationInCommandBar", Main, Enum, Auto),
        Row::new("CommandUniqueness", Main, Boolean, CHECKED),
        Row::new("OnMainServerUnavailableBehavior", Main, Enum, Auto)
            .with_source(Property("OnMainServerUnavalableBehavior")),
        Row::new("ToolTipRepresentation", Usage, Enum, Auto),
        Row::new("BackColor", Appearance, Color, Auto),
        Row::new("TextColor", Appearance, Color, Auto),
        Row::new("BorderColor", Appearance, Color, Auto),
        Row::new("Font", Appearance, Font, Auto),
        Row::new("Shape", Appearance, Enum, Auto)
            .with_enum_captions(&[("Usual", "standardShape"), ("Oval", "roundedShape")]),
        Row::new("ShapeRepresentation", Appearance, Enum, Auto),
        Row::new("PictureLocation", Appearance, Enum, Auto),
        Row::new("Width", Layout, Number, ZERO),
        Row::new("Height", Layout, Number, ZERO),
        Row::new("AutoMaxWidth", Layout, Boolean, CHECKED),
        Row::new("MaxWidth", Layout, Number, ZERO),
        Row::new("AutoMaxHeight", Layout, Boolean, CHECKED),
        Row::new("MaxHeight", Layout, Number, ZERO),
        Row::new("TitleHeight", Layout, Number, ZERO),
        Row::new("HorizontalAlignInGroup", Layout, Enum, Auto)
            .with_source(Property("GroupHorizontalAlign")),
        Row::new("VerticalAlignInGroup", Layout, Enum, Auto)
            .with_source(Property("GroupVerticalAlign")),
        Row::new("HorizontalStretch", Layout, Boolean, UNCHECKED),
        Row::new("VerticalStretch", Layout, Boolean, UNCHECKED),
    ]
};
