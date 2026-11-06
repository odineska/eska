//! Read-only handler navigation follows effective XML provenance and checked form modules.

use std::{ops::Range, path::PathBuf};

use super::{
    ComposedForm, EffectiveElement, ElementIdentity, FORM_NAMESPACE, FormContextError, FormPreview,
    PropertySource, SourcedProperty,
    context::{open_designer, source_identity},
};
use crate::{
    application::ContextSnapshot,
    project::{
        designer_source,
        metadata_model::{MetadataProperty, MetadataValue, ModuleRole, ObjectId},
        query::{self, bsl},
    },
};

#[derive(Clone, Debug)]
pub enum FormHandlerOwner {
    Element(ElementIdentity),
    Command(String),
}

#[derive(Clone, Debug)]
pub struct FormHandler {
    pub owner: FormHandlerOwner,
    pub event: String,
    pub name: String,
    pub origin: PropertySource,
}

#[derive(Debug)]
pub struct FormHandlerSource {
    pub id: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub name: String,
}

/// Retain repeats and the source of each effective Events/Action property.
#[must_use]
pub fn handlers(form: &ComposedForm) -> Vec<FormHandler> {
    let mut result = Vec::new();
    element_handlers(&form.root, &mut result);
    for command in &form.commands {
        let Some(name) = &command.name else { continue };
        for property in &command.properties {
            if property.property.key.namespace.as_deref() == Some(FORM_NAMESPACE)
                && property.property.key.name == "Action"
                && let MetadataValue::Text(handler) = &property.property.value
                && identifier(handler.trim())
            {
                result.push(FormHandler {
                    owner: FormHandlerOwner::Command(name.clone()),
                    event: "Action".into(),
                    name: handler.trim().into(),
                    origin: property.origin.clone(),
                });
            }
        }
    }
    result
}

fn element_handlers(node: &EffectiveElement, output: &mut Vec<FormHandler>) {
    for SourcedProperty { property, origin } in &node.properties {
        if property.key.namespace.as_deref() != Some(FORM_NAMESPACE)
            || property.key.name != "Events"
        {
            continue;
        }
        let MetadataValue::Record(events) = &property.value else {
            continue;
        };
        for event in events {
            let (Some(name), MetadataValue::Text(handler)) = (event_name(event), &event.value)
            else {
                continue;
            };
            if !identifier(handler.trim()) {
                continue;
            }
            output.push(FormHandler {
                owner: FormHandlerOwner::Element(node.identity.clone()),
                event: name.into(),
                name: handler.trim().into(),
                origin: origin.clone(),
            });
        }
    }
    for child in node.children.iter().chain(&node.auxiliary) {
        element_handlers(child, output);
    }
}

pub(super) fn event_name(event: &MetadataProperty) -> Option<&str> {
    if event.key.namespace.as_deref() != Some(FORM_NAMESPACE) || event.key.name != "Event" {
        return None;
    }
    event
        .qualifiers
        .iter()
        .find(|(key, _)| key.namespace.is_none() && key.name == "name")
        .map(|(_, name)| name.as_str())
}

