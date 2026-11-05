//! Derive captions, binding types and appearance once in the shared backend; the UI only lays them out.

use super::{
    AppearanceDiagnostic, ComposedForm, EffectiveDefinition, EffectiveElement, ElementIdentity,
    ElementKind, FormDocument, FormResource, PropertySource, ResolvedColor, ResolvedFont,
    SourcedProperty,
    appearance::{self, Appearance},
};
use crate::project::{
    metadata_model::{LocalizedText, MetadataKind, MetadataProperty, MetadataValue, PropertyKey},
    metadata_parser::ParsedObject,
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
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldType {
    pub kind: FieldValueKind,
    pub length: Option<u32>,
    pub digits: Option<u32>,
    pub fraction: Option<u32>,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementPresentation {
    pub identity: ElementIdentity,
    pub grouping: FormGrouping,
    pub displayed: bool,
    pub mark_incomplete: bool,
    pub title_location: FieldTitleLocation,
    pub picture: Option<super::FormPicture>,
    pub caption: PresentationText,
    pub tooltip: Option<PresentationText>,
    pub footer_text: Option<PresentationText>,
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
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormPresentation {
    pub elements: Vec<ElementPresentation>,
    pub important_color: ResolvedColor,
    pub diagnostics: Vec<AppearanceDiagnostic>,
}

#[must_use]
pub fn present(
    form: &ComposedForm,
    documents: &[FormDocument],
    resources: &[FormResource],
) -> FormPresentation {
    let mut appearance = Appearance::new(documents, resources);
    let mut elements = Vec::new();
    visit(
        &form.root,
        VisitScope::default(),
        form,
        resources,
        &mut appearance,
        &mut elements,
    );
    FormPresentation {
        elements,
        important_color: appearance.color(None, "ImportantColor"),
        diagnostics: appearance.diagnostics,
    }
}
#[derive(Clone, Copy, Default)]
struct VisitScope<'a> {
    parent: Option<&'a EffectiveElement>,
    in_table: bool,
}

impl<'a> VisitScope<'a> {
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
        property(element, name).or_else(|| {
            self.pages_parent(element)
                .and_then(|parent| property(parent, name))
        })
    }
}

fn visit(
    element: &EffectiveElement,
    scope: VisitScope<'_>,
    form: &ComposedForm,
    resources: &[FormResource],
    appearance: &mut Appearance<'_>,
    output: &mut Vec<ElementPresentation>,
) {
    let (binding_caption, value_type, binding) = bind(element, form, resources);
    let binding_caption = binding_caption.map(|caption| column_caption(caption, scope.in_table));
    let command =
        property(element, "CommandName").and_then(|value| appearance::text(&value.property.value));
    let caption = property(element, "Title")
        .and_then(|value| literal(&value.property.value))
        .or_else(|| command.and_then(|command| command_caption(command, form, resources)))
        .or(binding_caption)
        .unwrap_or_else(|| PresentationText::Name {
            text: element.name.clone().unwrap_or_default(),
        });
    let group = element.kind == Some(ElementKind::UsualGroup);
    let tab = element.kind == Some(ElementKind::Pages) || scope.pages_parent(element).is_some();
    let font = appearance.font(property(element, "Font"), font_role(element));
    let title_font = appearance.font(
        scope.title_property(element, "TitleFont"),
        title_font_role(element),
    );
    let foreground = appearance.color(property(element, "TextColor"), text_color_role(element));
    let background = appearance.color(
        property(element, "BackColor"),
        if element.kind == Some(ElementKind::Button) {
            "ButtonBackColor"
        } else {
            "BackColor"
        },
    );
    let border = appearance.color(property(element, "BorderColor"), "BorderColor");
    let title_color = appearance.color(
        scope.title_property(element, "TitleTextColor"),
        if tab {
            "FieldTextColor"
        } else if group {
            "GroupTitleTextColor"
        } else {
            "FormTextColor"
        },
    );
    let choices = property(element, "ChoiceList")
        .map(|value| choices(&value.property.value, resources))
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| enum_choices(&value_type, resources));
    let auto_command_items = match element.kind {
        Some(ElementKind::Form) => super::commands::automatic(form, resources),
        Some(ElementKind::Table) => super::commands::table(element, &value_type),
        _ => Vec::new(),
    };
    let auto_commands = standard_form_commands(&auto_command_items);
    let mark_incomplete = incomplete::mark(element, binding.as_ref(), resources);
    output.push(ElementPresentation {
        identity: element.identity.clone(),
        grouping: grouping(element),
        displayed: is_displayed(element, binding.as_ref(), command, appearance)
            && (scope.parent.is_some_and(is_command_area)
                || !matches!(
                    element.kind,
                    Some(ElementKind::ButtonGroup | ElementKind::Popup)
                )),
        mark_incomplete,
        title_location: title_location(element),
        picture: super::pictures::resolve(element, resources, &mut appearance.diagnostics),
        caption,
        tooltip: property(element, "ToolTip").and_then(|value| literal(&value.property.value)),
        footer_text: property(element, "FooterText")
            .and_then(|value| literal(&value.property.value)),
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
    });
    for child in element.children.iter().chain(&element.auxiliary) {
        visit(
            child,
            VisitScope {
                parent: Some(element),
                in_table: scope.in_table || element.kind == Some(ElementKind::Table),
            },
            form,
            resources,
            appearance,
            output,
        );
    }
}

