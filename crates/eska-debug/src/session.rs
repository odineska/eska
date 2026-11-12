//! One serialized owner of DAP state, with background HTTP polling and calculations.
mod breakpoints;
mod calculation;
mod client;
mod poll;
mod values;

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufReader, BufWriter, Write},
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use eska_build_runtime::runtime::{DebugConnection, DebugLaunch, ManagedDebugServer};
use eska_presentation::localization::{LocalizationValue, Localizer};
use eska_project_core::project::local_settings::{DEBUG_TARGET_TYPES, valid_auto_attach_types};
use serde_json::{Value, json};

use crate::{
    Mode,
    dap::{self, Output, Request},
    dbgs::{
        self, Breakpoint, Calculation, CalculationQuery, Event, Module, Page, Registration,
        Selector, Server, StackAction, Step, Target, View,
    },
    http::Cancellation,
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
    pub connection: DebugConnection,
    pub alias: String,
    pub auto_attach_types: Vec<String>,
    pub sources: Sources,
    pub mode: Mode,
    pub launch: Option<DebugLaunch>,
    pub production: bool,
    pub attach_selection: Option<AttachSelection>,
}

pub enum AttachSelection {
    MatchingTypes,
    Exact(String),
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
    Children(Page),
    Assign(String),
}

struct Pending {
    request: Request,
    variable: Variable,
    phase: CalculationPhase,
    deadline: Instant,
    cancellation: Arc<Cancellation>,
}

enum Input {
    Packet(io::Result<Option<Request>>),
    Events(io::Result<Vec<Event>>),
    Calculation(String, io::Result<Vec<Calculation>>),
}

struct Session<'a, W> {
    config: Option<Config>,
    output: Output<W>,
    language: &'a Localizer,
    server: Option<Arc<Server>>,
    endpoint: Option<url::Url>,
    managed_server: Option<ManagedDebugServer>,
    input: mpsc::SyncSender<Input>,
    poll: Option<Poll>,
    calculations: Option<calculation::Worker>,
    state: State,
    breakpoints: BTreeMap<Module, Vec<Breakpoint>>,
    pending: BTreeMap<String, Pending>,
    pause: Option<BTreeSet<u64>>,
    step_target: Option<u64>,
    client: Option<Client>,
    initialized: bool,
    configured: bool,
    ended: bool,
}