/// Resolve an existing module through Designer ownership, without reading or creating BSL.
///
/// # Errors
/// Returns context changes, descriptor, containment or filesystem failures.
pub fn handler_source(
    context: &ContextSnapshot,
    owner: &ObjectId,
    preview: &FormPreview,
    index: usize,
) -> Result<Option<FormHandlerSource>, FormContextError> {
    if context.fingerprint != preview.context_fingerprint {
        return Err(FormContextError::MemberMismatch);
    }
    let catalog = handlers(&preview.form);
    let Some(handler) = catalog.get(index) else {
        return Ok(None);
    };
    let Some(dependency) = preview
        .dependencies
        .iter()
        .find(|input| input.id == handler.origin.source)
    else {
        return Ok(None);
    };
    let Some(input) = context
        .context
        .sources()
        .iter()
        .find(|input| input.project().source() == dependency.root)
    else {
        return Ok(None);
    };
    let source = open_designer(input)?;
    let Some(module) = source
        .sources(owner)
        .map_err(FormContextError::Source)?
        .into_iter()
        .find(|location| location.role == designer_source::SourceRole::Module(ModuleRole::Module))
    else {
        return Ok(None);
    };
    Ok(Some(FormHandlerSource {
        id: source_identity(context, input.project().source(), &module.path),
        root: input.project().source().to_path_buf(),
        path: module.path,
        name: handler.name.clone(),
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerLookupError {
    TooLarge,
    InvalidModule,
    Ambiguous,
}

/// Find one complete declaration, ignoring literals, comments and preprocessor/annotation text.
///
/// # Errors
/// Rejects oversized/incomplete text and duplicate declarations instead of choosing a branch.
pub fn handler_range(input: &str, name: &str) -> Result<Option<Range<usize>>, HandlerLookupError> {
    if input.len() > query::MAX_SNAPSHOT_BYTES {
        return Err(HandlerLookupError::TooLarge);
    }
    if !identifier(name) {
        return Ok(None);
    }
    let tokens = bsl::tokens(input).map_err(|error| match error {
        query::QueryError::TooLarge => HandlerLookupError::TooLarge,
        _ => HandlerLookupError::InvalidModule,
    })?;
    let word = |index: usize| {
        tokens
            .get(index)
            .filter(|token| token.kind == bsl::Kind::Word)
            .map(|token| input[token.range.clone()].to_lowercase())
    };
    let mut active = None;
    let mut found = None;
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != bsl::Kind::Word
            || index > 0 && tokens[index - 1].kind == bsl::Kind::Punctuation(b'.')
        {
            continue;
        }
        match word(index).as_deref() {
            Some("procedure" | "процедура" | "function" | "функция") => {
                if active.is_some()
                    || word(index + 1).is_none()
                    || tokens
                        .get(index + 2)
                        .is_none_or(|token| token.kind != bsl::Kind::Punctuation(b'('))
                {
                    return Err(HandlerLookupError::InvalidModule);
                }
                let procedure = matches!(word(index).as_deref(), Some("procedure" | "процедура"));
                let mut depth = 0usize;
                let closed = tokens[index + 2..]
                    .iter()
                    .take_while(|token| {
                        token.kind != bsl::Kind::Word
                            || !matches!(
                                input[token.range.clone()].to_lowercase().as_str(),
                                "procedure"
                                    | "процедура"
                                    | "function"
                                    | "функция"
                                    | "endprocedure"
                                    | "конецпроцедуры"
                                    | "endfunction"
                                    | "конецфункции"
                            )
                    })
                    .any(|token| {
                        if token.kind == bsl::Kind::Punctuation(b'(') {
                            depth += 1;
                        }
                        if token.kind == bsl::Kind::Punctuation(b')') {
                            depth -= 1;
                        }
                        depth == 0
                    });
                if !closed {
                    return Err(HandlerLookupError::InvalidModule);
                }
                active = Some((procedure, tokens[index + 1].range.clone()));
            }
            Some("endprocedure" | "конецпроцедуры" | "endfunction" | "конецфункции") =>
            {
                let Some((procedure, range)) = active.take() else {
                    return Err(HandlerLookupError::InvalidModule);
                };
                if procedure
                    != matches!(
                        word(index).as_deref(),
                        Some("endprocedure" | "конецпроцедуры")
                    )
                {
                    return Err(HandlerLookupError::InvalidModule);
                }
                if input[range.clone()].to_lowercase() == name.to_lowercase() {
                    if found.is_some() {
                        return Err(HandlerLookupError::Ambiguous);
                    }
                    found = Some(range);
                }
            }
            _ => (),
        }
    }
    if active.is_some() {
        return Err(HandlerLookupError::InvalidModule);
    }
    Ok(found)
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}

#[cfg(test)]
mod tests;
