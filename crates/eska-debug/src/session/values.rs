//! Translate platform values without silently losing children or expandable handles.
use super::{Failure, INVALID};
use crate::{
    dbgs::{self, Calculation, Page, Selector},
    state::{State, Variable},
};
use serde_json::{Value, json};

pub(super) const LIMIT: Failure = "debug-value-limit";
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

pub(super) fn page(variable: &Variable, arguments: &Value) -> Result<Option<Page>, Failure> {
    match arguments["filter"].as_str() {
        Some("named") if variable.indexed => return Ok(None),
        Some("indexed") if !variable.indexed => return Ok(None),
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
    if variable.indexed && start.checked_add(count - 1).is_none() {
        return Err(INVALID);
    }
    Ok(Some(Page { start, count }))
}

pub(super) fn children(
    state: &mut State,
    variable: &Variable,
    page: Page,
    results: Vec<Calculation>,
) -> Result<Value, Failure> {
    let raw = results.into_iter().flat_map(|result| result.children);
    let raw = if variable.indexed {
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
        children.push(display_child(state, variable, page.start, value)?);
    }
    Ok(json!({"variables":children}))
}

fn display_child(
    state: &mut State,
    parent: &Variable,
    start: u32,
    value: &dbgs::Value,
) -> Result<Value, Failure> {
    let index = if parent.indexed {
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
    let name = index.map_or_else(|| value.name.clone(), |index| index.to_string());
    let mut body =
        json!({"name":name,"value":value.text,"type":value.kind,"variablesReference":reference});
    add_count(&mut body, value);
    Ok(body)
}

fn reference(
    state: &mut State,
    mut variable: Variable,
    value: &dbgs::Value,
) -> Result<u64, Failure> {
    if !value.can_expand() {
        return Ok(0);
    }
    variable.indexed = !value.expandable && value.indexed;
    variable.indexed_count = value.collection_size.filter(|_| variable.indexed);
    state.add_variable(variable).ok_or(LIMIT)
}

fn add_count(body: &mut Value, value: &dbgs::Value) {
    if let Some(size) = value
        .collection_size
        .filter(|_| value.indexed && !value.expandable)
    {
        body["indexedVariables"] = json!(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbgs::{Frame, Target};

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
            indexed: false,
            indexed_count: None,
        };
        (state, variable)
    }

    fn value() -> dbgs::Value {
        dbgs::Value {
            name: "Object".into(),
            text: "value".into(),
            kind: "object".into(),
            expandable: true,
            indexed: false,
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
            children(&mut state, &variable, page, result(513)),
            Err(LIMIT)
        );
        assert_eq!(
            children(&mut state, &variable, page, result(512)).unwrap()["variables"]
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
        scalar.expandable = false;
        assert_eq!(
            evaluated(&mut state, &variable, &scalar).unwrap()["variablesReference"],
            0
        );
    }
}
