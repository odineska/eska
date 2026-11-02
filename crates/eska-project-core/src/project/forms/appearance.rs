//! Resolve style references and partial font overrides without coupling the model to browser CSS.

use serde::Serialize;

use super::{
    FormDocument, FormReference, FormResource, PropertySource, ReferenceSlot, SourcedProperty,
    references,
};
use crate::project::metadata_model::{MetadataKind, MetadataProperty, MetadataValue, PropertyKey};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFont {
    pub face: Option<String>,
    pub points: f64,
    pub weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub origins: Vec<PropertySource>,
}
impl Default for ResolvedFont {
    fn default() -> Self {
        Self {
            face: None,
            points: 10.0,
            weight: 400,
            italic: false,
            underline: false,
            strikeout: false,
            origins: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum ResolvedColor {
    Rgb(String),
    Role(String),
    Web(String),
    System(String),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceDiagnostic {
    pub code: &'static str,
    pub origin: PropertySource,
    pub target: Option<PropertyKey>,
}

pub(super) struct Appearance<'a> {
    documents: &'a [FormDocument],
    resources: &'a [FormResource],
    pub diagnostics: Vec<AppearanceDiagnostic>,
}
impl<'a> Appearance<'a> {
    pub const fn new(documents: &'a [FormDocument], resources: &'a [FormResource]) -> Self {
        Self {
            documents,
            resources,
            diagnostics: Vec::new(),
        }
    }

    pub fn font(&mut self, property: Option<&SourcedProperty>, role: &str) -> ResolvedFont {
        let mut visited = Vec::new();
        let mut result = Self::font_role(role);
        if let Some(value) = self.style_property(role) {
            result = self.resolve_font(&value, result, &mut visited);
        }
        if let Some(property) = property {
            result = self.resolve_font(property, result, &mut visited);
        }
        result
    }
    pub fn color(&mut self, property: Option<&SourcedProperty>, role: &str) -> ResolvedColor {
        let mut visited = Vec::new();
        if let Some(property) = property {
            self.resolve_color(property, role, &mut visited)
        } else if let Some(value) = self.style_property(role) {
            self.resolve_color(&value, role, &mut visited)
        } else {
            ResolvedColor::Role(role.into())
        }
    }
    fn font_role(role: &str) -> ResolvedFont {
        let points = match role {
            "SmallTextFont" => 8.0,
            "LargeTextFont" | "TitleFont" | "FormTitleFont" => 12.0,
            "ExtraLargeTextFont" => 14.0,
            _ => 10.0,
        };
        ResolvedFont {
            points,
            ..ResolvedFont::default()
        }
    }
    fn refs(&self, source: &str) -> &[FormReference] {
        self.documents
            .iter()
            .find(|document| document.source == source)
            .map(|document| document.references.as_slice())
            .or_else(|| {
                self.resources
                    .iter()
                    .find(|resource| resource.source == source)
                    .map(|resource| resource.references.as_slice())
            })
            .unwrap_or_default()
    }
    fn target(&self, property: &SourcedProperty, slot: ReferenceSlot) -> Option<PropertyKey> {
        self.refs(&property.origin.source)
            .iter()
            .find(|reference| reference.range == property.origin.range && reference.slot == slot)
            .map(|reference| reference.target.clone())
    }
    fn style_property(&self, name: &str) -> Option<SourcedProperty> {
        self.resources.iter().rev().find_map(|resource| {
            resource
                .descriptor
                .objects
                .iter()
                .filter(|object| object.metadata.kind() == MetadataKind::Style)
                .find_map(|object| {
                    object
                        .properties
                        .as_ref()?
                        .iter()
                        .find(|property| property.property.key.name == name)
                        .map(|value| SourcedProperty {
                            origin: PropertySource {
                                source: resource.source.clone(),
                                range: value.range.clone(),
                            },
                            property: value.property.clone(),
                        })
                })
        })
    }
    fn item(&self, target: &PropertyKey) -> Option<SourcedProperty> {
        let name = target
            .name
            .strip_prefix("StyleItem.")
            .unwrap_or(&target.name);
        self.resources.iter().rev().find_map(|resource| {
            resource
                .descriptor
                .objects
                .iter()
                .find(|object| {
                    object.metadata.kind() == MetadataKind::StyleItem
                        && object.metadata.name() == name
                })
                .and_then(|object| {
                    object
                        .properties
                        .as_ref()?
                        .iter()
                        .find(|value| value.property.key.name == "Value")
                })
                .map(|value| SourcedProperty {
                    origin: PropertySource {
                        source: resource.source.clone(),
                        range: value.range.clone(),
                    },
                    property: value.property.clone(),
                })
        })
    }
    fn problem(
        &mut self,
        code: &'static str,
        property: &SourcedProperty,
        target: Option<PropertyKey>,
    ) {
        self.diagnostics.push(AppearanceDiagnostic {
            code,
            origin: property.origin.clone(),
            target,
        });
    }
    fn resolve_font(
        &mut self,
        property: &SourcedProperty,
        inherited: ResolvedFont,
        visited: &mut Vec<PropertyKey>,
    ) -> ResolvedFont {
        let mut result = inherited;
        if let Some(target) = self.target(property, ReferenceSlot::Ref) {
            if visited.contains(&target) || visited.len() >= 32 {
                self.problem("style_cycle", property, Some(target));
                return result;
            }
            visited.push(target.clone());
            match target.namespace.as_deref() {
                Some(references::STYLE) if font_role(&target.name) => {
                    result = Self::font_role(&target.name);
                    if let Some(value) = self.style_property(&target.name) {
                        result = self.resolve_font(&value, result, visited);
                    }
                }
                Some(references::CONFIG | references::STYLE) => {
                    if let Some(value) = self.item(&target) {
                        result = self.resolve_font(&value, result, visited);
                    } else {
                        self.problem("style_missing", property, Some(target));
                    }
                }
                Some(references::SYSTEM_FONT) => result = ResolvedFont::default(),
                _ => self.problem("font_reference_unsupported", property, Some(target)),
            }
            visited.pop();
        }
        for (key, value) in &property.property.qualifiers {
            if key.namespace.is_some() {
                continue;
            }
            match key.name.as_str() {
                "face" if !value.is_empty() => result.face = Some(value.clone()),
                "height" => {
                    if let Ok(points) = value.parse::<f64>() {
                        if points > 0.0 && points <= 1024.0 {
                            result.points = points;
                        } else if !points.is_finite()
                            || ((points + 1.0).abs() > f64::EPSILON && points.abs() > f64::EPSILON)
                        {
                            self.problem("font_size_invalid", property, None);
                        }
                    }
                }
                "bold" => {
                    if let Ok(value) = value.parse() {
                        result.weight = if value { 700 } else { 400 };
                    }
                }
                "italic" => {
                    if let Ok(value) = value.parse() {
                        result.italic = value;
                    }
                }
                "underline" => {
                    if let Ok(value) = value.parse() {
                        result.underline = value;
                    }
                }
                "strikeout" => {
                    if let Ok(value) = value.parse() {
                        result.strikeout = value;
                    }
                }
                _ => {}
            }
        }
        if let Some(scale) =
            qualifier(&property.property, "scale").and_then(|value| value.parse::<f64>().ok())
        {
            if scale.is_finite() && scale > 0.0 && scale <= 1000.0 {
                result.points *= scale / 100.0;
            } else {
                self.problem("font_scale_invalid", property, None);
            }
        }
        result.origins.push(property.origin.clone());
        result
    }
    fn resolve_color(
        &mut self,
        property: &SourcedProperty,
        role: &str,
        visited: &mut Vec<PropertyKey>,
    ) -> ResolvedColor {
        if let Some(target) = self.target(property, ReferenceSlot::Text) {
            if visited.contains(&target) || visited.len() >= 32 {
                self.problem("style_cycle", property, Some(target));
                return ResolvedColor::Role(role.into());
            }
            visited.push(target.clone());
            let result = match target.namespace.as_deref() {
                Some(references::STYLE) if color_role(&target.name) => {
                    self.style_property(&target.name).map_or_else(
                        || ResolvedColor::Role(target.name.clone()),
                        |value| self.resolve_color(&value, &target.name, visited),
                    )
                }
                Some(references::CONFIG | references::STYLE) => {
                    if let Some(value) = self.item(&target) {
                        self.resolve_color(&value, role, visited)
                    } else {
                        self.problem("style_missing", property, Some(target));
                        ResolvedColor::Role(role.into())
                    }
                }
                Some(references::WEB_COLOR)
                    if target.name.chars().all(|value| value.is_ascii_alphabetic()) =>
                {
                    ResolvedColor::Web(target.name)
                }
                Some(references::WINDOWS_COLOR) => ResolvedColor::System(target.name),
                _ => {
                    self.problem("color_reference_unsupported", property, Some(target));
                    ResolvedColor::Role(role.into())
                }
            };
            visited.pop();
            return result;
        }
        let text = text(&property.property.value).unwrap_or_default();
        if text.len() == 7
            && text.starts_with('#')
            && text[1..].bytes().all(|value| value.is_ascii_hexdigit())
        {
            ResolvedColor::Rgb(text.to_ascii_lowercase())
        } else if text.is_empty() || text == "Auto" {
            ResolvedColor::Role(role.into())
        } else {
            self.problem("color_invalid", property, None);
            ResolvedColor::Role(role.into())
        }
    }
}

pub(super) fn text(value: &MetadataValue) -> Option<&str> {
    match value {
        MetadataValue::Text(value)
        | MetadataValue::QualifiedText { text: value, .. }
        | MetadataValue::TypedText { text: value, .. } => Some(value),
        _ => None,
    }
}
pub(super) fn qualifier<'a>(property: &'a MetadataProperty, name: &str) -> Option<&'a str> {
    property
        .qualifiers
        .iter()
        .find(|(key, _)| key.namespace.is_none() && key.name == name)
        .map(|(_, value)| value.as_str())
}
fn font_role(name: &str) -> bool {
    matches!(
        name,
        "NormalTextFont"
            | "SmallTextFont"
            | "LargeTextFont"
            | "ExtraLargeTextFont"
            | "TitleFont"
            | "FormTitleFont"
            | "TooltipFont"
            | "TextFont"
            | "ButtonFont"
            | "FieldFont"
            | "LabelFont"
            | "GroupFont"
    )
}
fn color_role(name: &str) -> bool {
    matches!(
        name,
        "TextColor"
            | "BackColor"
            | "BorderColor"
            | "ButtonTextColor"
            | "ButtonBackColor"
            | "ButtonBorderColor"
            | "FieldTextColor"
            | "FieldBackColor"
            | "FieldBorderColor"
            | "HyperlinkTextColor"
            | "AccentColor"
            | "ImportantTextColor"
            | "SpecialTextColor"
            | "FormBackColor"
            | "GroupTitleTextColor"
            | "SelectionBackColor"
            | "SelectionTextColor"
            | "ToolTipBackColor"
            | "FieldSelectionBackColor"
            | "FormTextColor"
            | "TableFooterBackColor"
            | "TooltipBackColor"
            | "TooltipTextColor"
            | "LinesColor"
            | "HeaderBackColor"
            | "HeaderTextColor"
    )
}

#[cfg(test)]
mod tests;