fn text_color_role(element: &EffectiveElement) -> &'static str {
    match element.kind {
        Some(ElementKind::Button)
            if matches!(
                scalar(&element.properties, "Type"),
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
        && property(element, "DefaultButton")
            .and_then(|value| appearance::text(&value.property.value))
            .is_some_and(|value| matches!(value, "true" | "1"))
    {
        "PrimaryButtonFont"
    } else {
        "NormalTextFont"
    }
}

fn title_font_role(element: &EffectiveElement) -> &'static str {
    if element.kind == Some(ElementKind::UsualGroup)
        && scalar(&element.properties, "Representation") != Some("None")
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
    command: Option<&str>,
    appearance: &mut Appearance<'_>,
) -> bool {
    let needs_binding = element.kind.is_some_and(|kind| {
        matches!(
            kind.family(),
            super::ElementFamily::Field | super::ElementFamily::Table
        )
    });
    if needs_binding
        && binding.is_none()
        && let Some(path) = property(element, "DataPath").filter(|property| {
            appearance::text(&property.property.value).is_some_and(|path| !path.is_empty())
        })
    {
        appearance.diagnostics.push(AppearanceDiagnostic {
            code: "binding_missing",
            origin: path.origin.clone(),
            target: None,
        });
    }
    (!needs_binding || binding.is_some())
        && property(element, "Visible").and_then(|value| appearance::text(&value.property.value))
            != Some("false")
        && super::commands::visible(command)
}

