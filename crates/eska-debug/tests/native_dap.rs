//! Opt-in tests against a disposable 1C fixture prepared in the sibling playground.
#[path = "support/debug_relay.rs"]
mod debug_relay;
#[path = "support/posting.rs"]
mod posting;
#[path = "../../../tests/support/mod.rs"]
mod support;

use std::{
    collections::BTreeMap,
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Dap {
    process: Child,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Value>,
    seq: u64,
    stopped: Vec<Value>,
    output: Vec<Value>,
    durations: BTreeMap<String, Vec<f64>>,
}

impl Dap {
    fn start(root: &Path, profile: &str, mode: &str) -> Self {
        Self::start_language(root, profile, mode, "en-US")
    }

    fn start_language(root: &Path, profile: &str, mode: &str, language: &str) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_eska-debug"));
        command
            .current_dir(root)
            .args([
                "--lang",
                language,
                "dap",
                "--profile",
                profile,
                "--mode",
                mode,
            ])
            .args(if profile == "prod" {
                ["--sources", "current"].as_slice()
            } else {
                &[]
            });
        Self::spawn(command)
    }

    fn standalone(root: &Path, language: &str, launch: bool) -> Self {
        let name = Path::new(env!("CARGO_BIN_EXE_eska-debug"))
            .file_name()
            .unwrap();
        let binary = root.join(name);
        fs::copy(env!("CARGO_BIN_EXE_eska-debug"), &binary).unwrap();
        assert!(!root.join("eska.toml").exists());
        assert!(!root.join(".eska").exists());
        let mut command = Command::new(binary);
        command
            .current_dir(root)
            .env("PATH", if launch { "/usr/bin:/bin" } else { "" })
            .args(["--lang", language, "standalone"]);
        Self::spawn(command)
    }

    fn spawn(mut command: Command) -> Self {
        let mut process = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(process.stdout.take().unwrap());
        let input = process.stdin.take().unwrap();
        let (send, messages) = mpsc::sync_channel(64);
        thread::spawn(move || {
            while let Some(packet) = packet(&mut output) {
                if send.send(packet).is_err() {
                    break;
                }
            }
        });
        Self {
            process,
            input: Some(input),
            messages,
            seq: 0,
            stopped: Vec::new(),
            output: Vec::new(),
            durations: BTreeMap::new(),
        }
    }

    fn request(&mut self, command: &str, arguments: Value) -> Value {
        let start = Instant::now();
        let seq = self.send(command, arguments);
        loop {
            let reply = self.receive();
            if reply["request_seq"] == seq {
                assert_eq!(reply["command"], command);
                self.durations
                    .entry(command.to_owned())
                    .or_default()
                    .push(start.elapsed().as_secs_f64() * 1000.0);
                return reply;
            }
        }
    }

    fn send(&mut self, command: &str, arguments: Value) -> u64 {
        self.seq += 1;
        let mut request = json!({"seq":self.seq,"type":"request","command":command});
        request["arguments"] = arguments;
        let body = serde_json::to_vec(&request).unwrap();
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
        input.flush().unwrap();
        self.seq
    }

    fn success(&mut self, command: &str, arguments: Value) -> Value {
        let result = self.request(command, arguments);
        assert_eq!(result["success"], true, "{result}");
        result["body"].clone()
    }

    fn receive(&mut self) -> Value {
        self.receive_with_timeout(Duration::from_secs(12))
    }

    fn receive_with_timeout(&mut self, timeout: Duration) -> Value {
        let message = self
            .messages
            .recv_timeout(timeout)
            .unwrap_or_else(|error| panic!("DAP receive: {error}, output: {:?}", self.output));
        if message["event"] == "stopped" {
            self.stopped.push(message["body"].clone());
        }
        if message["event"] == "output" {
            self.output.push(message["body"].clone());
        }
        message
    }

    fn stop(&mut self) -> u64 {
        self.stop_event()["threadId"].as_u64().unwrap()
    }

    fn stop_event(&mut self) -> Value {
        loop {
            if !self.stopped.is_empty() {
                return self.stopped.remove(0);
            }
            let _ = self.receive();
        }
    }

    fn assert_output(&self, category: &str, expected: &str) {
        assert!(
            self.output.iter().any(|body| body["category"] == category
                && body["output"]
                    .as_str()
                    .is_some_and(|output| output.contains(expected))),
            "missing {expected:?} in {category}: {:?}",
            self.output
        );
    }

    fn shutdown(&mut self) {
        self.success("disconnect", json!({"terminateDebuggee":false}));
        self.wait_for_exit();
    }

    fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.process.try_wait().unwrap() {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(Instant::now() < deadline, "adapter did not exit");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
#[ignore = "requires the owned assignment fixture and its HTTP debug server"]
fn native_set_variable_updates_locals_properties_and_array_elements() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut dap = Dap::start(root, "test", "launch");
    let capabilities = dap.success("initialize", json!({"adapterID":"eska"}));
    assert_eq!(capabilities["supportsSetVariable"], true);
    dap.success("launch", json!({}));
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":7}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = &stack["stackFrames"][0]["id"];
    let scope = dap.success("scopes", json!({"frameId":frame}));
    let locals_ref = &scope["scopes"][0]["variablesReference"];
    let locals = dap.success("variables", json!({"variablesReference":locals_ref}));
    let change = |reference: &Value, name: &str, value: &str| json!({"variablesReference":reference,"name":name,"value":value});
    let reference = |name| {
        locals["variables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variable| variable["name"] == name)
            .unwrap()["variablesReference"]
            .clone()
    };

    let result = dap.success("setVariable", change(locals_ref, "A", "100 + 23"));
    assert_eq!(result["value"], "123");
    let bad = dap.request("setVariable", change(locals_ref, "A", "1 + )"));
    assert_eq!(bad["success"], false, "{bad}");
    assert!(
        bad["message"]
            .as_str()
            .unwrap()
            .contains("Ошибка в выражении"),
        "{bad}"
    );

    let structure = reference("Данные");
    let row = reference("Ряд");
    let fixed = reference("ФиксированныеДанные");
    for (parent, name, expression, expected) in
        [(&structure, "Число", "11", "11"), (&row, "0", "24", "24")]
    {
        let result = dap.success("setVariable", change(parent, name, expression));
        assert_eq!(result["value"], expected);
        let children = dap.success("variables", json!({"variablesReference":parent}));
        assert!(
            children["variables"]
                .as_array()
                .unwrap()
                .iter()
                .any(|variable| { variable["name"] == name && variable["value"] == expected }),
            "{children}"
        );
    }
    let read_only = dap.request("setVariable", change(&fixed, "Число", "1"));
    assert_eq!(read_only["success"], false, "{read_only}");
    let snapshot = dap.success("evaluate", json!({"frameId":frame,"expression":"Данные"}));
    let snapshot_ref = &snapshot["variablesReference"];
    let child = dap.success("variables", json!({"variablesReference":snapshot_ref}));
    assert_eq!(
        child["variables"][0]["presentationHint"]["attributes"],
        json!(["readOnly"])
    );
    let read_only = dap.request("setVariable", change(snapshot_ref, "Число", "1"));
    assert_eq!(read_only["success"], false, "{read_only}");

    let replacement = dap.success(
        "setVariable",
        json!({
            "variablesReference":locals_ref,"name":"Данные","value":"Новый Структура(\"Число\", 42)"
        }),
    );
    assert_ne!(replacement["variablesReference"], structure);
    assert_eq!(
        dap.request("variables", json!({"variablesReference":structure}))["success"],
        false
    );
    let current = dap.success(
        "variables",
        json!({"variablesReference":replacement["variablesReference"]}),
    );
    assert_eq!(current["variables"][0]["value"], "42");
    let result = dap.success("evaluate", json!({"frameId":frame,"expression":"A + Данные.Число + Ряд[0] + ФиксированныеДанные.Число"}));
    assert_eq!(result["result"], "194");
    dap.shutdown();
}

#[test]
#[ignore = "requires the pause fixture and one externally started managed client"]
fn native_pause_reports_both_running_clients_and_preserves_step_reason() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let mut dap = Dap::start(Path::new(&root), "test", "launch");
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", json!({}));
    let targets = dap.success("eska/listDebugTargets", json!({}));
    let existing = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["kind"] == "ManagedClient")
        .unwrap();
    dap.success("eska/attachDebugTarget", json!({"id":existing["id"]}));
    dap.success("configurationDone", json!({}));
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut clients = loop {
        let threads = dap.success("threads", json!({}));
        let ids: Vec<_> = threads["threads"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|thread| {
                thread["name"]
                    .as_str()
                    .unwrap()
                    .starts_with("ManagedClient")
            })
            .map(|thread| thread["id"].as_u64().unwrap())
            .collect();
        if ids.len() == 2 {
            break ids;
        }
        assert!(Instant::now() < deadline, "{threads}");
        thread::sleep(Duration::from_millis(40));
    };
    let selected = clients[0];
    dap.success("pause", json!({"threadId":selected}));
    let mut stopped = Vec::new();
    for _ in 0..2 {
        let event = dap.stop_event();
        assert_eq!(event["reason"], "pause", "{event}");
        stopped.push(event["threadId"].as_u64().unwrap());
    }
    clients.sort_unstable();
    stopped.sort_unstable();
    assert_eq!(stopped, clients);
    let mut old_frames = Vec::new();
    for id in &clients {
        let stack = dap.success("stackTrace", json!({"threadId":id}));
        assert_eq!(stack["stackFrames"][0]["line"], 6, "{stack}");
        old_frames.push(stack["stackFrames"][0]["id"].clone());
    }
    dap.success("next", json!({"threadId":selected}));
    let event = dap.stop_event();
    assert_eq!(event["reason"], "step", "{event}");
    assert_eq!(event["threadId"], selected, "{event}");
    let stack = dap.success("stackTrace", json!({"threadId":selected}));
    assert_eq!(stack["stackFrames"][0]["line"], 7, "{stack}");
    for old_frame in old_frames {
        assert_eq!(
            dap.request("scopes", json!({"frameId":old_frame}))["success"],
            false
        );
    }
    dap.shutdown();
}

