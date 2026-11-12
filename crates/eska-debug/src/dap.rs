//! DAP framing and the lifecycle boundary around one external HTTP debugger.

use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use eska_build_runtime::runtime::DebugLaunch;
use eska_presentation::localization::Localizer;
use serde_json::{Value, json};

use crate::Mode;

const MAX_PACKET: usize = 4 * 1024 * 1024;
const ATTACH_DEADLINE: Duration = Duration::from_secs(20);
const STEP_DEADLINE: Duration = Duration::from_secs(8);
const DISCONNECT_DEADLINE: Duration = Duration::from_secs(3);

enum Input {
    Client(io::Result<Option<Value>>),
    Backend(io::Result<Option<Value>>),
    ClientExited(bool),
}

#[derive(Clone)]
struct Pending {
    seq: i64,
    command: String,
    deadline: Instant,
}

impl Pending {
    fn new(message: &Value, command: &str, timeout: Duration) -> io::Result<Self> {
        let seq = message["seq"].as_i64().ok_or_else(invalid)?;
        Ok(Self {
            seq,
            command: command.to_owned(),
            deadline: Instant::now() + timeout,
        })
    }

    fn response(&self, error: Option<&str>) -> Value {
        let mut response = json!({"type":"response","request_seq":self.seq,"command":self.command,"success":error.is_none()});
        if let Some(message) = error {
            response["message"] = json!(message);
        }
        response
    }
}

