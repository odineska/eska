//! DAP identifiers have a lifetime independent of dbgs target and stack identifiers.
use std::collections::{BTreeMap, BTreeSet};

use crate::dbgs::{Frame, Selector, Target};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Position {
    frame: u64,
    pub thread: u64,
    pub level: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Variable {
    pub position: Position,
    pub path: Vec<Selector>,
    pub indexed: bool,
}

pub struct Thread {
    pub target: Target,
    pub stack: Option<Vec<(u64, Frame)>>,
    pub snapshot_count: usize,
    pub free_snapshots: Vec<String>,
    pub held_snapshots: Vec<String>,
}

#[derive(Default)]
pub struct State {
    next: u64,
    pub threads: BTreeMap<u64, Thread>,
    frames: BTreeMap<u64, Position>,
    variables: BTreeMap<u64, Variable>,
    variable_ids: BTreeMap<Variable, u64>,
}

impl State {
    pub fn thread_id(&self, id: &str) -> Option<u64> {
        self.threads
            .iter()
            .find_map(|(&key, thread)| (thread.target.id == id).then_some(key))
    }

    pub fn add_thread(&mut self, target: Target) -> u64 {
        let id = self.allocate();
        self.threads.insert(
            id,
            Thread {
                target,
                stack: None,
                snapshot_count: 0,
                free_snapshots: Vec::new(),
                held_snapshots: Vec::new(),
            },
        );
        id
    }

    pub fn stop(&mut self, thread: u64, frames: Vec<Frame>) {
        self.resume(thread);
        let stack = frames
            .into_iter()
            .take(1024)
            .enumerate()
            .map(|(level, frame)| {
                let id = self.allocate();
                self.frames.insert(
                    id,
                    Position {
                        frame: id,
                        thread,
                        level,
                    },
                );
                (id, frame)
            })
            .collect();
        if let Some(thread) = self.threads.get_mut(&thread) {
            thread.stack = Some(stack);
        }
    }

    pub fn resume(&mut self, id: u64) {
        self.frames.retain(|_, position| position.thread != id);
        self.variables
            .retain(|_, variable| variable.position.thread != id);
        self.variable_ids
            .retain(|variable, _| variable.position.thread != id);
        if let Some(thread) = self.threads.get_mut(&id) {
            thread.stack = None;
            thread.free_snapshots.append(&mut thread.held_snapshots);
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.resume(id);
        self.threads.remove(&id);
    }

    pub fn stopped(&self) -> BTreeSet<u64> {
        self.threads
            .iter()
            .filter_map(|(&id, thread)| thread.stack.is_some().then_some(id))
            .collect()
    }

    pub fn frame(&self, id: u64) -> Option<Position> {
        self.frames.get(&id).cloned()
    }

    pub fn is_current(&self, position: &Position) -> bool {
        self.frames.contains_key(&position.frame)
    }

    pub fn variable(&self, id: u64) -> Option<Variable> {
        self.variables.get(&id).cloned()
    }

    pub fn add_variable(&mut self, variable: Variable) -> Option<u64> {
        if !self.is_current(&variable.position) {
            return None;
        }
        if let Some(&id) = self.variable_ids.get(&variable) {
            return Some(id);
        }
        if self.variables.len() >= 8192 || variable.path.len() > 32 {
            return None;
        }
        let id = self.allocate();
        self.variable_ids.insert(variable.clone(), id);
        self.variables.insert(id, variable);
        Some(id)
    }

    const fn allocate(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resume_invalidates_handles_only_for_the_resumed_thread() {
        let mut state = State::default();
        let target = |id: &str| Target {
            id: id.into(),
            kind: String::new(),
            session: String::new(),
            user: String::new(),
        };
        let first = state.add_thread(target("one"));
        let second = state.add_thread(target("two"));
        for id in [first, second] {
            state.stop(
                id,
                vec![Frame {
                    module: None,
                    line: 3,
                    name: "test".into(),
                }],
            );
        }
        let frame = |state: &State, id| state.threads[&id].stack.as_ref().unwrap()[0].0;
        let (old, other) = (frame(&state, first), frame(&state, second));
        let reference = state.add_variable(Variable {
            position: state.frame(old).unwrap(),
            path: vec![],
            indexed: false,
        });
        state.resume(first);
        assert!(state.frame(old).is_none());
        assert!(state.variable(reference.unwrap()).is_none());
        assert!(state.frame(other).is_some());
    }

    #[test]
    fn repeated_expansion_reuses_handles_but_a_new_stop_does_not() {
        let mut state = State::default();
        let thread = state.add_thread(Target {
            id: "one".into(),
            kind: String::new(),
            session: String::new(),
            user: String::new(),
        });
        let frame = Frame {
            module: None,
            line: 3,
            name: "test".into(),
        };
        state.stop(thread, vec![frame.clone()]);
        let frame_id = state.threads[&thread].stack.as_ref().unwrap()[0].0;
        let mut variable = Variable {
            position: state.frame(frame_id).unwrap(),
            path: vec![Selector::Expression("A".into())],
            indexed: false,
        };
        let old = state.add_variable(variable.clone()).unwrap();
        for _ in 0..10_000 {
            assert_eq!(state.add_variable(variable.clone()), Some(old));
        }
        assert_eq!(state.variables.len(), 1);
        state.stop(thread, vec![frame]);
        assert!(state.variable(old).is_none());
        assert_eq!(state.add_variable(variable.clone()), None);
        let frame_id = state.threads[&thread].stack.as_ref().unwrap()[0].0;
        variable.position = state.frame(frame_id).unwrap();
        assert_ne!(state.add_variable(variable), Some(old));
    }
}