#[test]
#[ignore = "requires an external client and a runner passing /C log to its own client"]
fn native_logpoint_reports_other_resumed_targets_and_rejects_stale_values() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let mut dap = Dap::start(root, "test", "launch");
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", json!({}));
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":11,"logMessage":"ESKA multi-target {Значение}"},{"line":16}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    assert_eq!(points["breakpoints"][1]["verified"], true);
    let targets = dap.success("eska/listDebugTargets", json!({}));
    let existing = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["kind"] == "ManagedClient")
        .unwrap();
    dap.success("eska/attachDebugTarget", json!({"id":existing["id"]}));
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["line"], 16);
    let frame = &stack["stackFrames"][0]["id"];
    let value = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"Новый Структура(\"Поле\", Значение)"}),
    );
    let reference = &value["variablesReference"];
    let child = dap.success("variables", json!({"variablesReference":reference}));
    assert_eq!(child["variables"][0]["value"], "41");
    loop {
        let event = dap.receive();
        assert_ne!(event["event"], "stopped", "{event}");
        if event["event"] == "continued" && event["body"]["threadId"] == thread {
            assert_eq!(event["body"]["allThreadsContinued"], false);
            break;
        }
    }
    dap.assert_output("console", "ESKA multi-target 22");
    assert_eq!(
        dap.request("variables", json!({"variablesReference":reference}))["success"],
        false
    );
    assert_eq!(
        dap.request("scopes", json!({"frameId":frame}))["success"],
        false
    );
    assert_eq!(dap.stop(), thread);
    let fresh = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(fresh["stackFrames"][0]["line"], 16);
    assert_ne!(fresh["stackFrames"][0]["id"], *frame);
    dap.shutdown();
}

#[test]
#[ignore = "requires a standalone fixture and a client started outside the adapter"]
fn native_transport_loss_reports_lost_and_busy_connections() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let target = args["targetId"]
        .as_str()
        .expect("exact target UUID from the fixture");
    let endpoint = args["debugServerUrl"].as_str().unwrap();
    let relay = debug_relay::Relay::start(
        endpoint.strip_prefix("http://").unwrap().parse().unwrap(),
        b"POST /e1crdbg/rdbg?cmd=evalExpr ",
    );
    let proxied = json!({
        "debugServerUrl":format!("http://{}", relay.address()),
        "sourceRoot":args["sourceRoot"],
        "infobaseAlias":args["infobaseAlias"],
        "targetId":target,
    });
    let mut dap = Dap::standalone(root, "en-US", false);
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("attach", proxied);
    dap.success("configurationDone", json!({}));
    let threads = dap.success("threads", json!({}));
    assert_eq!(threads["threads"].as_array().unwrap().len(), 1);

    let start = Instant::now();
    drop(relay);
    loop {
        let remaining = Duration::from_secs(16).saturating_sub(start.elapsed());
        assert!(
            !remaining.is_zero(),
            "adapter did not report the lost connection"
        );
        if dap.receive_with_timeout(remaining)["event"] == "terminated" {
            break;
        }
    }
    dap.assert_output("stderr", "The connection to the 1C debug server was lost.");
    dap.wait_for_exit();
    eprintln!("transport loss signalled after {:?}", start.elapsed());

    let mut retry = Dap::standalone(root, "ru-RU", false);
    retry.success("initialize", json!({"adapterID":"eska"}));
    let busy = retry.request("attach", args);
    assert_eq!(busy["success"], false, "{busy}");
    assert!(
        busy["message"]
            .as_str()
            .unwrap()
            .contains("Сервер отладки 1С уже зарегистрировал отладчик"),
        "{busy}"
    );
    retry.shutdown();
}

#[test]
#[ignore = "requires the owned standalone fixture and its HTTP debug server"]
fn native_evaluation_cancellation_and_timeout_keep_dap_responsive() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let mut args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let endpoint = args["debugServerUrl"].as_str().unwrap();
    let relay = debug_relay::Relay::start(
        endpoint.strip_prefix("http://").unwrap().parse().unwrap(),
        b"POST /e1crdbg/rdbg?cmd=evalExpr ",
    );
    args["debugServerUrl"] = json!(format!("http://{}", relay.address()));
    let mut dap = Dap::standalone(root, "en-US", true);
    let caps = dap.success("initialize", json!({"adapterID":"eska"}));
    assert_eq!(caps["supportsCancelRequest"], true);
    dap.success("launch", args);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let expression = json!({"frameId":stack["stackFrames"][0]["id"],"expression":"A + 1"});

    relay.arm();
    let evaluate = dap.send("evaluate", expression.clone());
    relay.await_stall();
    let start = Instant::now();
    let stack_request = dap.send("stackTrace", json!({"threadId":thread}));
    let cancel = dap.send("cancel", json!({"requestId":evaluate}));
    let mut replies = BTreeMap::new();
    while replies.len() < 3 {
        let reply = dap.receive();
        if let Some(id) = reply["request_seq"].as_u64() {
            assert!(replies.insert(id, reply).is_none());
        }
    }
    let latency = start.elapsed();
    assert!(latency < Duration::from_secs(1), "{latency:?}");
    assert_eq!(replies[&stack_request]["body"], stack);
    assert_eq!(replies[&evaluate]["success"], false);
    assert_eq!(replies[&evaluate]["message"], "cancelled");
    assert_eq!(replies[&cancel]["success"], true);
    relay.await_close();
    assert_eq!(dap.success("evaluate", expression.clone())["result"], "42");

    relay.arm();
    let start = Instant::now();
    let evaluate = dap.send("evaluate", expression.clone());
    relay.await_stall();
    let timeout = loop {
        let reply = dap.receive();
        if reply["request_seq"] == evaluate {
            break reply;
        }
        assert!(
            reply["request_seq"].is_null(),
            "unexpected response {reply}"
        );
    };
    let elapsed = start.elapsed();
    assert!(
        (Duration::from_millis(3800)..Duration::from_secs(5)).contains(&elapsed),
        "{elapsed:?}"
    );
    assert_eq!(timeout["success"], false, "{timeout}");
    assert_eq!(
        timeout["message"],
        "The server did not return a calculation for the active frame in time."
    );
    relay.await_close();
    assert_eq!(dap.success("evaluate", expression)["result"], "42");
    eprintln!("cancel_with_stack={latency:?} calculation_timeout={elapsed:?}");
    dap.shutdown();
}

