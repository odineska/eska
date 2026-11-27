//! Live stderr telemetry. The final stdout document and the operation's result have separate lifetimes.
use std::{
    io::{self, Write},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use eska_build_runtime::{
    progress::{ProgressCommand, Signal},
    runtime::apply::{Progress, Stage},
};
use eska_presentation::localization::Localizer;
use serde_json::json;

const INTERVAL: Duration = Duration::from_secs(15);

struct Timeline {
    started: Instant,
    phase_started: Instant,
    phase: &'static str,
    label: String,
    component: Option<String>,
    last_native: Option<Instant>,
    native_lines: u64,
    objects_seen: u64,
    object: Option<String>,
    pending_object: bool,
    db_changed: Option<Instant>,
    announced: bool,
    step: Option<(&'static str, String)>,
    last_report: Instant,
}

struct Output {
    json: bool,
    elapsed: String,
    quiet: String,
    waiting: String,
    lines: String,
    phase_time: String,
    objects: String,
    db_change: String,
}

impl Output {
    fn report(&self, state: &Timeline, event: &str, now: Instant) {
        let elapsed = now.duration_since(state.started);
        let phase_elapsed = now.duration_since(state.phase_started);
        let quiet = state.last_native.map(|last| now.duration_since(last));
        let db_age = state.db_changed.map(|last| now.duration_since(last));
        let mut stderr = io::stderr().lock();
        if self.json {
            let mut row = json!({"schema_version":1,"kind":"operation_progress","event":event,
                "phase":state.phase,"elapsed_ms":elapsed.as_millis(), "stage_elapsed_ms":phase_elapsed.as_millis(),
                "quiet_ms":quiet.map(|time| time.as_millis()),"native_lines":state.native_lines, "db_change_age_ms":db_age.map(|delta| delta.as_millis())});
            if state.objects_seen > 0 {
                row["objects_seen"] = json!(state.objects_seen);
            }
            if let Some(object) = &state.object {
                row["object"] = json!(object);
            }
            if let Some(component) = &state.component {
                row["component"] = json!(component);
            }
            if let Some((step, _)) = &state.step {
                row["step"] = json!(step);
            }
            let _ = writeln!(stderr, "{row}");
        } else {
            if let Some(component) = &state.component {
                let _ = write!(stderr, "[{component}] ");
            }
            let _ = write!(stderr, "{}", state.label);
            if let Some((_, step)) = &state.step {
                let _ = write!(stderr, " · {step}");
            }
            if let Some(object) = &state.object {
                let _ = write!(stderr, " {object}");
            }
            if state.objects_seen > 0 {
                let _ = write!(stderr, " · {} {}", self.objects, state.objects_seen);
            }
            if matches!(event, "heartbeat" | "activity" | "database_file_changed") {
                let _ = write!(
                    stderr,
                    " · {} {} · {} {}",
                    self.elapsed,
                    clock(elapsed),
                    self.phase_time,
                    clock(phase_elapsed)
                );
                if let Some(quiet) = quiet {
                    let _ = write!(
                        stderr,
                        " · {} {} · {} {}",
                        self.lines,
                        state.native_lines,
                        self.quiet,
                        clock(quiet)
                    );
                } else {
                    let _ = write!(stderr, " · {}", self.waiting);
                }
            }
            if let Some(age) = db_age {
                let _ = write!(stderr, " · {} {}", self.db_change, clock(age));
            }
            let _ = writeln!(stderr);
        }
    }
}

fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

pub(super) struct Reporter<'a> {
    localizer: &'a Localizer,
    result_json: bool,
    enabled: bool,
    output: Arc<Output>,
    state: Arc<Mutex<Timeline>>,
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl<'a> Reporter<'a> {
    pub(super) fn new(json: bool, phase: &'static str, localizer: &'a Localizer) -> Self {
        let now = Instant::now();
        let state = Arc::new(Mutex::new(Timeline {
            started: now,
            phase_started: now,
            phase,
            label: localizer.text(&format!("op-{}", phase.replace('_', "-"))),
            component: None,
            last_native: None,
            native_lines: 0,
            objects_seen: 0,
            object: None,
            pending_object: false,
            db_changed: None,
            announced: false,
            step: None,
            last_report: now,
        }));
        let setting = std::env::var_os("ESKA_OPERATION_PROGRESS");
        let structured = setting.as_deref().is_some_and(|value| value == "1");
        let enabled = structured || (!json && setting.as_deref().is_none_or(|value| value != "0"));
        let output = Arc::new(Output {
            json: structured,
            elapsed: localizer.text("op-elapsed"),
            quiet: localizer.text("op-quiet"),
            waiting: localizer.text("op-no-output"),
            lines: localizer.text("op-lines"),
            phase_time: localizer.text("op-phase-time"),
            db_change: localizer.text("op-db-change"),
            objects: localizer.text("op-objects"),
        });
        let (stop, wake) = mpsc::channel();
        let timeline = Arc::clone(&state);
        let sink = Arc::clone(&output);
        let worker = if enabled {
            thread::Builder::new()
                .name("eska-progress".into())
                .spawn(move || {
                    let mut next = INTERVAL;
                    while matches!(
                        wake.recv_timeout(next),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        let Ok(mut state) = timeline.lock() else {
                            break;
                        };
                        let now = Instant::now();
                        if now.duration_since(state.last_report) >= INTERVAL {
                            sink.report(&state, "heartbeat", now);
                            state.last_report = now;
                            state.pending_object = false;
                        }
                        next = INTERVAL.saturating_sub(now.duration_since(state.last_report));
                    }
                })
                .ok()
        } else {
            None
        };
        Self {
            localizer,
            result_json: json,
            enabled,
            output,
            state,
            stop,
            worker,
        }
    }

    pub(super) fn apply(&self, update: Progress, legacy_kind: &str) {
        let phase = match update.stage {
            Stage::Inspect => "inspect",
            Stage::Import => "import",
            Stage::Apply => "apply",
            Stage::Remove => "remove",
            Stage::Verify => "verify",
        };
        if update.signal.is_none()
            && self.result_json
            && matches!(update.stage, Stage::Import | Stage::Apply | Stage::Remove)
        {
            let mut row = json!({"schema_version":1, "kind":legacy_kind, "phase":phase});
            if let Some(name) = &update.component {
                row["component"] = json!(name);
            }
            let _ = writeln!(io::stderr().lock(), "{row}");
        }
        self.observe(phase, update.component.as_deref(), update.signal);
    }

    pub(super) fn observe(
        &self,
        phase: &'static str,
        component: Option<&str>,
        signal: Option<Signal>,
    ) {
        if !self.enabled {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let now = Instant::now();
        self.enter_phase(&mut state, phase, component, now);
        let Some(signal) = signal else {
            return;
        };

        let mut object = None;
        let step = match signal {
            Signal::ObjectAdded(name) => {
                object = Some(name);
                Some("object_added")
            }
            Signal::ObjectModified(name) => {
                object = Some(name);
                Some("object_modified")
            }
            Signal::ObjectRestructuring(name) => {
                object = Some(name);
                Some("object_restructuring")
            }
            Signal::CommandStarted(command) => {
                self.flush_object(&mut state, now);
                let step = command_step(command);
                state.object = None;
                state.step = Some((
                    step,
                    self.localizer
                        .text(&format!("op-{}", step.replace('_', "-"))),
                ));
                self.output.report(&state, "command", now);
                state.last_report = now;
                return;
            }
            Signal::DatabaseFileChanged => {
                let first = state.db_changed.replace(now).is_none();
                if first || now.duration_since(state.last_report) >= INTERVAL {
                    self.output.report(&state, "database_file_changed", now);
                    state.last_report = now;
                }
                return;
            }
            Signal::Activity => None,
            Signal::Reading => Some("read_configuration"),
            Signal::Importing => Some("import_configuration"),
            Signal::CheckingMetadata => Some("validate_metadata"),
            Signal::Restructuring => Some("process_database_structure"),
            Signal::GatheringInformation => Some("collect_service_information"),
            Signal::AcceptingChanges => Some("accept_changes"),
            Signal::UpdatingDatabase => Some("update_database"),
            Signal::DynamicRestructuring => Some("dynamic_restructuring_changed"),
            Signal::GenerationCreated => Some("generation_created"),
            Signal::DatabaseUpdated => Some("database_updated"),
            Signal::BuildingHelpIndex => Some("build_help_index"),
            Signal::HelpIndexBuilt => Some("help_index_built"),
        };
        let changed_step =
            step.is_some_and(|step| state.step.as_ref().is_none_or(|(old, _)| *old != step));
        // Disjoint object messages can alternate for every table on a large infobase.
        // Show the first object immediately, then sample the latest until the next native stage.
        let consecutive_objects = object.is_some() && state.object.is_some();
        if changed_step && !consecutive_objects {
            self.flush_object(&mut state, now);
        }
        let first = state.last_native.replace(now).is_none();
        state.native_lines = state.native_lines.saturating_add(1);
        if object.is_some() {
            state.objects_seen = state.objects_seen.saturating_add(1);
            state.pending_object = true;
        }
        if let Some(step) = step {
            state.object = object;
            if changed_step {
                state.step = Some((
                    step,
                    self.localizer
                        .text(&format!("op-{}", step.replace('_', "-"))),
                ));
            }
        }
        if (changed_step && !consecutive_objects)
            || (state.pending_object && now.duration_since(state.last_report) >= INTERVAL)
        {
            self.output.report(&state, "native", now);
            state.pending_object = false;
        } else if first || now.duration_since(state.last_report) >= INTERVAL {
            self.output.report(&state, "activity", now);
        } else {
            return;
        }
        state.last_report = now;
    }

    fn enter_phase(
        &self,
        state: &mut Timeline,
        phase: &'static str,
        component: Option<&str>,
        now: Instant,
    ) {
        if !state.announced || state.phase != phase || state.component.as_deref() != component {
            self.flush_object(state, now);
            state.announced = true;
            state.phase = phase;
            state.phase_started = now;
            state.component = component.map(str::to_owned);
            state.label = self
                .localizer
                .text(&format!("op-{}", phase.replace('_', "-")));
            state.step = None;
            state.native_lines = 0;
            state.db_changed = None;
            state.object = None;
            state.objects_seen = 0;
            state.last_native = None;
            self.output.report(state, "stage", now);
            state.last_report = now;
        }
    }

    fn flush_object(&self, state: &mut Timeline, now: Instant) {
        if state.pending_object {
            self.output.report(state, "native", now);
            state.pending_object = false;
        }
    }

    pub(super) fn diagnostic<'b>(&self, line: &'b [u8]) -> std::borrow::Cow<'b, [u8]> {
        if self.output.json && line.starts_with(b"{") {
            [b"[1C] ".as_slice(), line].concat().into()
        } else {
            line.into()
        }
    }
}

impl Drop for Reporter<'_> {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut state) = self.state.lock() {
            self.flush_object(&mut state, Instant::now());
        }
    }
}

const fn command_step(command: ProgressCommand) -> &'static str {
    match command {
        ProgressCommand::ReadGeneration => "read_generation",
        ProgressCommand::ChecksumMain => "checksum_main",
        ProgressCommand::ChecksumDatabase => "checksum_database",
        ProgressCommand::ListExtensions => "list_extensions",
        ProgressCommand::ExportConfiguration => "export_configuration",
        ProgressCommand::SaveConfiguration => "save_configuration",
        ProgressCommand::ReadExtensionInfo => "read_extension_info",
    }
}
