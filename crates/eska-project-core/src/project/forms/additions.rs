//! Source references of independently placed form additions.

use super::{
    EffectiveElement, ElementFamily, ElementKind, appearance, composition::SourceItems,
    is_form_property,
};
use crate::project::metadata_model::MetadataValue;

pub(super) struct Source<'a> {
    pub(super) item: &'a str,
    pub(super) kind: ElementKind,
}

pub(super) fn source(value: &MetadataValue) -> Option<Source<'_>> {
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
    let kind = match get("Type")? {
        "SearchStringRepresentation" => ElementKind::SearchStringAddition,
        "SearchControl" => ElementKind::SearchControlAddition,
        "ViewStatusRepresentation" => ElementKind::ViewStatusAddition,
        _ => return None,
    };
    Some(Source { item, kind })
}

pub(super) fn available(element: &EffectiveElement, items: &SourceItems<'_>) -> bool {
    if !element.in_child_items
        || !element
            .kind
            .is_some_and(|kind| kind.family() == ElementFamily::Addition)
    {
        return true;
    }
    let Some(reference) = element
        .property("AdditionSource")
        .and_then(|property| source(&property.property.value))
        .filter(|reference| Some(reference.kind) == element.kind)
    else {
        return false;
    };
    // A hidden source can still supply a visible addition. A table must declare columns.
    items
        .get(reference.item)
        .is_some_and(|owner| match owner.kind {
            Some(ElementKind::Table) => owner.declares_columns(),
            Some(ElementKind::PdfDocumentField) => {
                reference.kind == ElementKind::ViewStatusAddition
            }
            _ => false,
        })
}