#[test]
#[ignore = "requires the owned standalone fixture and its HTTP debug server"]
fn native_cancel_and_disconnect_interrupt_a_stalled_continue() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let mut args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let endpoint = args["debugServerUrl"].as_str().unwrap();
    let relay = debug_relay::Relay::start(
        endpoint.strip_prefix("http://").unwrap().parse().unwrap(),
        b"POST /e1crdbg/rdbg?cmd=step ",
    );
    args["debugServerUrl"] = json!(format!("http://{}", relay.address()));
    for (command, locale) in [("cancel", "ru-RU"), ("disconnect", "en-US")] {
        verify_stalled_continue(root, &args, &relay, command, locale);
    }
}

fn verify_stalled_continue(
    root: &Path,
    args: &Value,
    relay: &debug_relay::Relay,
    command: &str,
    locale: &str,
) {
    let mut dap = Dap::standalone(root, locale, true);
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", args.clone());
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();

    relay.arm();
    let step = dap.send("continue", json!({"threadId":thread}));
    relay.await_stall();
    let start = Instant::now();
    let threads = dap.send("threads", json!({}));
    let stack = dap.send("stackTrace", json!({"threadId":thread}));
    let interruption = dap.send(command, json!({"requestId":step}));
    let mut replies = BTreeMap::new();
    let mut terminated = false;
    while replies.len() < 4 || (command == "cancel" && !terminated) {
        let reply = dap.receive_with_timeout(Duration::from_secs(4));
        terminated |= reply["event"] == "terminated";
        if let Some(id) = reply["request_seq"].as_u64() {
            assert!(replies.insert(id, reply).is_none());
        }
    }
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
    assert!(
        replies[&threads]["body"]["threads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|target| target["id"] == thread)
    );
    assert_eq!(replies[&stack]["success"], false);
    let expected = if locale == "ru-RU" {
        "Дождитесь подтверждения шага или продолжения от 1С."
    } else {
        "Wait for 1C to confirm the current step or continue command."
    };
    assert_eq!(replies[&stack]["message"], expected);
    assert_eq!(replies[&step]["message"], "cancelled");
    assert_eq!(replies[&interruption]["success"], true);
    if command == "cancel" {
        dap.assert_output("stderr", "Начните новый сеанс отладки");
    }
    relay.await_close();
    dap.wait_for_exit();
    eprintln!("{command}_during_continue={elapsed:?}");
}

#[test]
#[ignore = "requires the owned enumerable fixture and its HTTP debug server"]
fn native_enumeration_expands_keys_and_nested_values_by_page() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut dap = Dap::start(root, "test", "launch");
    dap.success(
        "initialize",
        json!({"adapterID":"eska","supportsVariablePaging":true}),
    );
    dap.success("launch", json!({}));
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":6}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = &stack["stackFrames"][0]["id"];
    for expression in ["Ряд", "ФиксированныйРяд"] {
        let evaluated = dap.success("evaluate", json!({"frameId":frame,"expression":expression}));
        assert_eq!(evaluated["indexedVariables"], 2, "{evaluated}");
        let reference = &evaluated["variablesReference"];
        let mut seen = std::collections::BTreeSet::new();
        for index in 0..2 {
            let page = dap.success(
                "variables",
                json!({"variablesReference":reference,"filter":"indexed","start":index,"count":1}),
            );
            let entry = &page["variables"][0];
            assert_eq!(page["variables"].as_array().unwrap().len(), 1);
            assert!(
                entry["name"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("{index}: ")),
                "{entry}"
            );
            let expanded = dap.success(
                "variables",
                json!({"variablesReference":entry["variablesReference"]}),
            );
            let properties = expanded["variables"].as_array().unwrap();
            let key = properties
                .iter()
                .find(|value| value["name"] == "Ключ")
                .unwrap()["value"]
                .as_str()
                .unwrap();
            let value = properties
                .iter()
                .find(|value| value["name"] == "Значение")
                .unwrap();
            assert!(entry["name"].as_str().unwrap().ends_with(key));
            assert!(seen.insert(key.to_owned()));
            if key == "\"первый\"" {
                assert_eq!(value["value"], "11");
            } else {
                let nested = dap.success(
                    "variables",
                    json!({"variablesReference":value["variablesReference"]}),
                );
                assert_eq!(nested["variables"][0]["name"], "Вложенный");
                assert_eq!(nested["variables"][0]["value"], "22");
            }
        }
    }
    dap.shutdown();
}

#[test]
#[ignore = "requires the owned native fixture with managed dbgs configured"]
fn native_managed_debug_server_lifecycle() {
    use std::net::{TcpListener, TcpStream};

    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    for eof in [false, true] {
        let mode = if eof { "attach" } else { "launch" };
        let mut dap = Dap::start(root, "test", mode);
        dap.success("initialize", json!({"adapterID":"eska"}));
        dap.success(mode, json!({}));
        let port = loop {
            let msg = dap.receive();
            if let Some(address) = msg["body"]["output"]
                .as_str()
                .and_then(|output| output.split("http://127.0.0.1:").nth(1))
            {
                let digits: String = address.chars().take_while(char::is_ascii_digit).collect();
                break digits.parse::<u16>().unwrap();
            }
        };
        assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
        if eof {
            dap.success("configurationDone", json!({}));
            dap.input.take();
            dap.wait_for_exit();
        } else {
            dap.success(
                "setBreakpoints",
                json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
            );
            dap.success("configurationDone", json!({}));
            verify_stop(&mut dap, &source, 1, (3, 1));
            dap.shutdown();
        }
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "owned listener still active"
        );
    }
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = occupied.local_addr().unwrap();
    let settings = root.join(".eska/settings_local.toml");
    let profile = fs::read_to_string(&settings).unwrap();
    fs::write(settings, format!("{profile}\nport = {}\n", address.port())).unwrap();
    let mut dap = Dap::start(root, "test", "launch");
    dap.success("initialize", json!({"adapterID":"eska"}));
    let failure = dap.request("launch", json!({}));
    assert_eq!(failure["success"], false);
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("local 1C debug server")
    );
    assert!(
        TcpStream::connect(address).is_ok(),
        "existing listener was affected"
    );
    dap.shutdown();
}

#[test]
#[ignore = "requires the owned standalone fixture and its HTTP debug server"]
fn native_standalone_launch_with_standard_dap() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let mut dap = Dap::standalone(root, "en-US", true);
    dap.success(
        "initialize",
        json!({"adapterID":"onec-http","linesStartAt1":false,"columnsStartAt1":false,"pathFormat":"path"}),
    );
    let invalid = dap.request(
        "launch",
        json!({"sourceRoot":"src","debugServerUrl":"https://127.0.0.1:1550"}),
    );
    assert_eq!(invalid["success"], false);
    assert!(
        invalid["message"]
            .as_str()
            .unwrap()
            .contains("Invalid standalone DAP")
    );
    dap.success("launch", args);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let limits = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":0},{"line":u32::MAX}]}),
    );
    assert_eq!(limits["breakpoints"][0]["verified"], true);
    assert_eq!(limits["breakpoints"][0]["line"], 0);
    assert_eq!(limits["breakpoints"][1]["verified"], false);
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":2}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    assert_eq!(points["breakpoints"][0]["line"], 2);
    dap.success("configurationDone", json!({}));
    verify_stop(&mut dap, &source, 1, (2, 0));
    dap.shutdown();
    assert!(!root.join(".eska").exists());
}

