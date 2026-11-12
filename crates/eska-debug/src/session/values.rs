//! Translate platform values without silently losing children or expandable handles.
use super::{Failure, INVALID};
use crate::{
    dbgs::{self, Calculation, Selector},
    state::{State, Variable},
};
use serde_json::{Value, json};

pub(super) const LIMIT: Failure = "debug-value-limit";
const MAX_CHILDREN: usize = 512;

pub(super) fn evaluated(
    state: &mut State,
    variable: &Variable,
    value: &dbgs::Value,
) -> Result<Value, Failure> {
    let reference = reference(state, variable.clone(), value)?;
    Ok(json!({"result":value.text,"type":value.kind,"variablesReference":reference}))
}

pub(super) fn children(
    state: &mut State,
    variable: &Variable,
    results: Vec<Calculation>,
) -> Result<Value, Failure> {
    if results
        .iter()
        .map(|result| result.children.len())
        .sum::<usize>()
        > MAX_CHILDREN
    {
        return Err(LIMIT);
    }
    let children = results.into_iter().flat_map(|result| result.children).map(|value| {
        let mut child = variable.clone();
        let reference = if value.expandable && !value.name.is_empty() {
            let selector = if child.path.is_empty() {
                Selector::Expression(value.name.clone())
            } else if variable.indexed {
                Selector::Index(value.name.parse().map_err(|_| INVALID)?)
            } else {
                Selector::Property(value.name.clone())
            };
            child.path.push(selector);
            reference(state, child, &value)?
        } else { 0 };
        Ok(json!({"name":value.name,"value":value.text,"type":value.kind,"variablesReference":reference}))
    }).collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({"variables":children}))
}

fn reference(
    state: &mut State,
    mut variable: Variable,
    value: &dbgs::Value,
) -> Result<u64, Failure> {
    if !value.expandable {
        return Ok(0);
    }
    variable.indexed = value.indexed;
    state.add_variable(variable).ok_or(LIMIT)
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
        assert_eq!(children(&mut state, &variable, result(513)), Err(LIMIT));
        assert_eq!(
            children(&mut state, &variable, result(512)).unwrap()["variables"]
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
