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

use crate::dbgs::{self, Calculation, Event, Frame, Module, Selector, Server, Step};

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
        extension: String::new(),
    };
    let launch = DebugLaunch::prepare(project, ProfileName::Test).unwrap();
    let endpoint = url::Url::parse(&launch.profile().debug.as_ref().unwrap().url).unwrap();
    let server = Server::new(&endpoint, "DefAlias".into()).unwrap();
    server.attach().unwrap();
    let _registration = Registration(&server);
    server.auto_attach().unwrap();
    server.breakpoints([(&module, [3].as_slice())]).unwrap();
    let worker = Client::launch(launch);
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

fn stopped(server: &Server) -> (String, Vec<Frame>) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        for event in server.poll().unwrap() {
            match event {
                Event::Started(target) => server.attach_target(&target.id).unwrap(),
                Event::Stopped { target, frames, .. } => return (target, frames),
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(35));
    }
    panic!("dbgs did not report a stop");
}

fn calculate(server: &Server, target: &str, level: usize, path: &[Selector]) -> Vec<Calculation> {
    let id = dbgs::random_id().unwrap();
    let result = server.calculate(target, level, &id, path, false).unwrap();
    if !result.is_empty() {
        return result;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for event in server.poll().unwrap() {
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
    fn launch(launch: DebugLaunch) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            let result = launch.run(&flag);
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