#[test]
#[ignore = "requires a built EPF and the owned 1C standalone fixture"]
fn native_standalone_launches_verified_external_processing() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let source = Path::new(args["externalProcessing"]["sourceRoot"].as_str().unwrap())
        .join("EskaEpfWire/Forms/ProbeForm/Ext/Form/Module.bsl");
    let artifact = Path::new(args["externalProcessing"]["artifact"].as_str().unwrap());
    let mut dap = Dap::standalone(root, "en-US", true);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    let backup = fs::read(artifact).unwrap();
    fs::write(artifact, b"unrelated artifact").unwrap();
    let reply = dap.request("launch", args.clone());
    fs::write(artifact, backup).unwrap();
    assert_eq!(reply["success"], false);
    assert!(reply["message"].as_str().unwrap().contains("checksum"));
    let saved = fs::read(&source).unwrap();
    fs::write(&source, b"changed module").unwrap();
    let reply = dap.request("launch", args.clone());
    fs::write(&source, &saved).unwrap();
    assert_eq!(reply["success"], false);
    assert!(reply["message"].as_str().unwrap().contains("snapshot"));
    dap.success("launch", args);
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":4}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    dap.success("configurationDone", json!({}));
    println!("EPF_CLIENT_STARTED");
    let stopped = dap.stop_event();
    assert_eq!(stopped["reason"], "breakpoint");
    let thread = stopped["threadId"].as_u64().unwrap();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let top = &stack["stackFrames"][0];
    assert_eq!(top["line"], 4);
    assert_eq!(top["source"]["path"], json!(source));
    let result = dap.success(
        "evaluate",
        json!({"frameId":top["id"],"expression":"Значение","context":"watch"}),
    );
    assert_eq!(result["result"], "73");
    fs::write(&source, b"changed module").unwrap();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let point = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
    );
    assert_eq!(point["breakpoints"][0]["verified"], false);
    let cleared = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[]}),
    );
    fs::write(&source, saved).unwrap();
    assert_eq!(cleared["breakpoints"], json!([]));
    assert!(stack["stackFrames"][0]["source"].is_null());
    dap.shutdown();
    assert!(!root.join(".eska").exists());
}

#[test]
#[ignore = "requires an already loaded EPF in the owned 1C standalone fixture"]
fn native_standalone_attaches_to_loaded_external_processing() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let source = Path::new(args["externalProcessing"]["sourceRoot"].as_str().unwrap()).to_owned();
    let mut dap = Dap::standalone(root, "ru-RU", false);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    let mut invalid = args.clone();
    invalid["externalProcessing"]["debugUrl"] = json!("file:///wrong/EskaEpfWire.epf");
    let reply = dap.request("attach", invalid);
    assert_eq!(reply["success"], false);
    assert!(reply["message"].as_str().unwrap().contains("SHA-256"));
    verify_loaded_processing(&mut dap, &source, args);
    assert!(!root.join(".eska").exists());
}

#[test]
#[ignore = "requires an already loaded EPF and a processing project with a configured test profile"]
fn native_project_attaches_to_loaded_external_processing() {
    let root = env::var_os("ESKA_DEBUG_PROCESSING_PROJECT").unwrap();
    let root = Path::new(&root);
    let mut dap = Dap::start(root, "test", "attach");
    dap.success("initialize", json!({"adapterID":"eska-debug"}));
    verify_loaded_processing(&mut dap, &root.join("src"), json!({}));
}

fn verify_loaded_processing(dap: &mut Dap, source: &Path, arguments: Value) {
    let source = source.join("EskaEpfWire/Forms/ProbeForm/Ext/Form/Module.bsl");
    dap.success("attach", arguments);
    dap.success("configurationDone", json!({}));
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":14}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    let stopped = dap.stop_event();
    assert_eq!(stopped["reason"], "breakpoint");
    let thread = stopped["threadId"].as_u64().unwrap();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let top = &stack["stackFrames"][0];
    assert_eq!(top["source"]["path"], json!(source));
    assert_eq!(top["line"], 14);
    let scopes = dap.success("scopes", json!({"frameId":top["id"]}));
    let locals = dap.success(
        "variables",
        json!({"variablesReference":scopes["scopes"][0]["variablesReference"]}),
    );
    let evaluated = dap.success(
        "evaluate",
        json!({"frameId":top["id"],"expression":"Значение","context":"watch"}),
    );
    assert!(
        evaluated["result"]
            .as_str()
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|calls| calls >= 2),
        "{evaluated}"
    );
    assert!(
        locals["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|variable| {
                variable["name"] == "Значение"
                    && variable["value"]
                        .as_str()
                        .and_then(|value| value.parse::<u32>().ok())
                        .is_some_and(|calls| calls >= 2)
            }),
        "{locals}"
    );
    dap.success("next", json!({"threadId":thread}));
    let next_event = dap.stop_event();
    assert_eq!(next_event["reason"], "step");
    assert_eq!(next_event["threadId"], thread);
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["line"], 15);
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(source));
    dap.shutdown();
}

#[test]
#[ignore = "requires an already loaded server EPF in the owned 1C standalone fixture"]
fn native_server_processing_steps_across_verified_sources() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let caller = Path::new(args["sourceRoot"].as_str().unwrap())
        .join("CommonModules/EskaEpfLoader/Ext/Module.bsl");
    let external = Path::new(args["externalProcessing"]["sourceRoot"].as_str().unwrap())
        .join("EskaEpfWire/Ext/ObjectModule.bsl");
    let mut dap = Dap::standalone(root, "en-US", false);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    dap.success("attach", args);
    dap.success("configurationDone", json!({}));
    let point = dap.success(
        "setBreakpoints",
        json!({"source":{"path":caller},"breakpoints":[{"line":3}]}),
    );
    assert_eq!(point["breakpoints"][0]["verified"], true);
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(caller));
    assert_eq!(stack["stackFrames"][0]["line"], 3);
    dap.success("stepIn", json!({"threadId":thread}));
    assert_eq!(dap.stop(), thread);
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(external));
    assert_eq!(stack["stackFrames"][1]["source"]["path"], json!(caller));
    dap.success("next", json!({"threadId":thread}));
    assert_eq!(dap.stop(), thread);
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let top = &stack["stackFrames"][0];
    assert_eq!(top["line"], 3);
    assert_eq!(top["source"]["path"], json!(external));
    let value = dap.success(
        "evaluate",
        json!({"frameId":top["id"],"expression":"Результат"}),
    );
    assert_eq!(value["result"], "42");
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":caller},"breakpoints":[]}),
    );
    dap.success("stepOut", json!({"threadId":thread}));
    let returned = dap.stop_event();
    assert_eq!(returned["reason"], "step");
    assert_eq!(returned["threadId"], thread);
    let mut stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(caller));
    if stack["stackFrames"][0]["line"] == 3 {
        dap.success("next", json!({"threadId":thread}));
        assert_eq!(dap.stop(), thread);
        stack = dap.success("stackTrace", json!({"threadId":thread}));
    }
    assert_eq!(stack["stackFrames"][0]["line"], 4);
    let value = dap.success(
        "evaluate",
        json!({"frameId":stack["stackFrames"][0]["id"],"expression":"Результат"}),
    );
    assert_eq!(value["result"], "42");
    let point = dap.success(
        "setBreakpoints",
        json!({"source":{"path":external},"breakpoints":[{"line":3}]}),
    );
    assert_eq!(point["breakpoints"][0]["verified"], true);
    dap.success("continue", json!({"threadId":thread}));
    let event = dap.stop_event();
    assert_eq!(event["reason"], "breakpoint");
    let stack = dap.success("stackTrace", json!({"threadId":event["threadId"]}));
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(external));
    assert_eq!(stack["stackFrames"][0]["line"], 3);
    dap.shutdown();
    assert!(!root.join(".eska").exists());
}

