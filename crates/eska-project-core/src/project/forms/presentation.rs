//! Derive captions, binding types and appearance once in the shared backend; the UI only lays them out.

use super::{
    AppearanceDiagnostic, ButtonRepresentation, ComposedForm, EffectiveDefinition,
    EffectiveElement, ElementFamily, ElementIdentity, ElementKind, FormDocument, FormResource,
    PropertySource, ResolvedColor, ResolvedFont, SourcedProperty, additions,
    appearance::{self, Appearance},
    commands::{ResolvedCommand, effective_metadata},
    composition::SourceItems,
    pictures, references,
};
use crate::project::{
    metadata_model::{
        LocalizedText, MetadataKind, MetadataProperty, MetadataValue, ObjectId, PropertyKey,
    },
    metadata_parser::{LocatedProperty, MD_NAMESPACE, ParsedObject},
};
use serde::Serialize;

mod incomplete;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PresentationText {
    Literal { items: Vec<LocalizedText> },
    Name { text: String },
    Standard { key: String },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldValueKind {
    String,
    Number,
    Boolean,
    Date,
    Reference,
    Composite,
    Table,
    Document,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum NumberSign {
    Any,
    Nonnegative,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldType {
    pub kind: FieldValueKind,
    pub length: Option<u32>,
    pub digits: Option<u32>,
    pub fraction: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_sign: Option<NumberSign>,
    pub date_parts: Option<String>,
    pub types: Vec<PropertyKey>,
}
impl Default for FieldType {
    fn default() -> Self {
        Self {
            kind: FieldValueKind::Unknown,
            length: None,
            digits: None,
            fraction: None,
            allowed_sign: None,
            date_parts: None,
            types: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum FormGrouping {
    Vertical,
    Horizontal,
    HorizontalIfPossible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum FieldTitleLocation {
    Left,
    Right,
    Top,
    Bottom,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TabularTotalCaption {
    Automatic,
    Overridden,
}

/// Explicit band appearance on a table column or column group.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumnAppearance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_background: Option<ResolvedColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_background: Option<ResolvedColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_foreground: Option<ResolvedColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_font: Option<ResolvedFont>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementPresentation {
    pub identity: ElementIdentity,
    #[serde(skip)]
    pub(super) in_table: bool,
    #[serde(skip)]
    pub(super) parent_kind: Option<ElementKind>,
    pub grouping: FormGrouping,
    pub united: Option<bool>,
    pub displayed: bool,
    pub mark_incomplete: bool,
    pub title_location: FieldTitleLocation,
    pub picture: Option<super::FormPicture>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_representation: Option<ButtonRepresentation>,
    pub caption: PresentationText,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabular_total: Option<TabularTotalCaption>,
    pub tooltip: Option<PresentationText>,
    pub footer_text: Option<PresentationText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_column_appearance: Option<TableColumnAppearance>,
    pub value_type: FieldType,
    pub binding: Option<PropertySource>,
    pub choices: Vec<PresentationText>,
    pub font: ResolvedFont,
    pub title_font: ResolvedFont,
    pub foreground: ResolvedColor,
    pub background: ResolvedColor,
    pub border: ResolvedColor,
    pub title_color: ResolvedColor,
    pub auto_commands: Vec<String>,
    pub auto_command_items: Vec<super::AutoCommandItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub object_command_menu: Vec<Vec<super::AutoCommandItem>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub table_command_menu: Vec<Vec<super::AutoCommandItem>>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormPresentation {
    pub elements: Vec<ElementPresentation>,
    pub important_color: ResolvedColor,
    pub activity_color: ResolvedColor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_variant: Option<FormScriptVariant>,
    pub diagnostics: Vec<AppearanceDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormScriptVariant {
    Russian,
    English,
}

#[must_use]
pub fn present(
    form: &ComposedForm,
    documents: &[FormDocument],
    resources: &[FormResource],
) -> FormPresentation {
    let mut appearance = Appearance::new(documents, resources);
    let mut elements = Vec::new();
    let sources = SourceItems::new(&form.root);
    visit(
        &form.root,
        VisitScope::default(),
        form,
        resources,
        &sources,
        &mut appearance,
        &mut elements,
    );
    FormPresentation {
        elements,
        important_color: appearance.color(None, "ImportantColor"),
        activity_color: appearance.color(None, "ActivityColor"),
        script_variant: script_variant(resources),
        diagnostics: appearance.diagnostics,
    }
}

fn script_variant(resources: &[FormResource]) -> Option<FormScriptVariant> {
    // Application resources start with the base configuration, even for a borrowed form.
    let config = resources
        .iter()
        .flat_map(|resource| &resource.descriptor.objects)
        .find(|object| object.metadata.kind() == MetadataKind::Configuration)?;
    let variant = config.properties.as_ref()?.iter().find(|entry| {
        let key = &entry.property.key;
        key.name == "ScriptVariant" && key.namespace.as_deref() == Some(MD_NAMESPACE)
    })?;
    match appearance::text(&variant.property.value)? {
        "Russian" => Some(FormScriptVariant::Russian),
        "English" => Some(FormScriptVariant::English),
        _ => None,
    }
}
#[derive(Clone, Copy, Default)]
struct VisitScope<'a> {
    parent: Option<&'a EffectiveElement>,
    table: Option<&'a EffectiveElement>,
    in_table: bool,
}

impl<'a> VisitScope<'a> {
    fn fonts(
        self,
        element: &EffectiveElement,
        appearance: &mut Appearance<'_>,
    ) -> (ResolvedFont, ResolvedFont) {
        let inherited = self.column_font(element);
        let font = appearance.font(element.property("Font").or(inherited), font_role(element));
        let title = appearance.font(
            self.title_property(element, "TitleFont").or(inherited),
            title_font_role(element),
        );
        (font, title)
    }

    fn column_font(self, element: &EffectiveElement) -> Option<&'a SourcedProperty> {
        let kind = element.kind?;
        if (kind != ElementKind::ColumnGroup && kind.family() != ElementFamily::Field)
            || !self.parent.is_some_and(|parent| {
                matches!(
                    parent.kind,
                    Some(ElementKind::Table | ElementKind::ColumnGroup)
                )
            })
        {
            return None;
        }
        // Table Font supplies column value/header defaults, not toolbar or addition fonts.
        self.table.and_then(|table| table.property("Font"))
    }

    fn pages_parent(self, element: &EffectiveElement) -> Option<&'a EffectiveElement> {
        self.parent.filter(|parent| {
            element.kind == Some(ElementKind::Page) && parent.kind == Some(ElementKind::Pages)
        })
    }

    fn title_property<'b>(
        self,
        element: &'b EffectiveElement,
        name: &str,
    ) -> Option<&'b SourcedProperty>
    where
        'a: 'b,
    {
        element.property(name).or_else(|| {
            self.pages_parent(element)
                .and_then(|parent| parent.property(name))
        })
    }
}

fn visit(
    element: &EffectiveElement,
    scope: VisitScope<'_>,
    form: &ComposedForm,
    resources: &[FormResource],
    sources: &SourceItems<'_>,
    appearance: &mut Appearance<'_>,
    output: &mut Vec<ElementPresentation>,
) {
    output.push(present_element(
        element, scope, form, resources, sources, appearance,
    ));
    let child_scope = VisitScope {
        parent: Some(element),
        table: if element.kind == Some(ElementKind::Table) {
            Some(element)
        } else {
            scope.table
        },
        in_table: scope.in_table || element.kind == Some(ElementKind::Table),
    };
    for child in element.children.iter().chain(&element.auxiliary) {
        visit(
            child,
            child_scope,
            form,
            resources,
            sources,
            appearance,
            output,
        );
    }
}

fn present_element(
    element: &EffectiveElement,
    scope: VisitScope<'_>,
    form: &ComposedForm,
    resources: &[FormResource],
    sources: &SourceItems<'_>,
    appearance: &mut Appearance<'_>,
) -> ElementPresentation {
    let FieldBinding {
        caption: binding_caption,
        value_type,
        source: binding,
        tabular_total,
    } = bind(element, form, resources);
    let binding_caption = binding_caption.map(|caption| column_caption(caption, scope.in_table));
    let command_name = element.scalar("CommandName");
    let command = command_name.and_then(|name| ResolvedCommand::resolve(name, form, resources));
    let authored_caption = element
        .property("Title")
        .and_then(|value| literal(&value.property.value))
        .or_else(|| {
            command
                .as_ref()
                .and_then(|command| command.caption(resources))
        });
    let tabular_total = tabular_total.then_some(if authored_caption.is_some() {
        TabularTotalCaption::Overridden
    } else {
        TabularTotalCaption::Automatic
    });
    let caption = authored_caption
        .or(binding_caption)
        .unwrap_or_else(|| PresentationText::Name {
            text: if element.kind == Some(ElementKind::LabelDecoration) {
                String::new()
            } else {
                element.name.clone().unwrap_or_default()
            },
        });
    let (font, title_font) = scope.fonts(element, appearance);
    let (foreground, background, border, title_color) = element_colors(element, scope, appearance);
    let choices = element
        .property("ChoiceList")
        .map(|value| choices(&value.property.value, resources))
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| enum_choices(&value_type, resources));
    let (picture, button_representation) = element_picture(
        element,
        scope,
        command.as_ref(),
        resources,
        &mut appearance.diagnostics,
    );
    let super::root_commands::RootCommands {
        inline: auto_command_items,
        object_menu: object_command_menu,
    } = element_commands(
        element,
        &value_type,
        form,
        resources,
        sources,
        &mut appearance.diagnostics,
    );
    let auto_commands = standard_form_commands(&auto_command_items);
    let mark_incomplete = incomplete::mark(element, binding.as_ref(), resources);
    ElementPresentation {
        identity: element.identity.clone(),
        in_table: scope.in_table,
        parent_kind: scope.parent.and_then(|parent| parent.kind),
        grouping: grouping(element),
        united: group_united(element),
        displayed: is_displayed(element, binding.as_ref(), appearance)
            && super::commands::visible(sources, command_name)
            && additions::available(element, sources)
            && (scope.parent.is_some_and(is_command_area)
                || !matches!(
                    element.kind,
                    Some(ElementKind::ButtonGroup | ElementKind::Popup)
                )),
        mark_incomplete,
        title_location: title_location(element),
        picture,
        button_representation,
        caption,
        tabular_total,
        tooltip: element
            .property("ToolTip")
            .and_then(|value| literal(&value.property.value)),
        footer_text: element
            .property("FooterText")
            .and_then(|value| literal(&value.property.value)),
        table_column_appearance: table_column_appearance(element, scope, appearance),
        table_command_menu: table_command_menu(element, &value_type, form, resources, sources),
        value_type,
        binding,
        choices,
        font,
        title_font,
        foreground,
        background,
        border,
        title_color,
        auto_commands,
        auto_command_items,
        object_command_menu,
    }
}

fn element_colors(
    element: &EffectiveElement,
    scope: VisitScope<'_>,
    appearance: &mut Appearance<'_>,
) -> (ResolvedColor, ResolvedColor, ResolvedColor, ResolvedColor) {
    let foreground = appearance.color(element.property("TextColor"), text_color_role(element));
    let background = appearance.color(
        element.property("BackColor"),
        if element.kind == Some(ElementKind::Button) {
            "ButtonBackColor"
        } else {
            "BackColor"
        },
    );
    let border = appearance.color(element.property("BorderColor"), "BorderColor");
    let tab = element.kind == Some(ElementKind::Pages) || scope.pages_parent(element).is_some();
    let title_role = if tab {
        "FieldTextColor"
    } else if element.kind == Some(ElementKind::UsualGroup) {
        "GroupTitleTextColor"
    } else {
        "FormTextColor"
    };
    let title_color = appearance.color(scope.title_property(element, "TitleTextColor"), title_role);
    (foreground, background, border, title_color)
}

fn element_picture(
    element: &EffectiveElement,
    scope: VisitScope<'_>,
    command: Option<&ResolvedCommand<'_>>,
    resources: &[FormResource],
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> (Option<super::FormPicture>, Option<ButtonRepresentation>) {
    let button = element.kind == Some(ElementKind::Button);
    let picture = if let Some(property) = element.property("Picture") {
        pictures::form_picture(property, resources, diagnostics)
    } else if button {
        command.and_then(|command| command.picture(resources, diagnostics))
    } else {
        None
    };
    let representation = button.then(|| {
        let automatic = match element.scalar("Type") {
            Some("CommandBarButton" | "CommandBarHyperlink") => ButtonRepresentation::Picture,
            Some("Hyperlink") => ButtonRepresentation::PictureAndText,
            None if scope.parent.is_some_and(is_command_area) => ButtonRepresentation::Picture,
            _ => ButtonRepresentation::Text,
        };
        ButtonRepresentation::effective(
            picture.as_ref(),
            element.scalar("Representation"),
            command.and_then(|command| command.representation(resources)),
            automatic,
        )
    });
    (picture, representation)
}

fn table_column_appearance(
    element: &EffectiveElement,
    scope: VisitScope<'_>,
    appearance: &mut Appearance<'_>,
) -> Option<TableColumnAppearance> {
    let kind = element.kind?;
    if !scope.in_table
        || (kind != ElementKind::ColumnGroup && kind.family() != ElementFamily::Field)
    {
        return None;
    }
    let header_background = element.property("TitleBackColor");
    let footer_background = element.property("FooterBackColor");
    let footer_foreground = element.property("FooterTextColor");
    let footer_font = element.property("FooterFont");
    if [
        header_background,
        footer_background,
        footer_foreground,
        footer_font,
    ]
    .into_iter()
    .all(|property| property.is_none())
    {
        return None;
    }
    Some(TableColumnAppearance {
        header_background: header_background
            .map(|value| appearance.color(Some(value), "HeaderBackColor")),
        footer_background: footer_background
            .map(|value| appearance.color(Some(value), "TableFooterBackColor")),
        footer_foreground: footer_foreground
            .map(|value| appearance.color(Some(value), "FormTextColor")),
        footer_font: footer_font.map(|value| appearance.font(Some(value), "NormalTextFont")),
    })
}

fn text_color_role(element: &EffectiveElement) -> &'static str {
    match element.kind {
        Some(ElementKind::Button)
            if matches!(
                element.scalar("Type"),
                Some("Hyperlink" | "CommandBarHyperlink")
            ) =>
        {
            "HyperlinkTextColor"
        }
        Some(ElementKind::Button) => "ButtonTextColor",
        Some(ElementKind::InputField) => "FieldTextColor",
        Some(ElementKind::LabelDecoration | ElementKind::PictureDecoration) => "TextColor",
        _ => "FormTextColor",
    }
}

fn font_role(element: &EffectiveElement) -> &'static str {
    if element.kind == Some(ElementKind::Button)
        && element
            .scalar("DefaultButton")
            .is_some_and(|value| matches!(value, "true" | "1"))
    {
        "PrimaryButtonFont"
    } else {
        "NormalTextFont"
    }
}

fn title_font_role(element: &EffectiveElement) -> &'static str {
    if element.kind == Some(ElementKind::UsualGroup)
        && element.scalar("Representation") != Some("None")
    {
        "LargeTextFont"
    } else {
        "NormalTextFont"
    }
}

const fn is_command_area(element: &EffectiveElement) -> bool {
    matches!(
        element.kind,
        Some(
            ElementKind::CommandBar
                | ElementKind::AutoCommandBar
                | ElementKind::ButtonGroup
                | ElementKind::Popup
        )
    )
}

fn column_caption(caption: PresentationText, in_table: bool) -> PresentationText {
    // The same standard attribute has a shorter design-time column heading.
    match caption {
        PresentationText::Standard { key } if in_table && key == "attribute.LineNumber" => {
            PresentationText::Standard {
                key: "table.LineNumber".into(),
            }
        }
        caption => caption,
    }
}

fn element_commands(
    element: &EffectiveElement,
    value_type: &FieldType,
    form: &ComposedForm,
    resources: &[FormResource],
    sources: &SourceItems<'_>,
    diagnostics: &mut Vec<AppearanceDiagnostic>,
) -> super::root_commands::RootCommands {
    use super::root_commands::{self, RootCommands};

    let inline = match element.kind {
        Some(ElementKind::Form) => return root_commands::project(form, resources, diagnostics),
        Some(ElementKind::CommandBar) if element.scalar("CommandSource") == Some("Form") => {
            return root_commands::project_standalone(form, element, resources);
        }
        Some(ElementKind::Table) => super::commands::table(element, value_type),
        Some(ElementKind::CommandBar) => super::commands::table_source(element, sources)
            .map(|table| {
                let binding = bind(table, form, resources);
                super::commands::table_items(table, &binding.value_type)
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    RootCommands {
        inline,
        ..RootCommands::default()
    }
}

fn table_command_menu(
    element: &EffectiveElement,
    value_type: &FieldType,
    form: &ComposedForm,
    resources: &[FormResource],
    sources: &SourceItems<'_>,
) -> Vec<Vec<super::AutoCommandItem>> {
    let table = match element.kind {
        Some(ElementKind::Table) if super::commands::autofill(element) => element,
        Some(ElementKind::CommandBar) => {
            let Some(table) = super::commands::table_source(element, sources) else {
                return Vec::new();
            };
            table
        }
        _ => return Vec::new(),
    };
    let binding;
    let value_type = if element.kind == Some(ElementKind::Table) {
        value_type
    } else {
        binding = bind(table, form, resources);
        &binding.value_type
    };
    if !super::commands::is_dynamic_list(value_type) {
        return Vec::new();
    }
    let source = binding_path(table, form)
        .filter(|(_, remaining)| remaining.is_empty())
        .and_then(|(attribute, _)| main_table_id(attribute))
        .and_then(|id| effective_metadata(&id, resources))
        .map(|(_, owner)| owner);
    super::table_commands::sections(table, source, resources)
}

fn standard_form_commands(items: &[super::AutoCommandItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| {
            item.key
                .strip_prefix("Form.StandardCommand.")
                .map(str::to_owned)
        })
        .collect()
}

fn is_displayed(
    element: &EffectiveElement,
    binding: Option<&PropertySource>,
    appearance: &mut Appearance<'_>,
) -> bool {
    let needs_binding = element.kind.is_some_and(|kind| {
        matches!(
            kind.family(),
            super::ElementFamily::Field | super::ElementFamily::Table
        )
    });
    // Designer retains unbound buttons in the tree but omits their preview widgets.
    let command_bound = element.kind != Some(ElementKind::Button)
        || element
            .scalar("CommandName")
            .is_some_and(|name| !matches!(name.trim(), "" | "0"));
    if needs_binding
        && binding.is_none()
        && let Some(path) = element
            .property("DataPath")
            .filter(|property| property.text().is_some_and(|path| !path.is_empty()))
    {
        appearance.diagnostics.push(AppearanceDiagnostic {
            code: "binding_missing",
            origin: path.origin.clone(),
            target: None,
        });
    }
    command_bound
        && (!needs_binding || binding.is_some())
        && element.scalar("Visible") != Some("false")
}

fn title_location(element: &EffectiveElement) -> FieldTitleLocation {
    match element.scalar("TitleLocation") {
        Some("Left") => FieldTitleLocation::Left,
        Some("Right") => FieldTitleLocation::Right,
        Some("Top") => FieldTitleLocation::Top,
        Some("Bottom") => FieldTitleLocation::Bottom,
        Some("None") => FieldTitleLocation::None,
        _ if element.kind == Some(ElementKind::InputField)
            && matches!(element.scalar("MultiLine"), Some("true" | "1"))
            && !element
                .scalar("Height")
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|height| height > 0.0 && height <= 1.0) =>
        {
            FieldTitleLocation::Top
        }
        _ if matches!(
            element.kind,
            Some(
                ElementKind::CalendarField
                    | ElementKind::ChartField
                    | ElementKind::DendrogramField
                    | ElementKind::FormattedDocumentField
                    | ElementKind::GanttChartField
                    | ElementKind::GeographicalSchemaField
                    | ElementKind::GraphicalSchemaField
                    | ElementKind::HtmlDocumentField
                    | ElementKind::PdfDocumentField
                    | ElementKind::PeriodField
                    | ElementKind::PictureField
                    | ElementKind::PlannerField
                    | ElementKind::SpreadsheetDocumentField
                    | ElementKind::TextDocumentField
                    | ElementKind::TrackBarField
            )
        ) =>
        {
            FieldTitleLocation::Top
        }
        _ => FieldTitleLocation::Left,
    }
}

fn group_united(element: &EffectiveElement) -> Option<bool> {
    (element.kind == Some(ElementKind::UsualGroup)).then(|| match element.scalar("United") {
        Some("true" | "1") => true,
        Some("false" | "0") => false,
        _ => super::DEFAULT_GROUP_UNITED,
    })
}

fn grouping(element: &EffectiveElement) -> FormGrouping {
    match element.scalar("Group") {
        Some("Horizontal" | "AlwaysHorizontal") => FormGrouping::Horizontal,
        Some("HorizontalIfPossible") => FormGrouping::HorizontalIfPossible,
        Some("Vertical") => FormGrouping::Vertical,
        _ if element.kind == Some(ElementKind::UsualGroup) => FormGrouping::HorizontalIfPossible,
        _ if matches!(
            element.kind,
            Some(ElementKind::ButtonGroup | ElementKind::CommandBar | ElementKind::Popup)
        ) =>
        {
            FormGrouping::Horizontal
        }
        _ => FormGrouping::Vertical,
    }
}

fn record<'a>(value: &'a MetadataValue, name: &str) -> Option<&'a MetadataProperty> {
    if let MetadataValue::Record(fields) = value {
        fields.iter().find(|value| value.key.name == name)
    } else {
        None
    }
}

/// Localized records are namespace-aware; a foreign `item` cannot supply a platform caption.
pub(super) fn literal(value: &MetadataValue) -> Option<PresentationText> {
    let items = match value {
        MetadataValue::Localized(items) => items.clone(),
        MetadataValue::Text(text) if !text.is_empty() => {
            return Some(PresentationText::Name { text: text.clone() });
        }
        MetadataValue::Record(fields) => fields
            .iter()
            .filter(|field| {
                field.key.namespace.as_deref() == Some("http://v8.1c.ru/8.1/data/core")
                    && field.key.name == "item"
            })
            .filter_map(|item| {
                let language =
                    record(&item.value, "lang").and_then(|value| appearance::text(&value.value))?;
                let content = record(&item.value, "content")
                    .and_then(|value| appearance::text(&value.value))?;
                Some(LocalizedText {
                    language: language.into(),
                    content: content.into(),
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    (!items.is_empty()).then_some(PresentationText::Literal { items })
}
pub(super) fn definition_type(definition: &EffectiveDefinition) -> FieldType {
    definition
        .property("Type")
        .map(|value| field_type(&value.property.value))
        .unwrap_or_default()
}
pub(super) fn field_type(value: &MetadataValue) -> FieldType {
    let mut result = FieldType::default();
    if let MetadataValue::Record(fields) = value {
        for field in fields {
            if field.key.name == "Type"
                && let MetadataValue::QualifiedText { key, .. } = &field.value
            {
                result.types.push(key.clone());
            }
            if field.key.name == "StringQualifiers" {
                result.length = record(&field.value, "Length")
                    .and_then(|value| appearance::text(&value.value))
                    .and_then(|value| value.parse().ok());
            }
            if field.key.name == "NumberQualifiers" {
                result.digits = record(&field.value, "Digits")
                    .and_then(|value| appearance::text(&value.value))
                    .and_then(|value| value.parse().ok());
                result.fraction = record(&field.value, "FractionDigits")
                    .and_then(|value| appearance::text(&value.value))
                    .and_then(|value| value.parse().ok());
                result.allowed_sign = record(&field.value, "AllowedSign")
                    .and_then(|value| appearance::text(&value.value))
                    .and_then(|value| match value {
                        "Any" => Some(NumberSign::Any),
                        "Nonnegative" => Some(NumberSign::Nonnegative),
                        _ => None,
                    });
            }
            if field.key.name == "DateQualifiers" {
                result.date_parts = record(&field.value, "DateFractions")
                    .and_then(|value| appearance::text(&value.value))
                    .map(str::to_owned);
            }
        }
    }
    result.kind = field_value_kind(&result.types);
    result
}

pub(super) fn field_value_kind(types: &[PropertyKey]) -> FieldValueKind {
    match types {
        [] => FieldValueKind::Unknown,
        [kind] if kind.namespace.as_deref() == Some("http://www.w3.org/2001/XMLSchema") => {
            match kind.name.as_str() {
                "string" => FieldValueKind::String,
                "boolean" => FieldValueKind::Boolean,
                "decimal" | "double" | "int" | "integer" => FieldValueKind::Number,
                "dateTime" | "date" | "time" => FieldValueKind::Date,
                _ => FieldValueKind::Unknown,
            }
        }
        [kind]
            if kind.namespace.as_deref() == Some("http://v8.1c.ru/8.1/data/core")
                && matches!(kind.name.as_str(), "ValueTable" | "ValueTree" | "ValueList")
                || kind.namespace.as_deref() == Some(super::references::CONFIG)
                    && kind.name == "DynamicList" =>
        {
            FieldValueKind::Table
        }
        [kind] if kind.namespace.as_deref() == Some(super::references::CONFIG) => {
            FieldValueKind::Reference
        }
        [_] => FieldValueKind::Document,
        _ => FieldValueKind::Composite,
    }
}
#[derive(Default)]
struct FieldBinding {
    caption: Option<PresentationText>,
    value_type: FieldType,
    source: Option<PropertySource>,
    tabular_total: bool,
}

fn bind(
    element: &EffectiveElement,
    form: &ComposedForm,
    resources: &[FormResource],
) -> FieldBinding {
    let Some((attribute, remaining)) = binding_path(element, form) else {
        return FieldBinding::default();
    };
    resolve_binding(attribute, &remaining, resources).unwrap_or_else(|_| FieldBinding {
        source: Some(attribute.origin.clone()),
        ..FieldBinding::default()
    })
}

fn binding_path<'e, 'f>(
    element: &'e EffectiveElement,
    form: &'f ComposedForm,
) -> Option<(&'f EffectiveDefinition, Vec<&'e str>)> {
    let mut path = element.scalar("DataPath")?.split('.');
    let name = path.next()?;
    let attribute = form
        .attributes
        .iter()
        .find(|definition| definition.name.as_deref() == Some(name))?;
    Some((attribute, path.collect()))
}

/// Only follow reference types on paths used by the composed form.
pub(super) fn binding_resource_requests(
    form: &ComposedForm,
    resources: &[FormResource],
) -> Vec<ObjectId> {
    let mut pending = vec![&form.root];
    let mut requests = Vec::new();
    while let Some(element) = pending.pop() {
        pending.extend(element.children.iter().chain(&element.auxiliary));
        let Some((attribute, remaining)) = binding_path(element, form) else {
            continue;
        };
        if element.kind == Some(ElementKind::Table)
            && remaining.is_empty()
            && super::commands::is_dynamic_list(&definition_type(attribute))
            && let Some(id) = main_table_id(attribute)
            && let Some((_, source)) = effective_metadata(&id, resources)
        {
            requests.extend(super::table_commands::creation_requests(source, resources));
        }
        if remaining.len() > 1
            && let Err(BindingFailure::Unloaded(target)) =
                resolve_binding(attribute, &remaining, resources)
        {
            requests.push(target);
        }
    }
    requests
}

enum BindingFailure {
    Unresolved,
    Unloaded(ObjectId),
}

fn resolve_binding(
    attribute: &EffectiveDefinition,
    remaining: &[&str],
    resources: &[FormResource],
) -> Result<FieldBinding, BindingFailure> {
    if remaining.is_empty() {
        return Ok(FieldBinding {
            caption: definition_caption(attribute),
            value_type: definition_type(attribute),
            source: Some(attribute.origin.clone()),
            tabular_total: false,
        });
    }
    if let Some((column, consumed)) = column_binding(attribute, remaining) {
        if consumed == remaining.len() {
            return Ok(column);
        }
        if let Some(target) = single_type_target(&column.value_type) {
            match metadata_binding(target, &remaining[consumed..], resources) {
                Err(BindingFailure::Unresolved) => {}
                binding => return binding,
            }
        }
    }
    let target = definition_type(attribute)
        .types
        .iter()
        .find_map(references::metadata_type_id)
        .or_else(|| main_table_id(attribute))
        .ok_or(BindingFailure::Unresolved)?;
    metadata_binding(target, remaining, resources)
}

fn single_type_target(value_type: &FieldType) -> Option<ObjectId> {
    let [key] = value_type.types.as_slice() else {
        return None;
    };
    references::metadata_type_id(key)
}

fn metadata_binding(
    target: ObjectId,
    remaining: &[&str],
    resources: &[FormResource],
) -> Result<FieldBinding, BindingFailure> {
    let (mut resource, mut owner) =
        effective_metadata(&target, resources).ok_or(BindingFailure::Unloaded(target))?;
    for (index, part) in remaining.iter().enumerate() {
        let terminal = index + 1 == remaining.len();
        if let Some((layer, child)) = metadata_child(owner, part, resources) {
            if terminal || child.metadata.kind() == MetadataKind::TabularSection {
                (resource, owner) = (layer, child);
            } else {
                let target = metadata_property(child, "Type", resources)
                    .and_then(|property| single_type_target(&field_type(&property.value)))
                    .ok_or(BindingFailure::Unresolved)?;
                (resource, owner) = effective_metadata(&target, resources)
                    .ok_or(BindingFailure::Unloaded(target))?;
            }
        } else if !terminal {
            return Err(BindingFailure::Unresolved);
        } else if standard_attribute(part) {
            return Ok(FieldBinding {
                caption: Some(standard_caption(part, owner, resources)),
                value_type: standard_type(part, owner, resources),
                source: Some(PropertySource {
                    source: resource.source.clone(),
                    range: owner.range.clone(),
                }),
                tabular_total: false,
            });
        } else if owner.metadata.kind() == MetadataKind::TabularSection {
            // Designer exports the synthetic total as Total<column> in DataPath.
            let column_name = part
                .strip_prefix("Total")
                .ok_or(BindingFailure::Unresolved)?;
            let (layer, column) =
                metadata_child(owner, column_name, resources).ok_or(BindingFailure::Unresolved)?;
            let mut binding = metadata_field(layer, column, resources);
            if binding.value_type.kind != FieldValueKind::Number {
                return Err(BindingFailure::Unresolved);
            }
            binding.tabular_total = true;
            return Ok(binding);
        } else {
            return Err(BindingFailure::Unresolved);
        }
    }
    Ok(metadata_field(resource, owner, resources))
}

fn metadata_child<'a>(
    owner: &ParsedObject,
    name: &str,
    resources: &'a [FormResource],
) -> Option<(&'a FormResource, &'a ParsedObject)> {
    resources.iter().rev().find_map(|layer| {
        layer
            .descriptor
            .objects
            .iter()
            .find(|object| {
                object.metadata.parent() == Some(owner.metadata.id())
                    && object.metadata.name() == name
            })
            .map(|child| (layer, child))
    })
}

fn metadata_field(
    resource: &FormResource,
    owner: &ParsedObject,
    resources: &[FormResource],
) -> FieldBinding {
    let caption = metadata_property(owner, "Synonym", resources)
        .and_then(|value| literal(&value.value))
        .unwrap_or_else(|| PresentationText::Name {
            text: owner.metadata.name().into(),
        });
    let value_type = if owner.metadata.kind() == MetadataKind::TabularSection {
        FieldType {
            kind: FieldValueKind::Table,
            ..FieldType::default()
        }
    } else {
        metadata_property(owner, "Type", resources)
            .map(|value| field_type(&value.value))
            .unwrap_or_default()
    };
    FieldBinding {
        caption: Some(caption),
        value_type,
        source: Some(PropertySource {
            source: resource.source.clone(),
            range: owner.range.clone(),
        }),
        tabular_total: false,
    }
}
pub(super) fn definition_caption(value: &EffectiveDefinition) -> Option<PresentationText> {
    value
        .property("Title")
        .and_then(|value| literal(&value.property.value))
        .or_else(|| {
            value
                .name
                .as_ref()
                .map(|text| PresentationText::Name { text: text.clone() })
        })
}
fn main_table_id(attribute: &EffectiveDefinition) -> Option<ObjectId> {
    let (kind, name) = attribute
        .properties
        .iter()
        .find_map(|property| find_main_table(&property.property.value))?;
    Some(ObjectId::from_parts(None, kind.as_str(), name))
}

fn find_main_table(value: &MetadataValue) -> Option<(MetadataKind, &str)> {
    if let MetadataValue::Record(fields) = value {
        for field in fields {
            if field.key.name == "MainTable"
                && let Some((kind, name)) =
                    appearance::text(&field.value).and_then(|text| text.split_once('.'))
                && let Some(kind) = super::references::type_kind(kind)
            {
                return Some((kind, name));
            }
            if let Some(value) = find_main_table(&field.value) {
                return Some(value);
            }
        }
    }
    None
}
fn standard_attribute(name: &str) -> bool {
    matches!(
        name,
        "Code"
            | "Description"
            | "Parent"
            | "Ref"
            | "IsFolder"
            | "DeletionMark"
            | "Date"
            | "Number"
            | "Posted"
            | "Owner"
            | "LineNumber"
            | "Recorder"
            | "Period"
            | "Active"
            | "RecordType"
            | "Predefined"
            | "PredefinedDataName"
    )
}
pub(super) fn metadata_property<'a>(
    owner: &ParsedObject,
    name: &str,
    resources: &'a [FormResource],
) -> Option<&'a MetadataProperty> {
    located_metadata_property(owner, name, resources).map(|(_, value)| &value.property)
}
pub(super) fn located_metadata_property<'a>(
    owner: &ParsedObject,
    name: &str,
    resources: &'a [FormResource],
) -> Option<(&'a FormResource, &'a LocatedProperty)> {
    resources.iter().rev().find_map(|resource| {
        let object = resource
            .descriptor
            .objects
            .iter()
            .find(|object| object.metadata.id() == owner.metadata.id())?;
        own_metadata_located_property(object, name).map(|property| (resource, property))
    })
}
fn own_metadata_property<'a>(owner: &'a ParsedObject, name: &str) -> Option<&'a MetadataProperty> {
    own_metadata_located_property(owner, name).map(|value| &value.property)
}
fn own_metadata_located_property<'a>(
    owner: &'a ParsedObject,
    name: &str,
) -> Option<&'a LocatedProperty> {
    owner.properties.as_ref()?.iter().find(|value| {
        value.property.key.name == name
            && value.property.key.namespace.as_deref() == Some(MD_NAMESPACE)
    })
}
fn standard_property<'a>(
    owner: &ParsedObject,
    name: &str,
    property: &str,
    resources: &'a [FormResource],
) -> Option<&'a MetadataProperty> {
    const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
    resources
        .iter()
        .rev()
        .flat_map(|resource| &resource.descriptor.objects)
        .filter(|object| object.metadata.id() == owner.metadata.id())
        .find_map(|object| {
            let MetadataValue::Record(attributes) =
                &own_metadata_property(object, "StandardAttributes")?.value
            else {
                return None;
            };
            let attribute = attributes.iter().find(|field| {
                field.key.namespace.as_deref() == Some(READABLE)
                    && appearance::qualifier(field, "name") == Some(name)
            })?;
            let MetadataValue::Record(properties) = &attribute.value else {
                return None;
            };
            properties.iter().find(|field| {
                field.key.namespace.as_deref() == Some(READABLE) && field.key.name == property
            })
        })
}
fn standard_caption(
    name: &str,
    owner: &ParsedObject,
    resources: &[FormResource],
) -> PresentationText {
    if let Some(caption) =
        standard_property(owner, name, "Synonym", resources).and_then(|value| literal(&value.value))
    {
        return caption;
    }
    let folder_parent = name == "Parent"
        && metadata_property(owner, "HierarchyType", resources)
            .and_then(|value| appearance::text(&value.value))
            == Some("HierarchyFoldersAndItems");
    PresentationText::Standard {
        key: format!("attribute.{}", if folder_parent { "Folder" } else { name }),
    }
}
fn standard_type(name: &str, owner: &ParsedObject, resources: &[FormResource]) -> FieldType {
    let kind = match name {
        "IsFolder" | "DeletionMark" | "Posted" | "Active" | "Predefined" => FieldValueKind::Boolean,
        "Date" | "Period" => FieldValueKind::Date,
        "LineNumber" => FieldValueKind::Number,
        "Parent" | "Ref" | "Owner" | "Recorder" => FieldValueKind::Reference,
        "Code"
            if metadata_property(owner, "CodeType", resources)
                .and_then(|value| appearance::text(&value.value))
                == Some("Number") =>
        {
            FieldValueKind::Number
        }
        _ => FieldValueKind::String,
    };
    let length_key = match name {
        "Code" => "CodeLength",
        "Description" => "DescriptionLength",
        "Number" => "NumberLength",
        _ => "",
    };
    let length = metadata_property(owner, length_key, resources)
        .and_then(|value| appearance::text(&value.value))
        .and_then(|value| value.parse().ok());
    let digits = (name == "LineNumber").then(|| {
        metadata_property(owner, "LineNumberLength", resources)
            .and_then(|value| appearance::text(&value.value))
            .and_then(|value| value.parse().ok())
            .unwrap_or(9)
    });
    FieldType {
        kind,
        length,
        digits,
        ..FieldType::default()
    }
}
fn enum_choices(value_type: &FieldType, resources: &[FormResource]) -> Vec<PresentationText> {
    let Some(name) = value_type
        .types
        .iter()
        .find_map(|value| value.name.strip_prefix("EnumRef."))
    else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for resource in resources.iter().rev() {
        let Some(owner) = resource.descriptor.objects.iter().find(|object| {
            object.metadata.kind() == MetadataKind::Enum && object.metadata.name() == name
        }) else {
            continue;
        };
        for object in resource.descriptor.objects.iter().filter(|object| {
            object.metadata.kind() == MetadataKind::EnumValue
                && object.metadata.parent() == Some(owner.metadata.id())
        }) {
            output.push(if object.synonyms.is_empty() {
                PresentationText::Name {
                    text: object.metadata.name().into(),
                }
            } else {
                PresentationText::Literal {
                    items: object.synonyms.clone(),
                }
            });
        }
        if !output.is_empty() {
            return output;
        }
    }
    output
}
fn choices(value: &MetadataValue, resources: &[FormResource]) -> Vec<PresentationText> {
    let MetadataValue::Record(fields) = value else {
        return Vec::new();
    };
    fields
        .iter()
        .filter_map(|item| {
            if let Some(caption) =
                record(&item.value, "Presentation").and_then(|value| literal(&value.value))
            {
                return Some(caption);
            }

            design_time_choice(item, resources).or_else(|| {
                record(&item.value, "Value")
                    .and_then(|value| choice_reference(&value.value, resources))
            })
        })
        .collect()
}

