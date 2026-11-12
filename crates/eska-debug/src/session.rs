//! One serialized debugger session. HTTP requests and poll results have a single owner.
mod client;
mod poll;
mod values;

use std::{
    collections::BTreeMap,
    io::{self, BufReader, BufWriter, Write},
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use eska_build_runtime::runtime::DebugLaunch;
use eska_presentation::localization::Localizer;
use serde_json::{Value, json};

use crate::{
    Mode,
    dap::{self, Output, Request},
    dbgs::{
        self, Breakpoint, Calculation, Event, Module, Selector, Server, StackAction, Step, Target,
    },
    source::Sources,
    state::{Position, State, Variable},
};

use client::Client;
use poll::Poll;

const MAX_THREADS: usize = 256;
const MAX_PENDING: usize = 16;
const MAX_SNAPSHOTS_PER_TARGET: usize = 256;
const MAX_DEBUG_EXPRESSION: usize = 16384;
const ERROR_FILTER: &str = "all";

pub struct Config {
    pub endpoint: url::Url,
    pub alias: String,
    pub sources: Sources,
    pub mode: Mode,
    pub launch: Option<DebugLaunch>,
    pub production: bool,
}

type Failure = &'static str;
const INVALID: Failure = "debug-request-unavailable";
const ENDPOINT: Failure = "debug-endpoint-unavailable";

fn optional_expression<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, Failure> {
    if value[key].is_null() {
        return Ok(None);
    }
    let expression = value[key]
        .as_str()
        .filter(|text| text.len() <= MAX_DEBUG_EXPRESSION)
        .ok_or(INVALID)?;
    Ok((!expression.is_empty()).then_some(expression))
}

fn optional_array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Failure> {
    if value[key].is_null() {
        return Ok(&[]);
    }
    value[key].as_array().map(Vec::as_slice).ok_or(INVALID)
}

enum CalculationPhase {
    Store,
    ReadSnapshot(String),
    ReplaceSnapshot(String),
    Children,
}

struct Pending {
    request: Request,
    variable: Variable,
    phase: CalculationPhase,
    deadline: Instant,
}

enum Input {
    Packet(io::Result<Option<Request>>),
    Events(io::Result<Vec<Event>>),
}

struct Session<'a, W> {
    config: Config,
    output: Output<W>,
    language: &'a Localizer,
    server: Option<Arc<Server>>,
    input: mpsc::SyncSender<Input>,
    poll: Option<Poll>,
    state: State,
    breakpoints: BTreeMap<Module, Vec<Breakpoint>>,
    pending: BTreeMap<String, Pending>,
    client: Option<Client>,
    initialized: bool,
    configured: bool,
    ended: bool,
}

pub fn run(config: Config, language: &Localizer) -> io::Result<()> {
    let (send, receive) = mpsc::sync_channel(32);
    let reader = send.clone();
    thread::Builder::new().spawn(move || {
        let mut input = BufReader::new(io::stdin());
        loop {
            let packet = dap::read(&mut input);
            let done = !matches!(packet, Ok(Some(_)));
            if reader.send(Input::Packet(packet)).is_err() || done {
                break;
            }
        }
    })?;
    let mut session = Session {
        config,
        output: Output::new(BufWriter::new(io::stdout().lock())),
        language,
        server: None,
        input: send,
        poll: None,
        state: State::default(),
        breakpoints: BTreeMap::new(),
        pending: BTreeMap::new(),
        client: None,
        initialized: false,
        configured: false,
        ended: false,
    };
    let operation = eska_tool_runtime::Operation::new()
        .map_err(|_| io::Error::other("cannot install debugger cancellation"))?;
    let result = drive(&mut session, &receive, &operation);
    let cancelled = session.cancel_pending();
    session.cleanup();
    result.and(cancelled)
}