#[test]
#[ignore = "requires two deployed builds of the same server EPF in the owned 1C fixture"]
fn native_server_processing_rebinds_after_reload() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let first: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let second: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_EPF_RELOAD_ARGUMENTS").unwrap()).unwrap();
    let caller = Path::new(first["sourceRoot"].as_str().unwrap())
        .join("CommonModules/EskaEpfLoader/Ext/Module.bsl");
    let module_path = |args: &Value| {
        Path::new(args["externalProcessing"]["sourceRoot"].as_str().unwrap())
            .join("EskaEpfWire/Ext/ObjectModule.bsl")
    };
    let original = module_path(&first);
    let replacement = module_path(&second);
    let mut dap = Dap::standalone(root, "en-US", false);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    dap.success("attach", first);
    dap.success("configurationDone", json!({}));
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":caller},"breakpoints":[{"line":3}]}),
    );
    let thread = dap.stop();
    dap.success("stepIn", json!({"threadId":thread}));
    assert_eq!(dap.stop(), thread);
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(original));

    fs::write(env::var_os("ESKA_DEBUG_EPF_RELOAD_MARKER").unwrap(), b"").unwrap();
    dap.success("continue", json!({"threadId":thread}));
    let thread = dap.stop();
    dap.success("stepIn", json!({"threadId":thread}));
    assert_eq!(dap.stop(), thread);
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert!(stack["stackFrames"][0]["source"].is_null());
    assert_eq!(stack["stackFrames"][1]["source"]["path"], json!(caller));
    dap.success("next", json!({"threadId":thread}));
    assert_eq!(dap.stop(), thread);
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let value = dap.success(
        "evaluate",
        json!({"frameId":stack["stackFrames"][0]["id"],"expression":"Результат"}),
    );
    assert_eq!(value["result"], "84");
    dap.success("continue", json!({"threadId":thread}));
    dap.shutdown();

    let mut dap = Dap::standalone(root, "en-US", false);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    dap.success("attach", second);
    dap.success("configurationDone", json!({}));
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":replacement},"breakpoints":[{"line":3}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let top = &stack["stackFrames"][0];
    assert_eq!(top["source"]["path"], json!(replacement));
    assert_eq!(top["line"], 3);
    let value = dap.success(
        "evaluate",
        json!({"frameId":top["id"],"expression":"Результат"}),
    );
    assert_eq!(value["result"], "84");
    dap.shutdown();
}

#[test]
#[ignore = "requires a standalone fixture and an already running 1C client"]
fn native_standalone_attaches_with_standard_dap() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let mut dap = Dap::standalone(root, "ru-RU", false);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    let invalid = dap.request(
        "attach",
        json!({"sourceRoot":"","debugServerUrl":"http://127.0.0.1:1550"}),
    );
    assert_eq!(invalid["success"], false);
    assert!(
        invalid["message"]
            .as_str()
            .unwrap()
            .contains("Недопустимые параметры")
    );
    dap.success("attach", args);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":7}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let top = &stack["stackFrames"][0];
    assert_eq!(top["source"]["path"], json!(source));
    assert_eq!(top["line"], 7);
    let scope = dap.success("scopes", json!({"frameId":top["id"]}));
    let locals = dap.success(
        "variables",
        json!({"variablesReference":scope["scopes"][0]["variablesReference"]}),
    );
    assert!(
        locals["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["name"] == "A" && value["value"] == "41")
    );
    assert_eq!(
        dap.success(
            "evaluate",
            json!({"frameId":top["id"],"expression":"A + 1"})
        )["result"],
        "42"
    );
    dap.success("next", json!({"threadId":thread}));
    let thread = dap.stop();
    assert_eq!(
        dap.success("stackTrace", json!({"threadId":thread}))["stackFrames"][0]["line"],
        8
    );
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[]}),
    );
    dap.shutdown();
}

#[test]
#[ignore = "requires the owned breakpoints fixture and its HTTP debug server"]
fn native_breakpoint_conditions_logpoints_and_exception_filters() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut dap = Dap::start(root, "test", "launch");
    let capabilities = dap.success("initialize", json!({"adapterID":"eska"}));
    for key in [
        "supportsConditionalBreakpoints",
        "supportsHitConditionalBreakpoints",
        "supportsLogPoints",
        "supportsExceptionFilterOptions",
        "supportsExceptionInfoRequest",
    ] {
        assert_eq!(capabilities[key], true);
    }
    dap.success("launch", json!({}));
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[
            {"line":3,"condition":"A < 0"},{"line":5,"hitCondition":"2"},
            {"line":7,"logMessage":"ESKA log {A}; {{текст}} {\"}\"}"},
            {"line":8,"condition":"A = 5"},{"line":24},
            {"line":31,"condition":"J > 1","hitCondition":"2"},
            {"line":4,"hitCondition":"0"},{"line":10,"logMessage":"ESKA {"}
        ]}),
    );
    let points = points["breakpoints"].as_array().unwrap();
    assert!(points[..6].iter().all(|point| point["verified"] == true));
    assert_eq!(points[6]["verified"], false);
    assert!(
        points[6]["message"]
            .as_str()
            .unwrap()
            .contains("positive integer")
    );
    assert_eq!(points[7]["verified"], false);
    assert!(
        points[7]["message"]
            .as_str()
            .unwrap()
            .contains("single braces")
    );
    dap.success(
        "setExceptionBreakpoints",
        json!({"filters":[],"filterOptions":[{"filterId":"all","condition":"ESKA match"}]}),
    );
    dap.success("configurationDone", json!({}));
    let stop = |dap: &mut Dap, reason, line| {
        let event = dap.stop_event();
        assert_eq!(event["reason"], reason, "{event}");
        let thread = event["threadId"].as_u64().unwrap();
        let stack = dap.success("stackTrace", json!({"threadId":thread}));
        assert_eq!(stack["stackFrames"][0]["line"], line, "{stack}");
        (thread, stack["stackFrames"][0]["id"].clone())
    };
    let (thread, frame) = stop(&mut dap, "breakpoint", 5);
    assert_eq!(
        dap.success("evaluate", json!({"frameId":frame,"expression":"I"}))["result"],
        "2"
    );
    dap.success("continue", json!({"threadId":thread}));
    let (thread, _) = stop(&mut dap, "breakpoint", 8);
    dap.assert_output("console", "ESKA log 4; {текст} }");
    dap.success("continue", json!({"threadId":thread}));
    let (thread, _) = stop(&mut dap, "exception", 15);
    dap.assert_output("stderr", "ESKA match");
    assert_exception_info(&mut dap, thread, "ESKA match");
    dap.success("setExceptionBreakpoints", json!({"filters":[]}));
    dap.success("continue", json!({"threadId":thread}));
    let (thread, _) = stop(&mut dap, "breakpoint", 24);
    assert_eq!(
        dap.request("exceptionInfo", json!({"threadId":thread}))["success"],
        false
    );
    dap.success("setExceptionBreakpoints", json!({"filters":["all"]}));
    dap.success("continue", json!({"threadId":thread}));
    let (thread, _) = stop(&mut dap, "exception", 26);
    dap.assert_output("stderr", "ESKA unfiltered");
    assert_exception_info(&mut dap, thread, "ESKA unfiltered");
    dap.success("setExceptionBreakpoints", json!({"filters":[]}));
    dap.success("continue", json!({"threadId":thread}));
    let (_, frame) = stop(&mut dap, "breakpoint", 31);
    assert_eq!(
        dap.success("evaluate", json!({"frameId":frame,"expression":"J"}))["result"],
        "3"
    );
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[]}),
    );
    dap.shutdown();
}

fn assert_exception_info(dap: &mut Dap, thread: u64, message: &str) {
    let exception = dap.success("exceptionInfo", json!({"threadId":thread}));
    assert_eq!(exception["breakMode"], "always");
    assert!(exception["description"].as_str().unwrap().contains(message));
}

