//! Translate platform values without silently losing children or expandable handles.
use std::fmt::Write as _;

use super::{Failure, INVALID};
use crate::{
    dbgs::{self, Calculation, Page, Selector},
    state::{State, Variable},
};
use serde_json::{Value, json};

pub(super) const LIMIT: Failure = "debug-value-limit";
const READ_ONLY: Failure = "debug-value-read-only";
const PAGE_LIMIT: Failure = "debug-variable-page-limit";
const MAX_CHILDREN: u32 = 512;

// Save the result once in 1C session storage. DAP handles contain only the
// retrieval expression and paths within that value, never the user's expression.
pub(super) fn store_expression(expression: &str, address: Option<&str>) -> String {
    let destination = address.map_or_else(
        || "Новый УникальныйИдентификатор".to_owned(),
        |address| format!("\"{address}\""),
    );
    let store = format!("ПоместитьВоВременноеХранилище(({expression}), {destination})");
    if address.is_some() {
        dbgs::snapshot_expression(&store)
    } else {
        store
    }
}

pub(super) fn snapshot_address(presentation: &str) -> Option<&str> {
    let address = presentation.strip_prefix('"')?.strip_suffix('"')?;
    (address.starts_with("e1cib/tempstorage/")
        && address.len() <= 512
        && address
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/-_.?=&%+".contains(&byte)))
    .then_some(address)
}

pub(super) fn evaluated(
    state: &mut State,
    variable: &Variable,
    value: &dbgs::Value,
) -> Result<Value, Failure> {
    let reference = reference(state, variable.clone(), value)?;
    let mut body = json!({"result":value.text,"type":value.kind,"variablesReference":reference});
    add_count(&mut body, value);
    Ok(body)
}

pub(super) fn assigned(
    state: &mut State,
    variable: Variable,
    value: &dbgs::Value,
) -> Result<Value, Failure> {
    state.invalidate_variable(&variable);
    let reference = reference(state, variable, value)?;
    let mut body = json!({"value":value.text,"type":value.kind,"variablesReference":reference});
    add_count(&mut body, value);
    Ok(body)
}

pub(super) fn assignment_target(mut parent: Variable, name: &str) -> Result<Variable, Failure> {
    if read_only(&parent) {
        return Err(READ_ONLY);
    }
    let selector = match parent.view {
        dbgs::View::Collection => {
            let index = name.parse::<u32>().map_err(|_| INVALID)?;
            if parent.indexed_count.is_some_and(|size| index >= size) {
                return Err(INVALID);
            }
            Selector::Index(index)
        }
        dbgs::View::Context => {
            if !bsl_identifier(name) {
                return Err(INVALID);
            }
            if parent.path.is_empty() {
                Selector::Expression(name.to_owned())
            } else {
                Selector::Property(name.to_owned())
            }
        }
        dbgs::View::Enumeration => return Err(READ_ONLY),
    };
    if parent.path.len() >= 32 {
        return Err(LIMIT);
    }
    parent.path.push(selector);
    parent.view = dbgs::View::Context;
    parent.indexed_count = None;
    Ok(parent)
}

pub(super) fn assignment_path(path: &[Selector]) -> Result<Vec<Selector>, Failure> {
    if !path
        .iter()
        .any(|selector| matches!(selector, Selector::Index(_)))
    {
        return Ok(path.to_vec());
    }
    // dbgs can read indexed selectors but expects a BSL l-value for indexed writes.
    let Some((Selector::Expression(root), tail)) = path.split_first() else {
        return Err(READ_ONLY);
    };
    if !bsl_identifier(root) {
        return Err(READ_ONLY);
    }
    let mut expression = root.clone();
    for selector in tail {
        match selector {
            Selector::Index(index) => {
                let _ = write!(expression, "[{index}]");
            }
            Selector::Property(name) if bsl_identifier(name) => {
                expression.push('.');
                expression.push_str(name);
            }
            _ => return Err(READ_ONLY),
        }
    }
    Ok(vec![Selector::Expression(expression)])
}

fn bsl_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn read_only(parent: &Variable) -> bool {
    matches!(parent.path.first(), Some(Selector::Snapshot(_)))
}

pub(super) fn page(variable: &Variable, arguments: &Value) -> Result<Option<Page>, Failure> {
    match arguments["filter"].as_str() {
        Some("named") if variable.view.paged() => return Ok(None),
        Some("indexed") if !variable.view.paged() => return Ok(None),
        Some("named" | "indexed") => {}
        None if arguments["filter"].is_null() => {}
        _ => return Err(INVALID),
    }
    let number = |field: &str| {
        if arguments[field].is_null() {
            return Ok(0);
        }
        arguments[field]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(INVALID)
    };
    let start = number("start")?;
    let requested = number("count")?;
    let remaining = variable
        .indexed_count
        .map(|size| size.saturating_sub(start));
    if remaining == Some(0) {
        return Ok(None);
    }
    let count = if requested == 0 {
        remaining.unwrap_or(MAX_CHILDREN + 1)
    } else {
        requested.min(remaining.unwrap_or(u32::MAX))
    };
    if count > MAX_CHILDREN && (remaining.is_some() || requested != 0) {
        return Err(PAGE_LIMIT);
    }
    if variable.view.paged() && start.checked_add(count - 1).is_none() {
        return Err(INVALID);
    }
    Ok(Some(Page { start, count }))
}

pub(super) fn children(
    state: &mut State,
    variable: &Variable,
    page: Page,
    results: Vec<Calculation>,
    unreadable: &str,
) -> Result<Value, Failure> {
    let raw = results.into_iter().flat_map(|result| result.children);
    let raw = if variable.view.paged() {
        raw.take(MAX_CHILDREN as usize + 1).collect::<Vec<_>>()
    } else {
        raw.skip(page.start as usize)
            .take(page.count as usize)
            .collect()
    };
    if raw.len() > MAX_CHILDREN as usize || raw.len() > page.count as usize {
        return Err(LIMIT);
    }
    let mut children = Vec::with_capacity(raw.len());
    for value in &raw {
        children.push(display_child(
            state, variable, page.start, value, unreadable,
        )?);
    }
    Ok(json!({"variables":children}))
}

fn display_child(
    state: &mut State,
    parent: &Variable,
    start: u32,
    value: &dbgs::Value,
    unreadable: &str,
) -> Result<Value, Failure> {
    let index = if parent.view.paged() {
        Some(
            value
                .name
                .parse::<u32>()
                .ok()
                .and_then(|index| start.checked_add(index))
                .ok_or(INVALID)?,
        )
    } else {
        None
    };
    let reference = if value.can_expand() && !value.name.is_empty() {
        let mut child = parent.clone();
        let selector = if child.path.is_empty() {
            Selector::Expression(value.name.clone())
        } else if let Some(index) = index {
            Selector::Index(index)
        } else {
            Selector::Property(value.name.clone())
        };
        child.path.push(selector);
        reference(state, child, value)?
    } else {
        0
    };
    let name = match (index, value.entry_key.as_deref()) {
        (Some(index), Some(key)) => format!("{index}: {key}"),
        (Some(index), None) => index.to_string(),
        (None, _) => value.name.clone(),
    };
    let text = if value.error && value.text.is_empty() {
        unreadable
    } else {
        &value.text
    };
    let mut body =
        json!({"name":name,"value":text,"type":value.kind,"variablesReference":reference});
    if value.error || read_only(parent) || parent.view == dbgs::View::Enumeration {
        body["presentationHint"] = json!({"attributes":["readOnly"]});
    }
    add_count(&mut body, value);
    Ok(body)
}

fn reference(
    state: &mut State,
    mut variable: Variable,
    value: &dbgs::Value,
) -> Result<u64, Failure> {
    let Some(view) = value.view.filter(|_| !value.error) else {
        return Ok(0);
    };
    variable.view = view;
    variable.indexed_count = value.collection_size.filter(|_| variable.view.paged());
    state.add_variable(variable).ok_or(LIMIT)
}

fn add_count(body: &mut Value, value: &dbgs::Value) {
    if let Some(size) = value
        .collection_size
        .filter(|_| !value.error && value.view.is_some_and(dbgs::View::paged))
    {
        body["indexedVariables"] = json!(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbgs::{Frame, Target, View};

    #[test]
    fn indexed_assignment_uses_only_validated_path_components() {
        let path = [
            Selector::Expression("Структура".into()),
            Selector::Property("Ряд".into()),
            Selector::Index(123),
            Selector::Property("Значение".into()),
        ];
        assert_eq!(
            assignment_path(&path).unwrap(),
            [Selector::Expression("Структура.Ряд[123].Значение".into())]
        );
        let invalid = [Selector::Expression("Функция()".into()), Selector::Index(0)];
        assert_eq!(assignment_path(&invalid), Err(READ_ONLY));
    }

    fn stopped() -> (State, Variable) {
        let mut state = State::default();
        let id = state.add_thread(Target {
            id: "one".into(),
            kind: String::new(),
            session: String::new(),
            user: String::new(),
        });
        state.stop(
            id,
            vec![Frame {
                module: None,
                line: 1,
                name: "one".into(),
            }],
        );
        let frame = state.threads[&id].stack.as_ref().unwrap()[0].0;
        let variable = Variable {
            position: state.frame(frame).unwrap(),
            path: vec![],
            view: View::Context,
            indexed_count: None,
        };
        (state, variable)
    }

    fn value() -> dbgs::Value {
        dbgs::Value {
            name: "Object".into(),
            text: "value".into(),
            kind: "object".into(),
            view: Some(View::Context),
            entry_key: None,
            collection_size: None,
            error: false,
        }
    }

    #[test]
    fn large_collection_is_rejected_instead_of_returning_a_successful_partial_list() {
        let (mut state, variable) = stopped();
        let result = |count| {
            vec![Calculation {
                id: "one".into(),
                value: value(),
                children: vec![value(); count],
            }]
        };
        let page = Page {
            start: 0,
            count: 513,
        };
        assert_eq!(
            children(&mut state, &variable, page, result(513), "Unreadable"),
            Err(LIMIT)
        );
        assert_eq!(
            children(&mut state, &variable, page, result(512), "Unreadable").unwrap()["variables"]
                .as_array()
                .unwrap()
                .len(),
            512
        );
    }

    #[test]
    fn expansion_limits_never_turn_an_object_into_an_apparent_leaf() {
        let (mut state, mut variable) = stopped();
        variable.path = vec![Selector::Property("Nested".into()); 33];
        assert_eq!(evaluated(&mut state, &variable, &value()), Err(LIMIT));
        let mut scalar = value();
        scalar.view = None;
        assert_eq!(
            evaluated(&mut state, &variable, &scalar).unwrap()["variablesReference"],
            0
        );
    }

    #[test]
    fn unreadable_properties_use_the_fallback_and_do_not_offer_a_handle() {
        let (mut state, variable) = stopped();
        let mut property = value();
        property.error = true;
        property.text.clear();
        let child = display_child(&mut state, &variable, 0, &property, "Недоступно").unwrap();
        assert_eq!(child["value"], "Недоступно");
        assert_eq!(child["variablesReference"], 0);
        assert_eq!(child["presentationHint"]["attributes"], json!(["readOnly"]));
    }
}