fn design_time_choice(
    item: &MetadataProperty,
    resources: &[FormResource],
) -> Option<PresentationText> {
    const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";
    if item.key.namespace.as_deref() != Some(READABLE) || item.key.name != "Item" {
        return None;
    }
    let MetadataValue::Record(fields) = &item.value else {
        return None;
    };
    let wrapper = fields.iter().find(|field| {
        field.key.namespace.as_deref() == Some(READABLE) && field.key.name == "Value"
    })?;
    let MetadataValue::Record(fields) = &wrapper.value else {
        return None;
    };
    // Designer exports an empty outer presentation and the caption inside this value wrapper.
    let field = |name: &str| {
        fields
            .iter()
            .find(|field| {
                field.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
                    && field.key.name == name
            })
            .map(|field| &field.value)
    };
    if let Some(caption) = field("Presentation").and_then(literal) {
        return Some(caption);
    }
    let value = field("Value")?;
    if let MetadataValue::TypedText { text, key } = value
        && key.namespace.as_deref() == Some("http://www.w3.org/2001/XMLSchema")
        && key.name == "string"
    {
        return Some(PresentationText::Name { text: text.clone() });
    }
    choice_reference(value, resources)
}

fn choice_reference(value: &MetadataValue, resources: &[FormResource]) -> Option<PresentationText> {
    if let Some(text) = appearance::text(value)
        && let Some((name, value)) = text
            .strip_prefix("Enum.")
            .and_then(|text| text.split_once(".EnumValue."))
    {
        return resources.iter().rev().find_map(|resource| {
            let owner = resource.descriptor.objects.iter().find(|object| {
                object.metadata.kind() == MetadataKind::Enum && object.metadata.name() == name
            })?;
            let object = resource.descriptor.objects.iter().find(|object| {
                object.metadata.kind() == MetadataKind::EnumValue
                    && object.metadata.parent() == Some(owner.metadata.id())
                    && object.metadata.name() == value
            })?;
            Some(if object.synonyms.is_empty() {
                PresentationText::Name { text: value.into() }
            } else {
                PresentationText::Literal {
                    items: object.synonyms.clone(),
                }
            })
        });
    }
    if let MetadataValue::Record(fields) = value {
        fields
            .iter()
            .find_map(|field| choice_reference(&field.value, resources))
    } else {
        None
    }
}

fn column_binding(
    attribute: &EffectiveDefinition,
    remaining: &[&str],
) -> Option<(FieldBinding, usize)> {
    let columns = attribute
        .properties
        .iter()
        .find(|value| value.property.key.name == "Columns")?;
    let mut value = &columns.property.value;
    let mut found = None;
    for (index, part) in remaining.iter().enumerate() {
        let MetadataValue::Record(fields) = value else {
            break;
        };
        let Some(column) = fields
            .iter()
            .find(|field| appearance::qualifier(field, "name") == Some(*part))
        else {
            break;
        };
        found = Some((column, index + 1));
        value = record(&column.value, "Columns").map_or(&column.value, |field| &field.value);
    }
    let (column, consumed) = found?;
    Some((
        FieldBinding {
            caption: record(&column.value, "Title").and_then(|value| literal(&value.value)),
            value_type: record(&column.value, "Type")
                .map(|value| field_type(&value.value))
                .unwrap_or_default(),
            source: Some(columns.origin.clone()),
            tabular_total: false,
        },
        consumed,
    ))
}

#[cfg(test)]
mod tests;