#[test]
#[ignore = "requires a 1C playground fixture with auto_attach_types = []"]
fn native_updates_auto_attach_types_without_losing_manual_selection() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut dap = Dap::start(root, "test", "launch");
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", json!({}));
    let types = dap.success("eska/getAutoAttachTypes", json!({}));
    assert_eq!(types["selected"], json!([]));
    assert!(
        types["available"]
            .as_array()
            .unwrap()
            .contains(&json!("ServerEmulation"))
    );
    assert_eq!(
        dap.request("eska/setAutoAttachTypes", json!({"types":["bad"]}))["success"],
        false
    );
    dap.success(
        "eska/setAutoAttachTypes",
        json!({"types":["ManagedClient"]}),
    );
    assert_eq!(
        dap.success("eska/getAutoAttachTypes", json!({}))["selected"],
        json!(["ManagedClient"])
    );
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let threads = dap.success("threads", json!({}));
    assert_eq!(threads["threads"].as_array().unwrap().len(), 1, "{threads}");
    assert_eq!(threads["threads"][0]["id"], thread);
    assert!(
        threads["threads"][0]["name"]
            .as_str()
            .unwrap()
            .starts_with("ManagedClient")
    );
    let targets = dap.success("eska/listDebugTargets", json!({}));
    let target = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["kind"] == "ServerEmulation")
        .unwrap();
    dap.success("eska/attachDebugTarget", json!({"id":target["id"]}));
    dap.success("eska/setAutoAttachTypes", json!({"types":[]}));
    assert_eq!(
        dap.success("threads", json!({}))["threads"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    dap.shutdown();
}

#[test]
#[ignore = "requires the owned collection fixture and its HTTP debug server"]
fn native_pages_large_collections_with_absolute_indices() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut dap = Dap::start(root, "test", "launch");
    let capabilities = dap.success(
        "initialize",
        json!({"adapterID":"eska","supportsVariablePaging":true}),
    );
    assert_eq!(capabilities["supportsVariablePaging"], true);
    dap.success("launch", json!({}));
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":9}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = &stack["stackFrames"][0]["id"];
    let evaluated = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"ПолучитьРяд(Ряд)"}),
    );
    assert_eq!(evaluated["indexedVariables"], 600, "{evaluated}");
    let reference = &evaluated["variablesReference"];
    assert_eq!(
        dap.request("variables", json!({"variablesReference":reference}))["success"],
        false
    );
    let page = |dap: &mut Dap, filter, start, count| {
        dap.success(
            "variables",
            json!({"variablesReference":reference,"filter":filter,"start":start,"count":count}),
        )["variables"]
            .clone()
    };
    let items = page(&mut dap, "indexed", 550, 5);
    assert_eq!(items[0]["name"], "550");
    assert_eq!(items[1]["value"], "551");
    assert_eq!(items[4]["name"], "554");
    assert_eq!(page(&mut dap, "indexed", 550, 5), items);
    if env::var_os("ESKA_DEBUG_TIMINGS").is_some() {
        let first = page(&mut dap, "indexed", 500, 100);
        let before = memory(dap.process.id());
        for _ in 0..24 {
            assert_eq!(page(&mut dap, "indexed", 500, 100), first);
        }
        let after = memory(dap.process.id());
        let durations = dap.durations.get_mut("variables").unwrap();
        let start = durations.len() - 24;
        let samples = &mut durations[start..];
        samples.sort_by(f64::total_cmp);
        eprintln!(
            "collection_page n=24 p50_ms={:.3} p95_ms={:.3} rss_hwm_before_kib={before:?} rss_hwm_after_kib={after:?}",
            samples[11].midpoint(samples[12]),
            samples[22]
        );
    }
    let nested = dap.success(
        "variables",
        json!({"variablesReference":items[0]["variablesReference"]}),
    );
    assert_eq!(nested["variables"][0]["name"], "Номер");
    assert_eq!(nested["variables"][0]["value"], "550");
    assert_eq!(
        page(&mut dap, "indexed", 598, 5).as_array().unwrap().len(),
        2
    );
    assert_eq!(page(&mut dap, "indexed", 600, 5), json!([]));
    assert_eq!(page(&mut dap, "named", 0, 0), json!([]));
    let calls = dap.success("evaluate", json!({"frameId":frame,"expression":"Счетчик"}));
    assert_eq!(calls["result"], "1");
    dap.shutdown();
}

#[test]
#[ignore = "requires the owned workspace fixture with two installed extensions"]
fn native_maps_breakpoints_and_stack_across_the_application() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let workspace = root.parent().unwrap();
    let module = "src/Ext/ManagedApplicationModule.bsl";
    let base = root.join(module);
    let extensions = BTreeMap::from([
        (workspace.join("первое расширение").join(module), "111"),
        (workspace.join("второе расширение").join(module), "222"),
    ]);
    let mut dap = Dap::start(root, "test", "launch");
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", json!({}));
    verify_application(&mut dap, &base, extensions);
    dap.shutdown();
}

#[test]
#[ignore = "requires standalone Designer XML sources with two installed extensions"]
fn native_standalone_maps_explicit_extension_roots() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let mut dap = Dap::standalone(root, "en-US", true);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    dap.success("launch", args);
    let base = root.join("src/Ext/ManagedApplicationModule.bsl");
    let extensions = BTreeMap::from([
        (
            root.join("первое расширение/Ext/ManagedApplicationModule.bsl"),
            "111",
        ),
        (
            root.join("второе расширение/Ext/ManagedApplicationModule.bsl"),
            "222",
        ),
    ]);
    verify_application(&mut dap, &base, extensions);
    dap.shutdown();
}

fn verify_application(
    dap: &mut Dap,
    base: &Path,
    mut extensions: BTreeMap<std::path::PathBuf, &str>,
) {
    for (source, line) in
        std::iter::once((base, 3)).chain(extensions.keys().map(|path| (path.as_path(), 4)))
    {
        let points = dap.success(
            "setBreakpoints",
            json!({"source":{"path":source},"breakpoints":[{"line":line}]}),
        );
        assert_eq!(
            points["breakpoints"][0]["verified"],
            true,
            "{}: {points}",
            source.display()
        );
    }
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(base));
    dap.success(
        "setBreakpoints",
        json!({"source":{"path":base},"breakpoints":[{"line":6}]}),
    );
    dap.success("continue", json!({"threadId":thread}));
    for _ in 0..2 {
        let thread = dap.stop();
        let stack = dap.success("stackTrace", json!({"threadId":thread}));
        let frame = &stack["stackFrames"][0];
        assert_eq!(frame["line"], 4, "{stack}");
        assert_eq!(frame["source"]["name"], "ManagedApplicationModule.bsl");
        let path = frame["source"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("unmapped extension frame: {stack}"));
        let expected = extensions
            .remove(Path::new(path))
            .unwrap_or_else(|| panic!("unexpected extension frame: {stack}"));
        assert_eq!(
            dap.success(
                "evaluate",
                json!({"frameId":frame["id"],"expression":"Значение"})
            )["result"],
            expected
        );
        dap.success("continue", json!({"threadId":thread}));
    }
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    assert_eq!(stack["stackFrames"][0]["line"], 6, "{stack}");
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(base));
}

impl Drop for Dap {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn packet(reader: &mut impl BufRead) -> Option<Value> {
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let length = line
        .trim()
        .strip_prefix("Content-Length: ")?
        .parse::<usize>()
        .ok()?;
    if length > 2 * 1024 * 1024 {
        return None;
    }
    line.clear();
    reader.read_line(&mut line).ok()?;
    if line != "\r\n" {
        return None;
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn memory(id: u32) -> (u64, u64) {
    let status = fs::read_to_string(format!("/proc/{id}/status")).unwrap_or_default();
    let field = |name| {
        status
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

fn cpu_ticks(id: u32) -> u64 {
    let stat = fs::read_to_string(format!("/proc/{id}/stat")).unwrap_or_default();
    let fields = stat.rsplit_once(')').map_or("", |(_, fields)| fields);
    fields
        .split_whitespace()
        .skip(11)
        .take(2)
        .filter_map(|ticks| ticks.parse::<u64>().ok())
        .sum()
}

fn verify_stop(dap: &mut Dap, source: &Path, samples: usize, position: (u32, u32)) {
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = stack["stackFrames"][0]["id"].as_u64().unwrap();
    assert_eq!(
        Path::new(stack["stackFrames"][0]["source"]["path"].as_str().unwrap()),
        eska_build_runtime::native_path(&source.canonicalize().unwrap()).as_ref(),
        "{stack}"
    );
    assert_eq!(stack["stackFrames"][0]["line"], position.0);
    assert_eq!(stack["stackFrames"][0]["column"], position.1);
    let scopes = dap.success("scopes", json!({"frameId":frame}));
    let reference = scopes["scopes"][0]["variablesReference"].as_u64().unwrap();
    for _ in 0..samples {
        dap.success("stackTrace", json!({"threadId":thread}));
        let variables = dap.success("variables", json!({"variablesReference":reference}));
        assert!(
            variables["variables"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["name"] == "A" && v["value"] == "41")
        );
        let result = dap.success("evaluate", json!({"frameId":frame,"expression":"A + 1"}));
        assert_eq!(result["result"], "42");
    }
    let structure = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"Новый Структура(\"Поле\", 17)"}),
    );
    assert!(
        structure["variablesReference"]
            .as_u64()
            .is_some_and(|reference| reference > 0)
    );
    let children = dap.success(
        "variables",
        json!({"variablesReference":structure["variablesReference"]}),
    );
    assert!(
        children["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "Поле" && v["value"] == "17")
    );
    let failed = dap.request("evaluate", json!({"frameId":frame,"expression":"1 + )"}));
    assert_eq!(failed["success"], false);
    dap.success("stepIn", json!({"threadId":thread}));
    assert_eq!(
        dap.request("variables", json!({"variablesReference":reference}))["success"],
        false
    );
    let thread = dap.stop();
    let nested = dap.success("stackTrace", json!({"threadId":thread}));
    let caller = nested["stackFrames"][1]["id"].as_u64().unwrap();
    let result = dap.success("evaluate", json!({"frameId":caller,"expression":"A + 2"}));
    assert_eq!(result["result"], "43");
    dap.success("next", json!({"threadId":thread}));
    let thread = dap.stop();
    dap.success("stepOut", json!({"threadId":thread}));
    let thread = dap.stop();
    dap.success("continue", json!({"threadId":thread}));
    let removed = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[]}),
    );
    assert_eq!(removed["breakpoints"], json!([]));
}