pub fn run(config: Option<Config>, language: &Localizer) -> io::Result<()> {
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
        endpoint: None,
        managed_server: None,
        input: send,
        poll: None,
        calculations: None,
        state: State::default(),
        breakpoints: BTreeMap::new(),
        pending: BTreeMap::new(),
        pause: None,
        step_target: None,
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
        if session
            .managed_server
            .as_mut()
            .is_some_and(ManagedDebugServer::stopped)
        {
            session.terminate(Some("debug-managed-server-stopped"))?;
            break;
        }
        if let Some(success) = session.client.as_mut().and_then(Client::completed) {
            session.terminate((!success).then_some("runtime-client-failed"))?;
            break;
        }
        if session.poll.as_ref().is_some_and(Poll::failed)
            || session
                .calculations
                .as_ref()
                .is_some_and(calculation::Worker::failed)
        {
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
                session.terminate(Some("debug-transport-lost"))?;
                break;
            }
            Ok(Input::Calculation(id, result)) => session.calculation_reply(&id, result)?,
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
        if matches!(
            request.command.as_str(),
            "evaluate" | "variables" | "setVariable"
        ) {
            return self.calculation_request(request);
        }
        let result = match request.command.as_str() {
            "initialize" if !self.initialized => {
                self.initialized = true;
                Ok(
                    json!({"supportsConfigurationDoneRequest":true,"supportsCancelRequest":true,
                    "supportsEvaluateForHovers":true, "supportsSingleThreadExecutionRequests":false,
                    "supportsVariablePaging":true, "supportsSetVariable":true,
                    "supportsConditionalBreakpoints":true,"supportsHitConditionalBreakpoints":true,
                    "supportsLogPoints":true,"supportsExceptionFilterOptions":true,
                    "exceptionBreakpointFilters":[{"filter":ERROR_FILTER,"label":self.language.text("debug-runtime-errors"),
                    "supportsCondition":true,"conditionDescription":self.language.text("debug-runtime-error-condition")}]}),
                )
            }
            "launch" | "attach" => self.open(&request),
            "configurationDone" if self.server.is_some() && !self.configured => self.configure(),
            "setBreakpoints" => self.set_breakpoints(&request.arguments),
            "setExceptionBreakpoints" => self.set_exception_breakpoints(&request.arguments),
            "threads" if self.server.is_some() => Ok(self.threads()),
            "stackTrace" => self.stack(&request.arguments),
            "scopes" => self.scopes(&request.arguments),
            "continue" | "next" | "stepIn" | "stepOut" => self.step(&request),
            "pause" => self.pause(&request.arguments),
            "eska/listDebugTargets" => self.targets(),
            "eska/attachDebugTarget" => self.select_target(&request.arguments),
            "eska/getAutoAttachTypes" if self.server.is_some() => {
                let selected = self.config.as_ref().map(|config| &config.auto_attach_types);
                Ok(json!({"available":DEBUG_TARGET_TYPES,"selected":selected}))
            }
            "eska/setAutoAttachTypes" => self.set_auto_attach_types(&request.arguments),
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
            if self.managed_server.is_some()
                && let Some(url) = &self.endpoint
            {
                let message = self.language.format(
                    "debug-managed-server-ready",
                    &[("url", LocalizationValue::Text(url.as_str()))],
                );
                self.output.event(
                    "output",
                    &json!({"category":"console","output":format!("{message}\n")}),
                )?;
            }
            if self.config.as_ref().is_some_and(|config| config.production) {
                self.output.event("output", &json!({"category":"stderr","output":format!("{}\n",self.language.text("debug-prod-warning"))}))?;
            }
        }
        Ok(())
    }

    fn open(&mut self, request: &Request) -> Result<Value, Failure> {
        if !self.initialized || self.server.is_some() {
            return Err(INVALID);
        }
        let mode = if request.command == "launch" {
            Mode::Launch
        } else {
            Mode::Attach
        };
        let standalone = self
            .config
            .is_none()
            .then(|| crate::standalone::configure(mode, &request.arguments))
            .transpose()?;
        let config = self
            .config
            .as_ref()
            .or(standalone.as_ref())
            .ok_or(INVALID)?;
        if config.mode != mode {
            return Err(INVALID);
        }
        let (endpoint, managed_server) = config
            .connection
            .open()
            .map_err(|_| "debug-managed-server-start-failed")?;
        let server = Server::new(&endpoint, config.alias.clone()).map_err(|_| ENDPOINT)?;
        match server.attach().map_err(|_| ENDPOINT)? {
            Registration::Registered => {}
            Registration::InfobaseBusy => return Err("debug-infobase-busy"),
            Registration::Rejected => return Err(ENDPOINT),
        }
        if server.auto_attach(&config.auto_attach_types).is_err() {
            let _ = server.detach();
            return Err(ENDPOINT);
        }
        let server = Arc::new(server);
        let worker =
            calculation::Worker::start(Arc::clone(&server), self.input.clone()).map_err(|_| {
                let _ = server.detach();
                ENDPOINT
            })?;
        let Ok(poll) = Poll::start(Arc::clone(&server), self.input.clone()) else {
            worker.stop();
            let _ = server.detach();
            return Err(ENDPOINT);
        };
        self.poll = Some(poll);
        self.calculations = Some(worker);
        self.server = Some(server);
        self.endpoint = Some(endpoint);
        self.managed_server = managed_server;
        if standalone.is_some() {
            self.config = standalone;
        }
        Ok(json!({}))
    }

    fn configure(&mut self) -> Result<Value, Failure> {
        let config = self.config.as_ref().ok_or(INVALID)?;
        if let Some(selection) = &config.attach_selection {
            let targets = self.server()?.targets().map_err(|_| ENDPOINT)?;
            let targets = match selection {
                AttachSelection::MatchingTypes => targets
                    .into_iter()
                    .filter(|target| config.auto_attach_types.contains(&target.kind))
                    .collect(),
                AttachSelection::Exact(id) => vec![
                    targets
                        .into_iter()
                        .find(|target| target.id.eq_ignore_ascii_case(id))
                        .ok_or("debug-target-not-found")?,
                ],
            };
            for target in targets {
                self.attach_target(target).map_err(|_| ENDPOINT)?;
            }
        }
        let launch = self.config.as_mut().and_then(|config| config.launch.take());
        let endpoint = self.endpoint.as_ref().ok_or(INVALID)?;
        self.client = launch
            .map(|launch| Client::launch(launch, endpoint.clone()))
            .transpose()
            .map_err(|_| {
                self.ended = true;
                "runtime-client-failed"
            })?;
        self.configured = true;
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
        let config = self.config.as_ref().ok_or(INVALID)?;
        let module = config.sources.module(Path::new(path)).cloned();
        let points: Vec<_> = entries
            .iter()
            .map(|entry| {
                let line = entry["line"]
                    .as_u64()
                    .and_then(|line| u32::try_from(line).ok())
                    .filter(|line| *line > 0);
                (
                    line,
                    line.ok_or(INVALID)
                        .and_then(|line| breakpoints::parse(entry, line)),
                )
            })
            .collect();
        let responses = points
            .iter()
            .map(|(line, point)| {
                let mut response = json!({"verified":module.is_some() && point.is_ok()});
                if let Some(line) = line {
                    response["line"] = json!(line);
                }
                if let Err(key) = point {
                    response["message"] = json!(self.language.text(key));
                }
                response
            })
            .collect::<Vec<_>>();
        if let Some(module) = module {
            let mut candidate = self.breakpoints.clone();
            candidate.insert(
                module.clone(),
                points
                    .into_iter()
                    .filter_map(|(_, point)| point.ok())
                    .collect(),
            );
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
        Ok(json!({"breakpoints":responses}))
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
        let sources = &self.config.as_ref().ok_or(INVALID)?.sources;
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
                    .and_then(|module| sources.path(module))
                    && let Some(display_path) = path.to_str()
                    && let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str)
                {
                    result["source"] = json!({"name":name,"path":display_path});
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
                view: View::Context,
                indexed_count: None,
            })
            .ok_or(values::LIMIT)?;
        Ok(
            json!({"scopes":[{"name":self.language.text("debug-locals"),"presentationHint":"locals","expensive":false,"variablesReference":reference}]}),
        )
    }

    fn pause(&mut self, arguments: &Value) -> Result<Value, Failure> {
        let thread = arguments["threadId"].as_u64().ok_or(INVALID)?;
        let running: BTreeSet<_> = self
            .state
            .threads
            .iter()
            .filter_map(|(&id, thread)| thread.stack.is_none().then_some(id))
            .collect();
        if !running.contains(&thread) {
            return Err(INVALID);
        }
        // dbgs applies this request to the whole debugger registration.
        self.server()?
            .break_on_next_statement(true)
            .map_err(|_| ENDPOINT)?;
        self.pause = Some(running);
        Ok(json!({}))
    }

    fn step(&mut self, request: &Request) -> Result<Value, Failure> {
        if self.assignment_pending() {
            return Err("debug-value-assignment-in-progress");
        }
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
        if self.pause.is_some() {
            self.server()?
                .break_on_next_statement(false)
                .map_err(|_| ENDPOINT)?;
            self.pause = None;
        }
        let resumed = self.server()?.step(&target, step).map_err(|_| ENDPOINT)?;
        self.step_target = (!matches!(step, Step::Continue)).then_some(id);
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

    fn set_auto_attach_types(&mut self, arguments: &Value) -> Result<Value, Failure> {
        let types = arguments["types"].as_array().ok_or(INVALID)?;
        if types.len() > DEBUG_TARGET_TYPES.len() {
            return Err(INVALID);
        }
        let types = types
            .iter()
            .map(|kind| kind.as_str().map(str::to_owned).ok_or(INVALID))
            .collect::<Result<Vec<_>, _>>()?;
        if !valid_auto_attach_types(&types) {
            return Err(INVALID);
        }
        self.server()?.auto_attach(&types).map_err(|_| ENDPOINT)?;
        self.config.as_mut().ok_or(INVALID)?.auto_attach_types = types;
        Ok(json!({}))
    }

    fn attach_target(&mut self, target: Target) -> io::Result<()> {
        if self.state.thread_id(&target.id).is_some() || self.state.threads.len() >= MAX_THREADS {
            return Ok(());
        }
        let server = self
            .server
            .as_ref()
            .ok_or_else(|| io::Error::other("session closed"))?;
        if self.pause.is_none() {
            server.break_on_next_statement(false)?;
        }
        server.attach_target(&target.id)?;
        let id = self.state.add_thread(target);
        self.output
            .event("thread", &json!({"reason":"started","threadId":id}))
    }

    fn events(&mut self, events: Vec<Event>) -> io::Result<()> {
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                Event::Started(target) => {
                    if self
                        .config
                        .as_ref()
                        .is_some_and(|config| config.auto_attach_types.contains(&target.kind))
                    {
                        self.attach_target(target)?;
                    }
                }
                Event::Exited(target) => {
                    if let Some(id) = self.state.thread_id(&target) {
                        if let Some(threads) = &mut self.pause {
                            threads.remove(&id);
                        }
                        if self.step_target == Some(id) {
                            self.step_target = None;
                        }
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
        let paused = !matches!(action, StackAction::Resume)
            && self
                .pause
                .as_mut()
                .is_some_and(|threads| threads.remove(&id));
        let stepped = !matches!(action, StackAction::Resume) && self.step_target == Some(id);
        if stepped {
            self.step_target = None;
        }
        let reason = match action {
            // dbgs can flag its own temporary stepping breakpoint as stopByBP.
            StackAction::Breakpoint if stepped && !self.has_breakpoint_at_top(&frames) => "step",
            StackAction::Breakpoint => "breakpoint",
            StackAction::Step if paused => "pause",
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

    fn has_breakpoint_at_top(&self, frames: &[dbgs::Frame]) -> bool {
        frames.first().is_some_and(|frame| {
            frame
                .module
                .as_ref()
                .and_then(|module| self.breakpoints.get(module))
                .is_some_and(|points| points.iter().any(|point| point.line == frame.line))
        })
    }

    fn calculation_request(&mut self, request: Request) -> io::Result<()> {
        let calculation = if self.pending.len() >= MAX_PENDING {
            Err(INVALID)
        } else if request.command == "evaluate" {
            self.expression(&request.arguments)
        } else if request.command == "setVariable" {
            self.assignment(&request.arguments)
        } else {
            let variable = request.arguments["variablesReference"]
                .as_u64()
                .and_then(|id| self.state.variable(id))
                .ok_or(INVALID);
            match variable.and_then(|variable| {
                values::page(&variable, &request.arguments).map(|page| (variable, page))
            }) {
                Ok((variable, Some(page))) => Ok((variable, CalculationPhase::Children(page))),
                Ok((_, None)) => {
                    return self.output.response(&request, Ok(json!({"variables":[]})));
                }
                Err(error) => Err(error),
            }
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
            cancellation: Arc::default(),
        })
    }

    fn assignment_pending(&self) -> bool {
        self.pending
            .values()
            .any(|pending| matches!(pending.phase, CalculationPhase::Assign(_)))
    }

    fn assignment(&self, arguments: &Value) -> Result<(Variable, CalculationPhase), Failure> {
        if self.assignment_pending() {
            return Err("debug-value-assignment-in-progress");
        }
        let parent = arguments["variablesReference"]
            .as_u64()
            .and_then(|id| self.state.variable(id))
            .ok_or(INVALID)?;
        let name = arguments["name"]
            .as_str()
            .filter(|name| name.len() <= MAX_DEBUG_EXPRESSION)
            .ok_or(INVALID)?;
        let expression = optional_expression(arguments, "value")?
            .filter(|value| !value.trim().is_empty())
            .ok_or(INVALID)?;
        let variable = values::assignment_target(parent, name)?;
        Ok((variable, CalculationPhase::Assign(expression.to_owned())))
    }

    fn submit(&mut self, pending: Pending) -> io::Result<()> {
        if Instant::now() >= pending.deadline {
            return self.fail_calculation(pending, "debug-evaluation-timeout");
        }
        let id = dbgs::random_id()?;
        let variable = &pending.variable;
        let page = match pending.phase {
            CalculationPhase::Children(page) if variable.view.paged() => Some(page),
            _ => None,
        };
        let Some(target) = self.state.threads.get(&variable.position.thread) else {
            return self.fail_calculation(pending, INVALID);
        };
        let (path, assignment) = match &pending.phase {
            CalculationPhase::Assign(expression) => match values::assignment_path(&variable.path) {
                Ok(path) => (path, Some(expression.clone())),
                Err(key) => return self.fail_calculation(pending, key),
            },
            _ => (variable.path.clone(), None),
        };
        let job = calculation::Job {
            query: CalculationQuery {
                id: id.clone(),
                target: target.target.id.clone(),
                level: variable.position.level,
                path,
                view: variable.view,
                page,
            },
            assignment,
            deadline: pending.deadline,
            cancellation: Arc::clone(&pending.cancellation),
        };
        if !self
            .calculations
            .as_ref()
            .is_some_and(|worker| worker.submit(job))
        {
            return self.fail_calculation(pending, INVALID);
        }
        self.pending.insert(id, pending);
        Ok(())
    }

    fn calculation_reply(
        &mut self,
        id: &str,
        result: io::Result<Vec<Calculation>>,
    ) -> io::Result<()> {
        match result {
            Ok(mut results) => {
                results.retain(|result| result.id == id);
                // An empty HTTP reply acknowledges a calculation whose result will be polled.
                if !results.is_empty()
                    && let Some(pending) = self.pending.remove(id)
                {
                    return self.finish(pending, results);
                }
            }
            Err(error) => {
                if let Some(pending) = self.pending.remove(id) {
                    let key = if Instant::now() >= pending.deadline
                        || matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        ) {
                        "debug-evaluation-timeout"
                    } else {
                        ENDPOINT
                    };
                    return self.fail_calculation(pending, key);
                }
            }
        }
        Ok(())
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
            view: View::Context,
            indexed_count: None,
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
        if !self.pending_current(&pending) {
            return self.fail_calculation(pending, INVALID);
        }
        if Instant::now() >= pending.deadline {
            return self.fail_calculation(pending, "debug-evaluation-timeout");
        }
        pending.cancellation.cancel();
        if let Some(failed) = results.iter().find(|value| value.value.error) {
            return self.calculation_error(pending, &failed.value);
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
                pending.cancellation = Arc::default();
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
            CalculationPhase::Children(page) => values::children(
                &mut self.state,
                &pending.variable,
                page,
                results,
                &self.language.text("debug-unreadable-property"),
            ),
            CalculationPhase::Assign(_) => {
                let result = results.first().ok_or("debug-value-assignment-failed");
                result.and_then(|result| {
                    values::assigned(&mut self.state, pending.variable, &result.value)
                })
            }
        };
        self.output.response(
            &pending.request,
            result.map_err(|key| self.language.text(key)),
        )
    }

    fn calculation_error(&mut self, pending: Pending, failed: &dbgs::Value) -> io::Result<()> {
        let message = {
            let fallback = if matches!(pending.phase, CalculationPhase::Assign(_)) {
                "debug-value-assignment-failed"
            } else {
                "debug-evaluation-failed"
            };
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
                    let hidden = failed.text.contains(&address);
                    if let Some(thread) = thread {
                        thread.free_snapshots.push(address);
                    }
                    hidden
                }
                CalculationPhase::Children(_) => {
                    matches!(pending.variable.path.first(), Some(Selector::Snapshot(address))
                        if failed.text.contains(address))
                }
                CalculationPhase::Assign(_) => false,
            };
            if hidden || failed.text.is_empty() {
                self.language.text(fallback)
            } else {
                failed.text.clone()
            }
        };
        self.output.response(&pending.request, Err(message))
    }

    fn cancel(&mut self, arguments: &Value) -> io::Result<()> {
        let seq = arguments["requestId"].as_u64();
        let id = self
            .pending
            .iter()
            .find_map(|(id, pending)| (Some(pending.request.seq) == seq).then(|| id.clone()));
        if let Some(pending) = id.and_then(|id| self.pending.remove(&id)) {
            self.cancel_calculation(pending)?;
        }
        Ok(())
    }

    fn cancel_pending(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        for (_, pending) in std::mem::take(&mut self.pending) {
            let reply = self.cancel_calculation(pending);
            result = result.and(reply);
        }
        result
    }

    fn expire(&mut self) -> io::Result<()> {
        let now = Instant::now();
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.deadline <= now || !self.pending_current(pending))
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(pending) = self.pending.remove(&id) {
                let key = if self.pending_current(&pending) {
                    "debug-evaluation-timeout"
                } else {
                    INVALID
                };
                self.fail_calculation(pending, key)?;
            }
        }
        Ok(())
    }

    fn pending_current(&self, pending: &Pending) -> bool {
        self.state.is_current(&pending.variable.position)
            && (!matches!(
                pending.phase,
                CalculationPhase::Children(_) | CalculationPhase::Assign(_)
            ) || pending.request.arguments["variablesReference"]
                .as_u64()
                .is_some_and(|id| self.state.has_variable(id)))
    }

    fn cancel_calculation(&mut self, pending: Pending) -> io::Result<()> {
        let request = self.discard_calculation(pending);
        self.output.response(&request, Err("cancelled".into()))
    }

    fn fail_calculation(&mut self, pending: Pending, key: Failure) -> io::Result<()> {
        let request = self.discard_calculation(pending);
        self.output.response(&request, Err(self.language.text(key)))
    }

    fn discard_calculation(&mut self, pending: Pending) -> Request {
        let unsent = pending.cancellation.cancel();
        if !unsent && matches!(pending.phase, CalculationPhase::Assign(_)) {
            self.state.invalidate_variable(&pending.variable);
        }
        if let Some(thread) = self
            .state
            .threads
            .get_mut(&pending.variable.position.thread)
        {
            match pending.phase {
                CalculationPhase::Store if unsent => thread.snapshot_count -= 1,
                CalculationPhase::ReplaceSnapshot(address) if unsent => {
                    thread.free_snapshots.push(address);
                }
                CalculationPhase::ReadSnapshot(address) => {
                    // The read stage never changes the reserved snapshot.
                    thread.free_snapshots.push(address);
                }
                _ => {}
            }
        }
        pending.request
    }

    fn cleanup(&mut self) {
        if let Some(worker) = self.calculations.take() {
            worker.stop();
        }
        if let Some(server) = self.server.take() {
            let start = Instant::now();
            let mut stopped = self.state.stopped();
            if !stopped.is_empty() && self.pause.take().is_some() {
                let _ = server.break_on_next_statement(false);
            }
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
        self.managed_server.take();
    }
}