pub fn run(
    executable: &Path,
    config: Value,
    mode: Mode,
    launch: Option<DebugLaunch>,
    warning: Option<String>,
    localizer: &Localizer,
) -> io::Result<()> {
    let child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut child = OwnedAdapter(child);
    let adapter_input = child.0.stdin.take().ok_or_else(invalid)?;
    let adapter_output = child.0.stdout.take().ok_or_else(invalid)?;
    let (sender, messages) = mpsc::sync_channel(64);
    spawn_reader(io::stdin(), sender.clone(), Input::Client);
    spawn_reader(adapter_output, sender.clone(), Input::Backend);
    let mut session = Session {
        output: BufWriter::new(io::stdout().lock()),
        adapter: BufWriter::new(adapter_input),
        seq: 0,
        config,
        mode,
        launch,
        launch_worker: None,
        cancel_launch: Arc::new(AtomicBool::new(false)),
        events: sender,
        initialize: None,
        attach: None,
        attached: false,
        steps: BTreeMap::new(),
        disconnect: None,
        warning,
        localizer,
    };
    let result = loop {
        match messages.recv_timeout(Duration::from_millis(100)) {
            Ok(Input::Client(Ok(Some(message)))) => session.handle_client(message)?,
            Ok(Input::Backend(Ok(Some(message)))) => {
                if session.handle_backend(message)? {
                    break Ok(());
                }
            }
            Ok(Input::Client(Ok(None))) => {
                session.disconnect_after_eof()?;
                if session.disconnect.is_none() {
                    break Ok(());
                }
            }
            Ok(Input::ClientExited(success)) => {
                if !success {
                    session.diagnostic("runtime-client-failed")?;
                }
                session.send(json!({"type":"event","event":"terminated"}))?;
            }
            Ok(Input::Backend(Ok(None))) if session.disconnect.is_some() => break Ok(()),
            Ok(Input::Client(Err(error)) | Input::Backend(Err(error))) => break Err(error),
            Ok(Input::Backend(Ok(None))) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                session.diagnostic("debug-adapter-disconnected")?;
                session.send(json!({"type":"event","event":"terminated"}))?;
                break Err(invalid());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if session.expire()? {
            break Ok(());
        }
    };
    drop(session);
    result
}

struct OwnedAdapter(Child);

impl Drop for OwnedAdapter {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

struct Session<'a> {
    output: BufWriter<io::StdoutLock<'a>>,
    adapter: BufWriter<ChildStdin>,
    seq: u64,
    config: Value,
    mode: Mode,
    launch: Option<DebugLaunch>,
    launch_worker: Option<thread::JoinHandle<()>>,
    cancel_launch: Arc<AtomicBool>,
    events: SyncSender<Input>,
    initialize: Option<Pending>,
    attach: Option<Pending>,
    attached: bool,
    steps: BTreeMap<i64, Pending>,
    disconnect: Option<Pending>,
    warning: Option<String>,
    localizer: &'a Localizer,
}

impl Session<'_> {
    fn send(&mut self, mut message: Value) -> io::Result<()> {
        self.seq += 1;
        message["seq"] = json!(self.seq);
        write_message(&mut self.output, &message)
    }

    fn diagnostic(&mut self, key: &str) -> io::Result<()> {
        let text = self.localizer.text(key);
        self.send(json!({"type":"event","event":"output","body":{"category":"stderr","output":format!("{text}\n")}}))
    }

    fn handle_client(&mut self, mut message: Value) -> io::Result<()> {
        let command = message["command"].as_str().unwrap_or_default().to_owned();
        match command.as_str() {
            "initialize" => {
                let request = Pending::new(&message, &command, ATTACH_DEADLINE)?;
                if self.initialize.is_some() {
                    return self.send(
                        request.response(Some(&self.localizer.text("debug-request-unavailable"))),
                    );
                }
                self.initialize = Some(request);
            }
            "launch" | "attach" => {
                let request = Pending::new(&message, &command, ATTACH_DEADLINE)?;
                let expected = if self.mode == Mode::Launch {
                    "launch"
                } else {
                    "attach"
                };
                if command != expected || self.attach.is_some() || self.attached {
                    return self.send(
                        request.response(Some(&self.localizer.text("debug-request-unavailable"))),
                    );
                }
                self.attach = Some(request);
                message["command"] = json!("attach");
                message["arguments"] = self.config.clone();
            }
            "configurationDone" => {
                let request = Pending::new(&message, &command, ATTACH_DEADLINE)?;
                if !self.attached {
                    return self.send(
                        request.response(Some(&self.localizer.text("debug-request-unavailable"))),
                    );
                }
                self.start_client();
                return self.send(request.response(None));
            }
            "stepIn" | "stepOut" | "next" => {
                let request = Pending::new(&message, &command, STEP_DEADLINE)?;
                let thread = message["arguments"]["threadId"]
                    .as_i64()
                    .ok_or_else(invalid)?;
                if self.steps.contains_key(&thread) {
                    return self.send(
                        request.response(Some(&self.localizer.text("debug-request-unavailable"))),
                    );
                }
                self.steps.insert(thread, request);
            }
            "disconnect" => {
                self.disconnect = Some(Pending::new(&message, &command, DISCONNECT_DEADLINE)?);
                message["arguments"] = json!({"terminateDebuggee": false});
            }
            "terminate" => {
                let request = Pending::new(&message, &command, DISCONNECT_DEADLINE)?;
                return self.send(
                    request.response(Some(&self.localizer.text("debug-request-unavailable"))),
                );
            }
            _ => {}
        }
        write_message(&mut self.adapter, &message)
    }

    fn handle_backend(&mut self, mut message: Value) -> io::Result<bool> {
        if message["event"] == "AdapterLog" {
            return Ok(false);
        }
        if message["command"] == "initialize" && message["success"] == true {
            message["body"]["supportsConfigurationDoneRequest"] = json!(true);
            message["body"]["supportsTerminateRequest"] = json!(false);
            message["body"]["supportTerminateDebuggee"] = json!(false);
        }
        let response = message["request_seq"].as_i64();
        if self
            .initialize
            .as_ref()
            .is_some_and(|request| response == Some(request.seq))
        {
            self.initialize = None;
        }
        if let Some(request) = self.attach.as_ref()
            && response == Some(request.seq)
        {
            self.attached = message["success"] == true;
            if self.attached {
                message["command"] = json!(request.command);
            } else {
                message =
                    request.response(Some(&self.localizer.text("debug-endpoint-unavailable")));
            }
            self.attach = None;
        }
        if self.attached
            && message["event"] == "initialized"
            && let Some(warning) = self.warning.take()
        {
            self.send(json!({"type":"event","event":"output","body":{"category":"stderr","output":format!("{warning}\n")}}))?;
        }
        if let Some(seq) = response {
            self.steps.retain(|_, request| request.seq != seq);
        }
        // Adapter 0.4.1 confirms these operations by event and omits their successful response.
        if message["event"] == "continued"
            && let Some(thread) = message["body"]["threadId"].as_i64()
            && let Some(request) = self.steps.remove(&thread)
        {
            self.send(request.response(None))?;
        }
        let done = self
            .disconnect
            .as_ref()
            .is_some_and(|request| response == Some(request.seq));
        self.send(message)?;
        Ok(done)
    }

    fn start_client(&mut self) {
        if let Some(launch) = self.launch.take() {
            let cancelled = Arc::clone(&self.cancel_launch);
            let events = self.events.clone();
            self.launch_worker = Some(thread::spawn(move || {
                let success = launch.run(&cancelled).is_ok();
                if !cancelled.load(Ordering::Acquire) {
                    let _ = events.try_send(Input::ClientExited(success));
                }
            }));
        }
    }

    fn disconnect_after_eof(&mut self) -> io::Result<()> {
        if self.disconnect.is_none() && (self.attached || self.attach.is_some()) {
            let request = json!({"type":"request","seq":i32::MAX,"command":"disconnect","arguments":{"terminateDebuggee":false}});
            self.disconnect = Some(Pending::new(&request, "disconnect", DISCONNECT_DEADLINE)?);
            write_message(&mut self.adapter, &request)?;
        }
        Ok(())
    }

    fn expire(&mut self) -> io::Result<bool> {
        let now = Instant::now();
        if self
            .disconnect
            .as_ref()
            .is_some_and(|request| request.deadline <= now)
        {
            return Ok(true);
        }
        let startup = if self
            .initialize
            .as_ref()
            .is_some_and(|request| request.deadline <= now)
        {
            self.initialize
                .take()
                .map(|request| (request, "debug-initialize-timeout"))
        } else if self
            .attach
            .as_ref()
            .is_some_and(|request| request.deadline <= now)
        {
            self.attach
                .take()
                .map(|request| (request, "debug-attach-timeout"))
        } else {
            None
        };
        if let Some((request, key)) = startup {
            self.send(request.response(Some(&self.localizer.text(key))))?;
            return Ok(true);
        }
        let expired: Vec<_> = self
            .steps
            .iter()
            .filter(|(_, request)| request.deadline <= now)
            .map(|(&thread, _)| thread)
            .collect();
        for thread in expired {
            if let Some(request) = self.steps.remove(&thread) {
                self.send(request.response(Some(&self.localizer.text("debug-step-timeout"))))?;
            }
        }
        Ok(false)
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        self.cancel_launch.store(true, Ordering::Release);
        if let Some(worker) = self.launch_worker.take() {
            let _ = worker.join();
        }
    }
}