#[test]
#[ignore = "requires the owned 1C playground fixture"]
fn native_dap_release_series() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let profile = env::var("ESKA_NATIVE_PROFILE").unwrap_or_else(|_| "test".into());
    let rounds = env::var("ESKA_DEBUG_ROUNDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(1);
    let samples = if rounds > 1 { 10 } else { 1 };
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut measurements = BTreeMap::<String, Vec<f64>>::new();
    for round in 0..rounds {
        let started = Instant::now();
        let mut dap = Dap::start(root, &profile, "launch");
        dap.success(
            "initialize",
            json!({"adapterID":"eska","linesStartAt1":true,"columnsStartAt1":true}),
        );
        measurements
            .entry("spawn_to_initialize".into())
            .or_default()
            .push(started.elapsed().as_secs_f64() * 1000.0);
        dap.success("launch", json!({}));
        let breakpoints = dap.success(
            "setBreakpoints",
            json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
        );
        assert_eq!(breakpoints["breakpoints"][0]["verified"], true);
        let id = dap.process.id();
        if round == 0 && rounds > 1 {
            let before = cpu_ticks(id);
            thread::sleep(Duration::from_secs(2));
            eprintln!(
                "idle cpu_ticks={} duration_seconds=2",
                cpu_ticks(id).saturating_sub(before)
            );
        }
        dap.success("configurationDone", json!({}));
        verify_stop(&mut dap, &source, samples, (3, 1));
        eprintln!("round={round} memory_rss_hwm_kib={:?}", memory(id));
        dap.shutdown();
        for (command, times) in std::mem::take(&mut dap.durations) {
            measurements.entry(command).or_default().extend(times);
        }
    }
    for (name, mut values) in measurements {
        values.sort_by(f64::total_cmp);
        let mid = values.len() / 2;
        let median = values[mid].midpoint(values[(values.len() - 1) / 2]);
        let p95 = values[(values.len() * 95).div_ceil(100) - 1];
        eprintln!(
            "{name} n={} median_ms={median:.3} p95_ms={p95:.3}",
            values.len()
        );
    }
}

#[test]
#[ignore = "requires the owned 1C playground fixture with only a prod profile and a built eska-vcs"]
fn native_prod_source_gate_before_live_launch() {
    let project = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let project = Path::new(&project);
    let vcs = fs::canonicalize(env::var_os("ESKA_VCS_BINARY").unwrap()).unwrap();
    let fixture = support::TestDir::new();
    let git = |directory: &Path, args: &[&str]| {
        let mut command = Command::new("git");
        for (key, _) in env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(directory)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", fixture.0.join("absent-config"))
            .env("GIT_AUTHOR_NAME", "Eska Test")
            .env("GIT_AUTHOR_EMAIL", "eska@example.invalid")
            .env("GIT_COMMITTER_NAME", "Eska Test")
            .env("GIT_COMMITTER_EMAIL", "eska@example.invalid")
            .args(["-c", "core.hooksPath=", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
    };
    let (upstream, work) = prod_checkout(&fixture.0, project, &git);

    verify_prod_source_gate(&upstream, &work, &vcs, &git);
}

fn prod_checkout(root: &Path, project: &Path, git: &impl Fn(&Path, &[&str])) -> (PathBuf, PathBuf) {
    let upstream = root.join("сервер");
    let work = root.join("проект");
    copy_sources(&project.join("src"), &upstream.join("src"));
    fs::write(
        upstream.join("eska.toml"),
        "[project]\ntype = 'configuration'\n[vcs.workflow]\npreset = 'git-flow'\n[vcs.workflow.policy]\nmain_branch = 'deployed'\nremote = 'ship'\n",
    )
    .unwrap();
    git(&upstream, &["init", "-q", "-b", "deployed"]);
    fs::write(upstream.join(".gitignore"), "/.eska/\n").unwrap();
    git(&upstream, &["add", "."]);
    git(&upstream, &["commit", "-qm", "initial"]);
    git(
        root,
        &["clone", "-q", "--origin", "ship", "сервер", "проект"],
    );
    fs::create_dir(work.join(".eska")).unwrap();
    let settings = fs::read_to_string(project.join(".eska/settings_local.toml")).unwrap();
    assert!(settings.contains("[prod.infobase]") && !settings.contains("[test."));
    fs::write(work.join(".eska/settings_local.toml"), settings).unwrap();
    (upstream, work)
}

fn verify_prod_source_gate(
    upstream: &Path,
    work: &Path,
    vcs: &Path,
    git: &impl Fn(&Path, &[&str]),
) {
    let binary = |language| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_eska-debug"));
        command
            .current_dir(work)
            .env("ESKA_VCS_BINARY", vcs)
            .args(["--lang", language]);
        command
    };
    let check = |language, args: &[&str]| {
        let output = binary(language).arg("check").args(args).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["schema_version"], 1);
        assert_eq!(result["kind"], "prod-source-check");
        result["check"].clone()
    };
    let launch = |language, args: &[&str]| {
        let mut command = binary(language);
        command
            .args(["dap", "--profile", "prod", "--mode", "launch"])
            .args(args);
        command
    };
    let reject = |language, args: &[&str], message| {
        let output = launch(language, args).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{output:?}"
        );
    };

    let clean = check("en-US", &[]);
    assert_eq!(clean["status"], "current");
    assert_eq!(clean["main_branch"], "deployed");
    assert_eq!(clean["remote"], "ship");
    assert_eq!(clean, check("ru-RU", &[]));
    let unsaved = check("ru-RU", &["--unsaved"]);
    assert_eq!(unsaved["status"], "different");
    assert_eq!(unsaved["unsaved"], true);
    assert_eq!(unsaved["update"], Value::Null);
    reject("ru-RU", &["--unsaved"], "Исходники prod различаются");

    // Advance the selected remote without changing the BSL compiled into the fixture.
    let revision = "deployed-revision.txt";
    fs::write(upstream.join(revision), "2\n").unwrap();
    git(upstream, &["add", revision]);
    git(upstream, &["commit", "-qm", "advance deployment"]);
    let behind = check("en-US", &[]);
    assert_eq!(behind["status"], "different");
    assert_eq!(behind["local_main_behind"], 1);
    assert_eq!(behind["update"], "fast_forward");
    reject("en-US", &[], "Production sources differ");
    reject(
        "ru-RU",
        &["--unsaved", "--sources", "fast-forward"],
        "Отладка не запущена",
    );

    let source = work.join("src/Ext/ManagedApplicationModule.bsl");
    let original = fs::read_to_string(&source).unwrap();
    fs::write(&source, format!("{original}\n// dirty\n")).unwrap();
    let dirty = check("en-US", &[]);
    assert_eq!(dirty["dirty"], true);
    assert_eq!(dirty["update"], Value::Null);
    reject(
        "en-US",
        &["--sources", "fast-forward"],
        "Debugging did not start",
    );
    fs::write(&source, original).unwrap();
    assert!(!work.join(revision).exists());

    let mut dap = Dap::spawn(launch("en-US", &["--sources", "fast-forward"]));
    dap.success("initialize", json!({"adapterID":"eska"}));
    assert_eq!(fs::read_to_string(work.join(revision)).unwrap(), "2\n");
    dap.success("launch", json!({}));
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    dap.success("configurationDone", json!({}));
    verify_stop(&mut dap, &source, 1, (3, 1));
    dap.shutdown();
    assert_eq!(check("ru-RU", &[])["status"], "current");

    git(work, &["remote", "set-url", "ship", "../missing-remote"]);
    let unavailable = check("en-US", &[]);
    assert_eq!(unavailable["status"], "unverified");
    assert_eq!(unavailable["reason"], "remote");
    assert_eq!(unavailable["update"], Value::Null);
    reject("en-US", &[], "could not be verified");
}