fn drive(
    session: &mut Session<'_, impl Write>,
    receive: &mpsc::Receiver<Input>,
    operation: &eska_tool_runtime::Operation,
) -> io::Result<()> {
    while !session.ended && !operation.is_cancelled() {
        if let Some(success) = session.client.as_mut().and_then(Client::completed) {
            session.terminate((!success).then_some("runtime-client-failed"))?;
            break;
        }
        if session.poll.as_ref().is_some_and(Poll::failed) {
            session.terminate(Some(ENDPOINT))?;
            break;
        }
        match receive.recv_timeout(Duration::from_millis(100)) {
            Ok(Input::Packet(Ok(Some(request)))) => session.request(request)?,
            Ok(Input::Packet(Ok(None))) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Input::Packet(Err(error))) => return Err(error),
            Ok(Input::Events(Ok(events))) => {
                session.events(events)?;
            }
            Ok(Input::Events(Err(_))) => {
                session.terminate(Some(ENDPOINT))?;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        session.expire()?;
    }
    Ok(())
}

impl<W: Write> Session<'_, W> {
    fn terminate(&mut self, failure: Option<Failure>) -> io::Result<()> {
        self.cancel_pending()?;
        if let Some(key) = failure {
            self.output.event(
                "output",
                &json!({"category":"stderr","output":format!("{}\n", self.language.text(key))}),
            )?;
        }
        self.output.event("terminated", &json!({}))
    }

    fn request(&mut self, request: Request) -> io::Result<()> {
        if matches!(request.command.as_str(), "evaluate" | "variables") {
            return self.calculation_request(request);
        }
        let result = match request.command.as_str() {
            "initialize" if !self.initialized => {
                self.initialized = true;
                Ok(
                    json!({"supportsConfigurationDoneRequest":true,"supportsCancelRequest":true,
                    "supportsEvaluateForHovers":true, "supportsSingleThreadExecutionRequests":false,
                    "supportsConditionalBreakpoints":true,"supportsHitConditionalBreakpoints":true,
                    "supportsLogPoints":true,"supportsExceptionFilterOptions":true,
                    "exceptionBreakpointFilters":[{"filter":ERROR_FILTER,"label":self.language.text("debug-runtime-errors"),
                    "supportsCondition":true,"conditionDescription":self.language.text("debug-runtime-error-condition")}]}),
                )
            }
            "launch" | "attach" => self.open(&request.command),
            "configurationDone" if self.server.is_some() && !self.configured => {
                if let Ok(client) = self.config.launch.take().map(Client::launch).transpose() {
                    self.client = client;
                    self.configured = true;
                    Ok(json!({}))
                } else {
                    self.ended = true;
                    Err("runtime-client-failed")
                }
            }
            "setBreakpoints" => self.set_breakpoints(&request.arguments),
            "setExceptionBreakpoints" => self.set_exception_breakpoints(&request.arguments),
            "threads" if self.server.is_some() => Ok(self.threads()),
            "stackTrace" => self.stack(&request.arguments),
            "scopes" => self.scopes(&request.arguments),
            "continue" | "next" | "stepIn" | "stepOut" => self.step(&request),
            "eska/listDebugTargets" => self.targets(),
            "eska/attachDebugTarget" => self.select_target(&request.arguments),
            "cancel" => {
                self.cancel(&request.arguments)?;
                Ok(json!({}))
            }
            "disconnect" => {
                self.cancel_pending()?;
                self.cleanup();
                self.ended = true;
                Ok(json!({}))
            }
            _ => Err(INVALID),
        };
        let started = matches!(request.command.as_str(), "attach" | "launch") && result.is_ok();
        self.output
            .response(&request, result.map_err(|key| self.language.text(key)))?;
        if started {
            self.output.event("initialized", &json!({}))?;
            if self.config.production {
                self.output.event("output", &json!({"category":"stderr","output":format!("{}\n",self.language.text("debug-prod-warning"))}))?;
            }
        }
        Ok(())
    }

    fn open(&mut self, command: &str) -> Result<Value, Failure> {
        let expected = if self.config.mode == Mode::Launch {
            "launch"
        } else {
            "attach"
        };
        if !self.initialized || self.server.is_some() || command != expected {
            return Err(INVALID);
        }
        let server =
            Server::new(&self.config.endpoint, self.config.alias.clone()).map_err(|_| ENDPOINT)?;
        server.attach().map_err(|_| ENDPOINT)?;
        if server.auto_attach().is_err() {
            let _ = server.detach();
            return Err(ENDPOINT);
        }
        let server = Arc::new(server);
        self.poll = Some(
            Poll::start(Arc::clone(&server), self.input.clone()).map_err(|_| {
                let _ = server.detach();
                ENDPOINT
            })?,
        );
        self.server = Some(server);
        Ok(json!({}))
    }

    fn server(&self) -> Result<&Server, Failure> {
        self.server.as_deref().ok_or(INVALID)
    }

    fn set_breakpoints(&mut self, arguments: &Value) -> Result<Value, Failure> {
        let path = arguments["source"]["path"].as_str().ok_or(INVALID)?;
        let entries = arguments["breakpoints"].as_array().ok_or(INVALID)?;
        if entries.len() > 1024 {
            return Err(INVALID);
        }
        let module = self.config.sources.module(Path::new(path)).cloned();
        let lines: Vec<_> = entries
            .iter()
            .map(|entry| {
                entry["line"]
                    .as_u64()
                    .and_then(|line| u32::try_from(line).ok())
                    .filter(|line| *line > 0)
            })
            .collect();
        let points: Vec<_> = entries
            .iter()
            .zip(&lines)
            .map(|(entry, line)| {
                let line = (*line)?;
                let condition = optional_expression(entry, "condition")
                    .ok()?
                    .map(str::to_owned);
                let hit_count = optional_expression(entry, "hitCondition")
                    .ok()?
                    .map(|hit| hit.trim().parse().unwrap_or(1));
                let log_expression = optional_expression(entry, "logMessage")
                    .ok()?
                    .map(str::to_owned);
                Some(Breakpoint {
                    line,
                    condition,
                    hit_count,
                    log_expression,
                })
            })
            .collect();
        let accepted = points
            .iter()
            .map(|point| point.is_some() && module.is_some())
            .collect::<Vec<_>>();
        if let Some(module) = module {
            let mut candidate = self.breakpoints.clone();
            candidate.insert(module.clone(), points.into_iter().flatten().collect());
            if candidate.len() > 2048 {
                return Err(INVALID);
            }
            self.server()?
                .breakpoints(candidate.iter().map(|(id, lines)| (id, lines.as_slice())))
                .map_err(|_| ENDPOINT)?;
            if candidate.get(&module).is_some_and(Vec::is_empty) {
                candidate.remove(&module);
            }
            self.breakpoints = candidate;
        }
        Ok(
            json!({"breakpoints":lines.iter().zip(accepted).map(|(line, verified)| line.as_ref().map_or_else(|| json!({"verified":false}), |line| json!({"verified":verified,"line":line}))).collect::<Vec<_>>() }),
        )
    }

    fn set_exception_breakpoints(&self, arguments: &Value) -> Result<Value, Failure> {
        let filters = optional_array(arguments, "filters")?;
        let options = optional_array(arguments, "filterOptions")?;
        if filters.len() > 1
            || options.len() > 1
            || filters.iter().any(|id| id.as_str() != Some(ERROR_FILTER))
            || options
                .iter()
                .any(|option| option["filterId"].as_str() != Some(ERROR_FILTER))
        {
            return Err(INVALID);
        }
        let enabled = !filters.is_empty() || !options.is_empty();
        let condition = options
            .first()
            .map(|option| optional_expression(option, "condition"))
            .transpose()?
            .flatten();
        self.server()?
            .break_on_errors(enabled, condition)
            .map_err(|_| ENDPOINT)?;
        Ok(json!({"breakpoints":vec![json!({"verified":true}); filters.len() + options.len()]}))
    }

    fn threads(&self) -> Value {
        json!({"threads":self.state.threads.iter().map(|(id, thread)| {
            json!({"id":id,"name":format!("{} ({})", thread.target.kind, thread.target.session)})
        }).collect::<Vec<_>>()})
    }

    fn stack(&self, arguments: &Value) -> Result<Value, Failure> {
        let id = arguments["threadId"].as_u64().ok_or(INVALID)?;
        let stack = self
            .state
            .threads
            .get(&id)
            .and_then(|thread| thread.stack.as_ref())
            .ok_or(INVALID)?;
        let start = arguments["startFrame"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);
        let levels = arguments["levels"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|&n| n > 0)
            .unwrap_or(stack.len());
        let frames = stack
            .iter()
            .skip(start)
            .take(levels)
            .map(|(id, frame)| {
                let mut result =
                    json!({"id":id,"name":frame.name,"line":frame.line.max(1),"column":1});
                if let Some(path) = frame
                    .module
                    .as_ref()
                    .and_then(|module| self.config.sources.path(module))
                {
                    result["source"] = json!({"name":path.file_name(),"path":path});
                }
                result
            })
            .collect::<Vec<_>>();
        Ok(json!({"stackFrames":frames,"totalFrames":stack.len()}))
    }

    fn scopes(&mut self, arguments: &Value) -> Result<Value, Failure> {
        let position = self
            .state
            .frame(arguments["frameId"].as_u64().ok_or(INVALID)?)
            .ok_or(INVALID)?;
        let reference = self
            .state
            .add_variable(Variable {
                position,
                path: Vec::new(),
                indexed: false,
            })
            .ok_or(values::LIMIT)?;
        Ok(
            json!({"scopes":[{"name":self.language.text("debug-locals"),"presentationHint":"locals","expensive":false,"variablesReference":reference}]}),
        )
    }

    fn step(&mut self, request: &Request) -> Result<Value, Failure> {
        let id = request.arguments["threadId"].as_u64().ok_or(INVALID)?;
        let target = self
            .state
            .threads
            .get(&id)
            .filter(|thread| thread.stack.is_some())
            .ok_or(INVALID)?
            .target
            .id
            .clone();
        let step = match request.command.as_str() {
            "continue" => Step::Continue,
            "next" => Step::Over,
            "stepIn" => Step::In,
            _ => Step::Out,
        };
        let resumed = self.server()?.step(&target, step).map_err(|_| ENDPOINT)?;
        let mut ids = vec![id];
        for target in resumed {
            if let Some(other) = self.state.thread_id(&target)
                && other != id
                && self.state.threads[&other].stack.is_some()
            {
                ids.push(other);
            }
        }
        for id in ids {
            self.state.resume(id);
            self.output
                .event(
                    "continued",
                    &json!({"threadId":id,"allThreadsContinued":false}),
                )
                .map_err(|_| INVALID)?;
        }
        Ok(json!({"allThreadsContinued":false}))
    }

    fn targets(&self) -> Result<Value, Failure> {
        let targets = self.server()?.targets().map_err(|_| ENDPOINT)?;
        Ok(
            json!({"targets":targets.into_iter().filter(|target| self.state.thread_id(&target.id).is_none()).map(|target|
            json!({"id":target.id,"kind":target.kind,"session":target.session,"user":target.user})).collect::<Vec<_>>() }),
        )
    }

    fn select_target(&mut self, arguments: &Value) -> Result<Value, Failure> {
        let id = arguments["id"].as_str().ok_or(INVALID)?;
        let target = self
            .server()?
            .targets()
            .map_err(|_| ENDPOINT)?
            .into_iter()
            .find(|target| target.id == id)
            .ok_or(INVALID)?;
        self.attach_target(target).map_err(|_| ENDPOINT)?;
        Ok(json!({}))
    }

    fn attach_target(&mut self, target: Target) -> io::Result<()> {
        if self.state.thread_id(&target.id).is_some() || self.state.threads.len() >= MAX_THREADS {
            return Ok(());
        }
        self.server
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?
            .attach_target(&target.id)?;
        let id = self.state.add_thread(target);
        self.output
            .event("thread", &json!({"reason":"started","threadId":id}))
    }

    fn events(&mut self, events: Vec<Event>) -> io::Result<()> {
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                Event::Started(target) => self.attach_target(target)?,
                Event::Exited(target) => {
                    if let Some(id) = self.state.thread_id(&target) {
                        self.state.remove(id);
                        self.output
                            .event("thread", &json!({"reason":"exited","threadId":id}))?;
                    }
                }
                Event::Stack {
                    target,
                    action,
                    frames,
                    message,
                } => {
                    // dbgs can describe one exception stop with two adjacent events.
                    if matches!(action, StackAction::Breakpoint | StackAction::Step)
                        && matches!(events.peek(), Some(Event::Stack { target: next_target, action: StackAction::Exception, .. }) if next_target == &target)
                        && let Some(Event::Stack {
                            frames: exception_frames,
                            message,
                            ..
                        }) = events.next()
                    {
                        let frames = if exception_frames.is_empty() {
                            frames
                        } else {
                            exception_frames
                        };
                        self.stack_event(&target, StackAction::Exception, frames, &message)?;
                    } else {
                        self.stack_event(&target, action, frames, &message)?;
                    }
                }
                Event::Calculated(result) => {
                    if let Some(pending) = self.pending.remove(&result.id) {
                        self.finish(pending, vec![result])?;
                    }
                }
            }
        }
        Ok(())
    }

    fn stack_event(
        &mut self,
        target: &str,
        action: StackAction,
        frames: Vec<dbgs::Frame>,
        message: &str,
    ) -> io::Result<()> {
        let Some(id) = self.state.thread_id(target) else {
            return Ok(());
        };
        if !message.is_empty() {
            let category = if matches!(action, StackAction::Exception) {
                "stderr"
            } else {
                "console"
            };
            self.output.event(
                "output",
                &json!({"category":category,"output":format!("{message}\n")}),
            )?;
        }
        let reason = match action {
            StackAction::Breakpoint => "breakpoint",
            StackAction::Step => "step",
            StackAction::Exception => "exception",
            StackAction::Resume => {
                // A message-only or hit-counter-only event still suspends the target.
                if let Some(server) = &self.server
                    && let Err(error) = server.step(target, Step::Continue)
                {
                    self.state.stop(id, frames);
                    return Err(error);
                }
                return Ok(());
            }
        };
        self.state.stop(id, frames);
        self.output.event(
            "stopped",
            &json!({"reason":reason,"threadId":id,"allThreadsStopped":false}),
        )
    }

    fn calculation_request(&mut self, request: Request) -> io::Result<()> {
        let calculation = if self.pending.len() >= MAX_PENDING {
            Err(INVALID)
        } else if request.command == "evaluate" {
            self.expression(&request.arguments)
        } else {
            request.arguments["variablesReference"]
                .as_u64()
                .and_then(|id| self.state.variable(id))
                .map(|variable| (variable, CalculationPhase::Children))
                .ok_or(INVALID)
        };
        let (variable, phase) = match calculation {
            Ok(calculation) => calculation,
            Err(key) => return self.output.response(&request, Err(self.language.text(key))),
        };
        self.submit(Pending {
            request,
            variable,
            phase,
            deadline: Instant::now() + Duration::from_secs(4),
        })
    }

    fn submit(&mut self, pending: Pending) -> io::Result<()> {
        if Instant::now() >= pending.deadline {
            return self.output.response(
                &pending.request,
                Err(self.language.text("debug-evaluation-timeout")),
            );
        }
        let id = dbgs::random_id()?;
        let variable = &pending.variable;
        let result = self.server().and_then(|server| {
            let target = &self
                .state
                .threads
                .get(&variable.position.thread)
                .ok_or(INVALID)?
                .target
                .id;
            server
                .calculate(
                    target,
                    variable.position.level,
                    &id,
                    &variable.path,
                    variable.indexed,
                )
                .map_err(|_| ENDPOINT)
        });
        match result {
            Ok(results) if results.iter().any(|result| result.id == id) => self.finish(
                pending,
                results
                    .into_iter()
                    .filter(|result| result.id == id)
                    .collect(),
            ),
            Ok(_) => {
                self.pending.insert(id, pending);
                Ok(())
            }
            Err(key) => self
                .output
                .response(&pending.request, Err(self.language.text(key))),
        }
    }

    fn expression(&mut self, arguments: &Value) -> Result<(Variable, CalculationPhase), Failure> {
        let expression = arguments["expression"]
            .as_str()
            .filter(|text| !text.is_empty() && text.len() <= MAX_DEBUG_EXPRESSION)
            .ok_or(INVALID)?;
        let position = self.expression_position(arguments).ok_or(INVALID)?;
        let thread = self
            .state
            .threads
            .get_mut(&position.thread)
            .ok_or(INVALID)?;
        let address = thread.free_snapshots.pop();
        if address.is_none() {
            if thread.snapshot_count >= MAX_SNAPSHOTS_PER_TARGET {
                return Err("debug-snapshot-limit");
            }
            thread.snapshot_count += 1;
        }
        let variable = Variable {
            position,
            path: vec![Selector::Expression(values::store_expression(
                expression,
                address.as_deref(),
            ))],
            indexed: false,
        };
        let phase = address.map_or(CalculationPhase::Store, CalculationPhase::ReplaceSnapshot);
        Ok((variable, phase))
    }

    fn expression_position(&self, arguments: &Value) -> Option<Position> {
        if let Some(id) = arguments["frameId"].as_u64() {
            self.state.frame(id)
        } else {
            let mut stopped = self
                .state
                .threads
                .values()
                .filter_map(|thread| thread.stack.as_ref());
            let frame = stopped.next()?.first()?.0;
            if stopped.next().is_some() {
                return None;
            }
            self.state.frame(frame)
        }
    }

    fn finish(&mut self, mut pending: Pending, results: Vec<Calculation>) -> io::Result<()> {
        if !self.state.is_current(&pending.variable.position) {
            return self
                .output
                .response(&pending.request, Err(self.language.text(INVALID)));
        }
        if let Some(failed) = results.iter().find(|value| value.value.error) {
            let thread = self
                .state
                .threads
                .get_mut(&pending.variable.position.thread);
            let hidden = match pending.phase {
                CalculationPhase::Store => {
                    if let Some(thread) = thread {
                        thread.snapshot_count -= 1;
                    }
                    false
                }
                CalculationPhase::ReadSnapshot(_) => true,
                CalculationPhase::ReplaceSnapshot(address) => {
                    let hidden = failed.value.text.contains(&address);
                    if let Some(thread) = thread {
                        thread.free_snapshots.push(address);
                    }
                    hidden
                }
                CalculationPhase::Children => {
                    matches!(pending.variable.path.first(), Some(Selector::Snapshot(address))
                        if failed.value.text.contains(address))
                }
            };
            let message = if hidden || failed.value.text.is_empty() {
                self.language.text("debug-evaluation-failed")
            } else {
                failed.value.text.clone()
            };
            return self.output.response(&pending.request, Err(message));
        }
        let result = match pending.phase {
            CalculationPhase::Store => {
                let Some(address) = results
                    .first()
                    .and_then(|result| values::snapshot_address(&result.value.text))
                else {
                    return self.output.response(
                        &pending.request,
                        Err(self.language.text("debug-evaluation-failed")),
                    );
                };
                pending.variable.path = vec![Selector::Snapshot(address.to_owned())];
                pending.phase = CalculationPhase::ReadSnapshot(address.to_owned());
                return self.submit(pending);
            }
            CalculationPhase::ReadSnapshot(address)
            | CalculationPhase::ReplaceSnapshot(address) => {
                let Some(result) = results.first() else {
                    return self.output.response(
                        &pending.request,
                        Err(self.language.text("debug-evaluation-failed")),
                    );
                };
                pending.variable.path = vec![Selector::Snapshot(address.clone())];
                let evaluated =
                    values::evaluated(&mut self.state, &pending.variable, &result.value);
                if let Some(thread) = self
                    .state
                    .threads
                    .get_mut(&pending.variable.position.thread)
                {
                    if result.value.can_expand() && evaluated.is_ok() {
                        thread.held_snapshots.push(address);
                    } else {
                        thread.free_snapshots.push(address);
                    }
                }
                evaluated
            }
            CalculationPhase::Children => {
                values::children(&mut self.state, &pending.variable, results)
            }
        };
        self.output.response(
            &pending.request,
            result.map_err(|key| self.language.text(key)),
        )
    }

    fn cancel(&mut self, arguments: &Value) -> io::Result<()> {
        let seq = arguments["requestId"].as_u64();
        let id = self
            .pending
            .iter()
            .find_map(|(id, pending)| (Some(pending.request.seq) == seq).then(|| id.clone()));
        if let Some(pending) = id.and_then(|id| self.pending.remove(&id)) {
            self.output.response(
                &pending.request,
                Err(self.language.text("component-cancelled")),
            )?;
        }
        Ok(())
    }

    fn cancel_pending(&mut self) -> io::Result<()> {
        for (_, pending) in std::mem::take(&mut self.pending) {
            self.output.response(
                &pending.request,
                Err(self.language.text("component-cancelled")),
            )?;
        }
        Ok(())
    }

    fn expire(&mut self) -> io::Result<()> {
        let now = Instant::now();
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| {
                pending.deadline <= now || !self.state.is_current(&pending.variable.position)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(pending) = self.pending.remove(&id) {
                let key = if self.state.is_current(&pending.variable.position) {
                    "debug-evaluation-timeout"
                } else {
                    INVALID
                };
                self.output
                    .response(&pending.request, Err(self.language.text(key)))?;
            }
        }
        Ok(())
    }

    fn cleanup(&mut self) {
        if let Some(poll) = &self.poll {
            poll.cancel();
        }
        if let Some(server) = self.server.take() {
            let start = Instant::now();
            let mut stopped = self.state.stopped();
            while let Some(id) = stopped.pop_first() {
                if start.elapsed() >= Duration::from_secs(2) {
                    break;
                }
                if let Ok(resumed) = server.step(&self.state.threads[&id].target.id, Step::Continue)
                {
                    for target in resumed {
                        if let Some(thread) = self.state.thread_id(&target) {
                            stopped.remove(&thread);
                        }
                    }
                }
            }
            let _ = server.detach();
        }
        if let Some(poll) = self.poll.take() {
            poll.stop();
        }
        self.client.take();
    }
}