fn title_location(element: &EffectiveElement) -> FieldTitleLocation {
    match scalar(&element.properties, "TitleLocation") {
        Some("Left") => FieldTitleLocation::Left,
        Some("Right") => FieldTitleLocation::Right,
        Some("Top") => FieldTitleLocation::Top,
        Some("Bottom") => FieldTitleLocation::Bottom,
        Some("None") => FieldTitleLocation::None,
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

fn grouping(element: &EffectiveElement) -> FormGrouping {
    match property(element, "Group").and_then(|property| appearance::text(&property.property.value))
    {
        Some("Horizontal") => FormGrouping::Horizontal,
        Some("HorizontalIfPossible") => FormGrouping::HorizontalIfPossible,
        Some("Vertical") => FormGrouping::Vertical,
        _ if matches!(
            element.kind,
            Some(
                ElementKind::UsualGroup
                    | ElementKind::ButtonGroup
                    | ElementKind::CommandBar
                    | ElementKind::Popup
            )
        ) =>
        {
            FormGrouping::Horizontal
        }
        _ => FormGrouping::Vertical,
    }
}

pub(super) fn property<'a>(
    element: &'a EffectiveElement,
    name: &str,
) -> Option<&'a SourcedProperty> {
    element.properties.iter().find(|value| {
        value.property.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
            && value.property.key.name == name
    })
}
fn scalar<'a>(values: &'a [SourcedProperty], name: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|value| {
            value.property.key.name == name
                && value.property.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
        })
        .and_then(|value| appearance::text(&value.property.value))
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
        .properties
        .iter()
        .find(|value| {
            value.property.key.name == "Type"
                && value.property.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
        })
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
            }
            if field.key.name == "DateQualifiers" {
                result.date_parts = record(&field.value, "DateFractions")
                    .and_then(|value| appearance::text(&value.value))
                    .map(str::to_owned);
            }
        }
    }
    result.kind = match result.types.as_slice() {
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
    };
    result
}
fn bind(
    element: &EffectiveElement,
    form: &ComposedForm,
    resources: &[FormResource],
) -> (Option<PresentationText>, FieldType, Option<PropertySource>) {
    let Some(path) =
        property(element, "DataPath").and_then(|value| appearance::text(&value.property.value))
    else {
        return (None, FieldType::default(), None);
    };
    let mut path = path.split('.');
    let Some(attribute) = path.next().and_then(|name| {
        form.attributes
            .iter()
            .find(|definition| definition.name.as_deref() == Some(name))
    }) else {
        return (None, FieldType::default(), None);
    };
    let remaining: Vec<_> = path.collect();
    if remaining.is_empty() {
        return (
            definition_caption(attribute),
            definition_type(attribute),
            Some(attribute.origin.clone()),
        );
    }
    if let Some(value) = column_binding(attribute, &remaining) {
        return value;
    }
    let kind = definition_type(attribute);
    let target = kind
        .types
        .iter()
        .filter(|kind| kind.namespace.as_deref() == Some(super::references::CONFIG))
        .find_map(|kind| {
            let (kind, name) = kind.name.split_once('.')?;
            super::resources::type_kind(kind).map(|kind| (kind, name))
        })
        .or_else(|| {
            attribute
                .properties
                .iter()
                .find_map(|property| find_main_table(&property.property.value))
        });
    let Some((kind, name)) = target else {
        return (None, FieldType::default(), Some(attribute.origin.clone()));
    };
    metadata_binding(kind, name, &remaining, resources)
        .unwrap_or_else(|| (None, FieldType::default(), Some(attribute.origin.clone())))
}
fn metadata_binding(
    kind: MetadataKind,
    name: &str,
    remaining: &[&str],
    resources: &[FormResource],
) -> Option<(Option<PresentationText>, FieldType, Option<PropertySource>)> {
    let (mut resource, mut owner) = resources.iter().rev().find_map(|resource| {
        resource
            .descriptor
            .objects
            .iter()
            .find(|object| object.metadata.kind() == kind && object.metadata.name() == name)
            .map(|owner| (resource, owner))
    })?;
    for (index, part) in remaining.iter().enumerate() {
        if let Some((layer, child)) = resources.iter().rev().find_map(|layer| {
            layer
                .descriptor
                .objects
                .iter()
                .find(|object| {
                    object.metadata.parent() == Some(owner.metadata.id())
                        && object.metadata.name() == *part
                })
                .map(|child| (layer, child))
        }) {
            resource = layer;
            owner = child;
        } else if index + 1 == remaining.len() && standard_attribute(part) {
            return Some((
                Some(standard_caption(part, owner, resources)),
                standard_type(part, owner, resources),
                Some(PropertySource {
                    source: resource.source.clone(),
                    range: owner.range.clone(),
                }),
            ));
        } else {
            return None;
        }
    }
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
    Some((
        Some(caption),
        value_type,
        Some(PropertySource {
            source: resource.source.clone(),
            range: owner.range.clone(),
        }),
    ))
}
fn definition_caption(value: &EffectiveDefinition) -> Option<PresentationText> {
    value
        .properties
        .iter()
        .find(|value| {
            value.property.key.name == "Title"
                && value.property.key.namespace.as_deref() == Some(super::FORM_NAMESPACE)
        })
        .and_then(|value| literal(&value.property.value))
        .or_else(|| {
            value
                .name
                .as_ref()
                .map(|text| PresentationText::Name { text: text.clone() })
        })
}
fn find_main_table(value: &MetadataValue) -> Option<(MetadataKind, &str)> {
    if let MetadataValue::Record(fields) = value {
        for field in fields {
            if field.key.name == "MainTable"
                && let Some((kind, name)) =
                    appearance::text(&field.value).and_then(|text| text.split_once('.'))
                && let Some(kind) = super::resources::type_kind(kind)
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
    resources
        .iter()
        .rev()
        .flat_map(|resource| &resource.descriptor.objects)
        .filter(|object| object.metadata.id() == owner.metadata.id())
        .find_map(|object| own_metadata_property(object, name))
}
fn own_metadata_property<'a>(owner: &'a ParsedObject, name: &str) -> Option<&'a MetadataProperty> {
    owner
        .properties
        .as_ref()?
        .iter()
        .find(|value| {
            value.property.key.name == name
                && value.property.key.namespace.as_deref() == Some("http://v8.1c.ru/8.3/MDClasses")
        })
        .map(|value| &value.property)
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
fn command_caption(
    command: &str,
    form: &ComposedForm,
    resources: &[FormResource],
) -> Option<PresentationText> {
    if let Some(name) = command.strip_prefix("Form.Command.") {
        return form
            .commands
            .iter()
            .find(|value| value.name.as_deref() == Some(name))
            .and_then(definition_caption);
    }
    if let Some(name) = command.strip_prefix("CommonCommand.") {
        return resources
            .iter()
            .rev()
            .flat_map(|resource| &resource.descriptor.objects)
            .find(|object| {
                object.metadata.kind() == MetadataKind::CommonCommand
                    && object.metadata.name() == name
            })
            .map(|object| PresentationText::Literal {
                items: object.synonyms.clone(),
            });
    }
    command
        .split_once(".StandardCommand.")
        .map(|(_, key)| PresentationText::Standard {
            key: format!("command.{key}"),
        })
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

            record(&item.value, "Value").and_then(|value| choice_reference(&value.value, resources))
        })
        .collect()
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
) -> Option<(Option<PresentationText>, FieldType, Option<PropertySource>)> {
    if let Some(columns) = attribute
        .properties
        .iter()
        .find(|value| value.property.key.name == "Columns")
    {
        let mut value = &columns.property.value;
        let mut found = None;
        for part in remaining {
            found = if let MetadataValue::Record(fields) = value {
                fields
                    .iter()
                    .find(|field| appearance::qualifier(field, "name") == Some(*part))
            } else {
                None
            };
            let Some(column) = found else {
                break;
            };
            value = record(&column.value, "Columns").map_or(&column.value, |field| &field.value);
        }
        if let Some(column) = found {
            return Some((
                record(&column.value, "Title").and_then(|value| literal(&value.value)),
                record(&column.value, "Type")
                    .map(|value| field_type(&value.value))
                    .unwrap_or_default(),
                Some(columns.origin.clone()),
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests;