fn copy_sources(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_sources(&entry.path(), &path);
        } else {
            fs::copy(entry.path(), path).unwrap();
        }
    }
}

#[test]
#[ignore = "requires a playground dbgs with an already running 1C client"]
fn native_attach_detach_series() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let mut process_memory = Vec::new();
    for round in 0..8 {
        let mut dap = Dap::start(root, "test", "attach");
        dap.success("initialize", json!({"adapterID":"eska"}));
        dap.success("attach", json!({}));
        dap.success("configurationDone", json!({}));
        let deadline = Instant::now() + Duration::from_secs(3);
        let target = loop {
            let targets = dap.success("eska/listDebugTargets", json!({}));
            if let Some(target) = targets["targets"].as_array().unwrap().first() {
                break target["id"].clone();
            }
            assert!(Instant::now() < deadline, "existing target not listed");
            thread::sleep(Duration::from_millis(50));
        };
        dap.success("eska/attachDebugTarget", json!({"id":target}));
        let threads = dap.success("threads", json!({}));
        assert_ne!(threads["threads"], json!([]));
        process_memory.push(memory(dap.process.id()));
        if round % 2 == 0 {
            dap.shutdown();
        } else {
            drop(dap.input.take());
            dap.wait_for_exit();
        }
    }
    eprintln!("attach/detach memory RSS/HWM KiB: {process_memory:?}");
}

#[test]
#[ignore = "requires the owned side-effect fixture and its HTTP debug server"]
fn evaluate_expands_a_nested_snapshot_without_running_the_function_again() {
    let root = env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap();
    let root = Path::new(&root);
    let source = root.join("src/Ext/ManagedApplicationModule.bsl");
    let mut dap = Dap::start(root, "test", "launch");
    dap.success("initialize", json!({"adapterID":"eska"}));
    dap.success("launch", json!({}));
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":5}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = stack["stackFrames"][0]["id"].clone();
    let evaluated = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"ПолучитьСнимок()"}),
    );
    let reference = evaluated["variablesReference"].clone();
    assert!(reference.as_u64().is_some_and(|id| id > 0), "{evaluated}");
    let counter = |dap: &mut Dap, frame: &Value| {
        dap.success("evaluate", json!({"frameId":frame,"expression":"Счетчик"}))["result"].clone()
    };
    assert_eq!(counter(&mut dap, &frame), "1");
    let number = |dap: &mut Dap, reference: &Value| {
        let fields = dap.success("variables", json!({"variablesReference":reference}));
        let array = fields["variables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "Данные")
            .unwrap();
        assert!(
            array["variablesReference"]
                .as_u64()
                .is_some_and(|id| id > 0),
            "{array}"
        );
        let items = dap.success(
            "variables",
            json!({"variablesReference":array["variablesReference"]}),
        );
        assert!(
            items["variables"][0]["variablesReference"]
                .as_u64()
                .is_some_and(|id| id > 0),
            "{items}"
        );
        let nested = dap.success(
            "variables",
            json!({"variablesReference":items["variables"][0]["variablesReference"]}),
        );
        assert_eq!(nested["variables"][0]["name"], "Номер");
        nested["variables"][0]["value"].clone()
    };
    let repeats = env::var("ESKA_DEBUG_SNAPSHOT_REPEATS")
        .ok()
        .map_or(3, |value| value.parse().unwrap());
    let pid = dap.process.id();
    let before = memory(pid);
    let started = Instant::now();
    for _ in 0..repeats {
        assert_eq!(number(&mut dap, &reference), "1");
    }
    if repeats > 3 {
        eprintln!(
            "snapshot expansions={repeats} elapsed={:?} RSS/HWM KiB: {before:?} -> {:?}",
            started.elapsed(),
            memory(pid)
        );
    }
    assert_eq!(counter(&mut dap, &frame), "1");
    let second = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"ПолучитьСнимок()"}),
    );
    assert_ne!(second["variablesReference"], reference);
    assert_eq!(number(&mut dap, &second["variablesReference"]), "2");
    assert_eq!(number(&mut dap, &reference), "1");
    assert_eq!(counter(&mut dap, &frame), "2");
    verify_opaque_evaluation_once(&mut dap, &frame, "4");
    assert_eq!(number(&mut dap, &reference), "1");
    dap.success("stepIn", json!({"threadId":thread}));
    let thread = dap.stop();
    assert_eq!(
        dap.request("variables", json!({"variablesReference":reference}))["success"],
        false
    );
    let nested = dap.success("stackTrace", json!({"threadId":thread}));
    let caller = nested["stackFrames"][1]["id"].clone();
    let value = dap.success(
        "evaluate",
        json!({"frameId":caller,"expression":"A + Счетчик"}),
    );
    assert_eq!(value["result"], "45");
    dap.shutdown();
}

fn verify_opaque_evaluation_once(dap: &mut Dap, frame: &Value, expected_calls: &str) {
    let writer = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"СоздатьЗаписьJSON()"}),
    );
    assert_eq!(writer["type"], "ЗаписьJSON");
    for _ in 0..3 {
        let children = dap.success(
            "variables",
            json!({"variablesReference":writer["variablesReference"]}),
        );
        let parameters = children["variables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "Параметры")
            .unwrap();
        assert_eq!(parameters["type"], "ПараметрыЗаписиJSON");
        let nested = dap.success(
            "variables",
            json!({"variablesReference":parameters["variablesReference"]}),
        );
        assert!(
            nested["variables"]
                .as_array()
                .unwrap()
                .iter()
                .any(|child| { child["name"] == "ПереносСтрок" })
        );
    }
    let reader = dap.success(
        "evaluate",
        json!({"frameId":frame,"expression":"СоздатьЧтениеJSON()"}),
    );
    assert_eq!(reader["type"], "ЧтениеJSON");
    let children = dap.success(
        "variables",
        json!({"variablesReference":reader["variablesReference"]}),
    );
    assert!(
        children["variables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| { child["name"] == "ТипТекущегоЗначения" })
    );
    let counter = dap.success("evaluate", json!({"frameId":frame,"expression":"Счетчик"}));
    assert_eq!(counter["result"], expected_calls);
}

#[test]
#[ignore = "requires the owned server EPF Evaluate fixture"]
fn native_server_evaluate_preserves_opaque_snapshot_and_children() {
    let root = env::var_os("ESKA_DEBUG_STANDALONE_ROOT").unwrap();
    let root = Path::new(&root);
    let args: Value =
        serde_json::from_str(&env::var("ESKA_DEBUG_STANDALONE_ARGUMENTS").unwrap()).unwrap();
    let source = Path::new(args["externalProcessing"]["sourceRoot"].as_str().unwrap())
        .join("EskaEpfWire/Ext/ObjectModule.bsl");
    let mut dap = Dap::standalone(root, "en-US", false);
    dap.success("initialize", json!({"adapterID":"onec-http"}));
    dap.success("attach", args);
    dap.success("configurationDone", json!({}));
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":5}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = &stack["stackFrames"][0]["id"];
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(source));
    verify_opaque_evaluation_once(&mut dap, frame, "2");
    dap.shutdown();
}