fn spawn_reader(
    stream: impl io::Read + Send + 'static,
    sender: SyncSender<Input>,
    event: fn(io::Result<Option<Value>>) -> Input,
) {
    thread::spawn(move || {
        let mut stream = BufReader::new(stream);
        loop {
            let result = read_message(&mut stream);
            let ended = !matches!(result, Ok(Some(_)));
            if sender.send(event(result)).is_err() || ended {
                break;
            }
        }
    });
}

fn read_message(stream: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut size = None;
    let mut headers = 0;
    loop {
        let mut line = String::new();
        if io::Read::take(&mut *stream, 1025).read_line(&mut line)? == 0 {
            return if headers == 0 {
                Ok(None)
            } else {
                Err(invalid())
            };
        }
        if line.len() > 1024 || headers >= 16 {
            return Err(invalid());
        }
        if line == "\r\n" {
            if size.is_some() {
                break;
            }
            continue;
        }
        headers += 1;
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
        {
            size = value
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|size| (1..=MAX_PACKET).contains(size));
            if size.is_none() {
                return Err(invalid());
            }
        }
    }
    let mut body = vec![0; size.ok_or_else(invalid)?];
    stream.read_exact(&mut body)?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| invalid())?;
    if value.is_object() {
        Ok(Some(value))
    } else {
        Err(invalid())
    }
}

fn write_message(stream: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(|_| invalid())?;
    write!(stream, "Content-Length: {}\r\n\r\n", body.len())?;
    stream.write_all(&body)?;
    stream.flush()
}

fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}
