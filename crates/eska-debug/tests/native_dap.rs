//! Opt-in tests against a disposable 1C fixture prepared in the sibling playground.
use std::{
    collections::BTreeMap,
    env, fs,
    io::{BufRead, BufReader, Write},
    path::Path,
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_eska-debug"));
        command
            .current_dir(root)
            .args([
                "--lang",
                "en-US",
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
        self.seq += 1;
        let mut request = json!({"seq":self.seq,"type":"request","command":command});
        request["arguments"] = arguments;
        let body = serde_json::to_vec(&request).unwrap();
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
        input.flush().unwrap();
        loop {
            let reply = self.receive();
            if reply["request_seq"] == self.seq {
                assert_eq!(reply["command"], command);
                self.durations
                    .entry(command.to_owned())
                    .or_default()
                    .push(start.elapsed().as_secs_f64() * 1000.0);
                return reply;
            }
        }
    }

    fn success(&mut self, command: &str, arguments: Value) -> Value {
        let result = self.request(command, arguments);
        assert_eq!(result["success"], true, "{result}");
        result["body"].clone()
    }

    fn receive(&mut self) -> Value {
        let message = self
            .messages
            .recv_timeout(Duration::from_secs(12))
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
            verify_stop(&mut dap, &source, 1);
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
        json!({"adapterID":"onec-http","linesStartAt1":true,"pathFormat":"path"}),
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
    let points = dap.success(
        "setBreakpoints",
        json!({"source":{"path":source},"breakpoints":[{"line":3}]}),
    );
    assert_eq!(points["breakpoints"][0]["verified"], true);
    dap.success("configurationDone", json!({}));
    verify_stop(&mut dap, &source, 1);
    dap.shutdown();
    assert!(!root.join(".eska").exists());
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
    dap.success("setExceptionBreakpoints", json!({"filters":[]}));
    dap.success("continue", json!({"threadId":thread}));
    let (thread, _) = stop(&mut dap, "breakpoint", 24);
    dap.success("setExceptionBreakpoints", json!({"filters":["all"]}));
    dap.success("continue", json!({"threadId":thread}));
    let (thread, _) = stop(&mut dap, "exception", 26);
    dap.assert_output("stderr", "ESKA unfiltered");
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
        json!({"source":{"path":source},"breakpoints":[{"line":7}]}),
    );
    dap.success("configurationDone", json!({}));
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = &stack["stackFrames"][0]["id"];
    let evaluated = dap.success("evaluate", json!({"frameId":frame,"expression":"Ряд"}));
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

fn verify_stop(dap: &mut Dap, source: &Path, samples: usize) {
    let thread = dap.stop();
    let stack = dap.success("stackTrace", json!({"threadId":thread}));
    let frame = stack["stackFrames"][0]["id"].as_u64().unwrap();
    assert_eq!(stack["stackFrames"][0]["source"]["path"], json!(source));
    assert_eq!(stack["stackFrames"][0]["line"], 3);
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
        verify_stop(&mut dap, &source, samples);
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
    for _ in 0..3 {
        assert_eq!(number(&mut dap, &reference), "1");
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
    assert_eq!(value["result"], "43");
    dap.shutdown();
}
