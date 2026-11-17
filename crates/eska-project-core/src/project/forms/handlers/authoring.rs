//! Resolve authoring against the same composed form and checked Designer module mapping as navigation.

use std::path::Path;

use super::super::{
    EffectiveElement, ElementKey, ElementKind, FormContextError, FormPreview, FormScriptVariant,
    PaletteCategory,
    context::{open_designer, source_identity},
};
use super::{FormHandlerOwner, FormHandlerSource, handlers, identifier};
use crate::{
    application::ContextSnapshot,
    project::{
        designer_source,
        metadata_form_edit::FormTarget,
        metadata_model::{ModuleRole, ObjectId},
    },
};

#[derive(Debug)]
pub struct FormHandlerEditSource {
    pub source: FormHandlerSource,
    pub exists: bool,
    pub bound: bool,
    pub(crate) template: Option<HandlerTemplate>,
    pub(crate) local: bool,
}

#[derive(Debug)]
pub struct HandlerTemplate {
    pub name: String,
    pub parameters: &'static str,
    pub directive: &'static str,
    pub region: String,
    pub alternate_region: String,
    pub english: bool,
}

/// Resolve only the selected event; foreign module paths never come from the editor.
///
/// # Errors
/// Returns stale application/source errors. Unsupported targets are unavailable.
pub fn handler_edit_source(
    context: &ContextSnapshot,
    selected: &Path,
    owner: &ObjectId,
    preview: &FormPreview,
    target: &FormTarget,
    event: &str,
) -> Result<Option<FormHandlerEditSource>, FormContextError> {
    if context.fingerprint != preview.context_fingerprint {
        return Err(FormContextError::MemberMismatch);
    }
    let Some((handler_owner, element, prefix, table)) = selection(preview, target, event) else {
        return Ok(None);
    };
    let matches: Vec<_> = handlers(&preview.form)
        .into_iter()
        .filter(|handler| {
            handler.event == event
                && match (&handler.owner, &handler_owner) {
                    (FormHandlerOwner::Element(a), FormHandlerOwner::Element(b)) => a == b,
                    (FormHandlerOwner::Command(a), FormHandlerOwner::Command(b)) => a == b,
                    _ => false,
                }
        })
        .collect();
    if matches.len() > 1 {
        return Ok(None);
    }
    let bound = matches.first();
    let current = preview.dependencies.iter().find(|input| {
        input.root == selected && input.role == super::super::FormDependencyRole::Form
    });
    let dependency = bound
        .and_then(|handler| {
            preview
                .dependencies
                .iter()
                .find(|input| input.id == handler.origin.source)
        })
        .or(current);
    let Some(dependency) = dependency else {
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
    let module = source
        .sources(owner)
        .map_err(FormContextError::Source)?
        .into_iter()
        .find(|location| location.role == designer_source::SourceRole::Module(ModuleRole::Module));
    let exists = module.is_some();
    let path = module.map_or_else(
        || dependency.path.with_extension("").join("Module.bsl"),
        |module| module.path,
    );
    if !source
        .module_candidates(owner, ModuleRole::Module)
        .map_err(FormContextError::Source)?
        .contains(&path)
    {
        return Ok(None);
    }
    let template = template_for(
        preview,
        target,
        event,
        element.and_then(|element| element.kind),
        prefix,
        table,
        bound.map(|handler| handler.name.as_str()),
    );
    let Some(name) = bound
        .map(|handler| handler.name.clone())
        .or_else(|| template.as_ref().map(|template| template.name.clone()))
        .filter(|name| identifier(name))
    else {
        return Ok(None);
    };
    Ok(Some(FormHandlerEditSource {
        source: FormHandlerSource {
            id: source_identity(context, input.project().source(), &path),
            root: input.project().source().to_path_buf(),
            path,
            name,
        },
        exists,
        bound: bound.is_some(),
        template,
        local: dependency.root == selected,
    }))
}

fn template_for(
    preview: &FormPreview,
    target: &FormTarget,
    event: &str,
    kind: Option<ElementKind>,
    prefix: &str,
    table: Option<&str>,
    bound: Option<&str>,
) -> Option<HandlerTemplate> {
    if kind.is_none() && !matches!(target, FormTarget::Command { .. }) {
        return None;
    }
    let english = preview.presentation.script_variant == Some(FormScriptVariant::English);
    let mut signature = super::signatures::signature(kind, event)?;
    if matches!(target, FormTarget::Root {}) && event == "ActivationProcessing" {
        use crate::project::metadata_model::MetadataKind;
        match preview.form_owner.as_ref().map(|owner| owner.kind) {
            Some(MetadataKind::BusinessProcess) => {
                signature.russian = "ОбработкаАктивации";
                signature.russian_parameters = "СтандартнаяОбработка";
                signature.english_parameters = "StandardProcessing";
            }
            Some(MetadataKind::Task) => {
                signature.russian = "ОбработкаАктивации";
                signature.russian_parameters = "";
                signature.english_parameters = "";
            }
            _ => {}
        }
    }
    let name = bound.map_or_else(
        || {
            format!(
                "{prefix}{}",
                if english {
                    signature.english
                } else {
                    signature.russian
                }
            )
        },
        str::to_owned,
    );
    let (russian_region, english_region) = region_names(target, table);
    let (region, alternate_region) = if english {
        (english_region, russian_region)
    } else {
        (russian_region, english_region)
    };
    Some(HandlerTemplate {
        name,
        parameters: if english {
            signature.english_parameters
        } else {
            signature.russian_parameters
        },
        directive: match (
            english,
            event == "OnGetDataAtServer",
            event.ends_with("AtServer"),
        ) {
            (true, true, _) => "&AtServerNoContext",
            (false, true, _) => "&НаСервереБезКонтекста",
            (true, _, true) => "&AtServer",
            (false, _, true) => "&НаСервере",
            (true, _, _) => "&AtClient",
            (false, _, _) => "&НаКлиенте",
        },
        region,
        alternate_region,
        english,
    })
}

fn region_names(target: &FormTarget, table: Option<&str>) -> (String, String) {
    let names = match target {
        FormTarget::Command { .. } => ("ОбработчикиКомандФормы", "FormCommandsEventHandlers"),
        FormTarget::Root { .. } => ("ОбработчикиСобытийФормы", "FormEventHandlers"),
        _ => {
            if let Some(table) = table {
                return (
                    format!("ОбработчикиСобытийЭлементовТаблицыФормы{table}"),
                    format!("FormTableItemsEventHandlers{table}"),
                );
            }
            (
                "ОбработчикиСобытийЭлементовШапкиФормы",
                "FormHeaderItemsEventHandlers",
            )
        }
    };
    (names.0.to_owned(), names.1.to_owned())
}

type Selection<'a> = (
    FormHandlerOwner,
    Option<&'a EffectiveElement>,
    &'a str,
    Option<&'a str>,
);
fn selection<'a>(
    preview: &'a FormPreview,
    target: &'a FormTarget,
    event: &str,
) -> Option<Selection<'a>> {
    if let FormTarget::Command { name } = target {
        return (event == "Action"
            && preview
                .form
                .commands
                .iter()
                .any(|command| command.name.as_ref() == Some(name)))
        .then_some((
            FormHandlerOwner::Command(name.clone()),
            None,
            name.as_str(),
            None,
        ));
    }
    let key = match target {
        FormTarget::Root {} => ElementKey::Root,
        FormTarget::Element { id } => ElementKey::Xml(*id),
        _ => return None,
    };
    let mut found = Vec::new();
    find(&preview.form.root, key, None, &mut found);
    let [(element, table)] = found.as_slice() else {
        return None;
    };
    let available = preview
        .property_palette(&element.identity)
        .and_then(|palette| palette.rows)
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.available
                    && row.definition.is_some_and(|definition| {
                        definition.id == event && definition.category == PaletteCategory::Events
                    })
            })
        });
    // Existing bindings still navigate even when the platform palette is incomplete for this subtype.
    let declared = handlers(&preview.form).iter().any(|handler| handler.event == event && matches!(&handler.owner, FormHandlerOwner::Element(identity) if identity == &element.identity));
    if !available && !declared {
        return None;
    }
    Some((
        FormHandlerOwner::Element(element.identity.clone()),
        Some(*element),
        if key == ElementKey::Root {
            ""
        } else {
            element.name.as_deref()?
        },
        *table,
    ))
}

fn find<'a>(
    element: &'a EffectiveElement,
    key: ElementKey,
    table: Option<&'a str>,
    found: &mut Vec<(&'a EffectiveElement, Option<&'a str>)>,
) {
    let table = if element.kind == Some(ElementKind::Table) {
        element.name.as_deref()
    } else {
        table
    };
    if element.identity.key == key {
        found.push((element, table));
    }
    for child in element.children.iter().chain(&element.auxiliary) {
        find(child, key, table, found);
    }
}
