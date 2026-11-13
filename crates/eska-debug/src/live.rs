use std::{
    env, fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use eska_build_runtime::runtime::DebugLaunch;
use eska_project_core::project::{discovery, local_settings::ProfileName};

use crate::dbgs::{
    self, Breakpoint, Calculation, Event, Frame, Module, ModuleOrigin, Selector, Server,
    StackAction, Step,
};

#[test]
#[ignore = "requires the owned 1C playground fixture and its HTTP debug server"]
fn direct_http_breakpoint_and_selected_frame() {
    let root = PathBuf::from(env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap());
    let project = discovery::discover(&root).unwrap();
    let xml = fs::read_to_string(project.source().join("Configuration.xml")).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let configuration = document
        .descendants()
        .find(|node| node.has_tag_name("Configuration"))
        .unwrap();
    let module = Module {
        object: configuration
            .attribute("uuid")
            .unwrap()
            .to_ascii_lowercase(),
        property: "d22e852a-cf8a-4f77-8ccb-3548e7792bea".to_owned(),
        origin: ModuleOrigin::Configuration,
    };
    let launch = DebugLaunch::prepare(&project, ProfileName::Test, None).unwrap();
    let endpoint = url::Url::parse(
        launch
            .profile()
            .debug
            .as_ref()
            .unwrap()
            .url
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    let server = Server::new(&endpoint, "DefAlias".into()).unwrap();
    assert_eq!(server.attach().unwrap(), dbgs::Registration::Registered);
    let _registration = Registration(&server);
    server
        .auto_attach(&launch.profile().debug.as_ref().unwrap().auto_attach_types())
        .unwrap();
    server
        .breakpoints([(
            &module,
            [Breakpoint {
                line: 3,
                ..Default::default()
            }]
            .as_slice(),
        )])
        .unwrap();
    let worker = Client::launch(launch, endpoint);
    let (target, frames) = stopped(&server);
    assert_eq!(frames[0].module.as_ref(), Some(&module));
    assert_eq!(frames[0].line, 3);
    let locals = calculate(&server, &target, 0, &[]);
    eprintln!("locals: {locals:?}");
    assert!(
        locals
            .iter()
            .flat_map(|result| &result.children)
            .any(|value| value.name == "A" && value.text == "41")
    );
    assert_eq!(
        calculate(&server, &target, 0, &[Selector::Expression("A + 1".into())])[0]
            .value
            .text,
        "42"
    );
    server.step(&target, Step::In).unwrap();
    let (target, frames) = stopped(&server);
    assert_eq!(frames.len(), 2, "{frames:?}");
    assert_eq!(
        calculate(&server, &target, 1, &[Selector::Expression("A + 2".into())])[0]
            .value
            .text,
        "43"
    );
    server.step(&target, Step::Over).unwrap();
    stopped(&server);
    server.step(&target, Step::Out).unwrap();
    stopped(&server);
    server.breakpoints([(&module, [].as_slice())]).unwrap();
    server.step(&target, Step::Continue).unwrap();
    drop(worker);
}

#[test]
#[ignore = "requires a built external processing in the owned 1C playground fixture"]
fn external_module_breakpoint_and_evaluate() {
    let project = discovery::discover(&PathBuf::from(
        env::var_os("ESKA_DEBUG_LIVE_PROJECT").unwrap(),
    ))
    .unwrap();
    let launch = DebugLaunch::prepare(&project, ProfileName::Test, None).unwrap();
    let debug = launch.profile().debug.as_ref().unwrap();
    let endpoint = url::Url::parse(debug.url.as_deref().unwrap()).unwrap();
    let artifact = env::var("ESKA_DEBUG_LIVE_EPF").unwrap();
    let form = env::var_os("ESKA_DEBUG_LIVE_EPF_FORM").is_some();
    let (tag, scheme, property, line, expression, expected) = if form {
        (
            "Form",
            "file",
            "32e087ab-1491-49b6-aba7-43571b41ac2b",
            4,
            "Значение",
            "73",
        )
    } else {
        (
            "ExternalDataProcessor",
            "sfile",
            "a637f77f-3840-441d-a1c3-699c8c5cb7e0",
            3,
            "Результат",
            "42",
        )
    };
    let descriptor =
        fs::read_to_string(env::var_os("ESKA_DEBUG_LIVE_EPF_DESCRIPTOR").unwrap()).unwrap();
    let document = roxmltree::Document::parse(descriptor.trim_start_matches('\u{feff}')).unwrap();
    let module = Module {
        object: document
            .root_element()
            .children()
            .find(|node| node.has_tag_name(tag))
            .unwrap()
            .attribute("uuid")
            .unwrap()
            .to_ascii_lowercase(),
        property: property.into(),
        origin: ModuleOrigin::External(format!("{scheme}://{artifact}")),
    };
    let server = Server::new(&endpoint, "DefAlias".into()).unwrap();
    assert_eq!(server.attach().unwrap(), dbgs::Registration::Registered);
    let _registration = Registration(&server);
    let kinds = ["ManagedClient".to_owned(), "ServerEmulation".to_owned()];
    server.auto_attach(&kinds).unwrap();
    server
        .breakpoints([(
            &module,
            [Breakpoint {
                line,
                ..Default::default()
            }]
            .as_slice(),
        )])
        .unwrap();
    // The external driver starts its client only after this signal.
    println!("EPF_BREAKPOINT_READY");
    let (target, frames, action) = wait_for_stop(&server, Duration::from_secs(35), |target| {
        if kinds.contains(&target.kind) {
            server.attach_target(&target.id).unwrap();
        }
    });
    assert!(matches!(action, StackAction::Breakpoint));
    assert_eq!(frames[0].module.as_ref(), Some(&module));
    assert_eq!(frames[0].line, line);
    assert_eq!(
        calculate(
            &server,
            &target,
            0,
            &[Selector::Expression(expression.into())]
        )[0]
        .value
        .text,
        expected
    );
    server.breakpoints([(&module, [].as_slice())]).unwrap();
    server.step(&target, Step::Continue).unwrap();
}

fn stopped(server: &Server) -> (String, Vec<Frame>) {
    let (target, frames, _) = wait_for_stop(server, Duration::from_secs(12), |target| {
        server.break_on_next_statement(false).unwrap();
        server.attach_target(&target.id).unwrap();
    });
    (target, frames)
}

fn wait_for_stop(
    server: &Server,
    timeout: Duration,
    attach: impl Fn(dbgs::Target),
) -> (String, Vec<Frame>, StackAction) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for event in server.poll(None).unwrap() {
            match event {
                Event::Started(target) => attach(target),
                Event::Stack {
                    target,
                    frames,
                    action: action @ (StackAction::Breakpoint | StackAction::Step),
                    ..
                } => return (target, frames, action),
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(35));
    }
    panic!("dbgs did not report a stop");
}

fn calculate(server: &Server, target: &str, level: usize, path: &[Selector]) -> Vec<Calculation> {
    let id = dbgs::random_id().unwrap();
    let result = server
        .calculate(
            &dbgs::CalculationQuery {
                id: id.clone(),
                target: target.into(),
                level,
                path: path.to_vec(),
                view: dbgs::View::Context,
                page: None,
                new_expression: None,
            },
            Instant::now() + Duration::from_secs(5),
            None,
        )
        .unwrap();
    if !result.is_empty() {
        return result;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for event in server.poll(None).unwrap() {
            if let Event::Calculated(result) = event
                && result.id == id
            {
                return vec![result];
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("dbgs did not return a calculation");
}

struct Registration<'a>(&'a Server);
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        let _ = self.0.detach();
    }
}

struct Client {
    cancelled: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Client {
    fn launch(launch: DebugLaunch, endpoint: url::Url) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            let result = launch.run(&endpoint, &flag);
            if !flag.load(Ordering::Acquire) {
                eprintln!("native client result: {result:?}");
            }
        });
        Self {
            cancelled,
            worker: Some(worker),
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
