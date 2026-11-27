//! Optional machine progress and localized human progress never alter the report stream.
use eska_analyzer::check::Progress;
use eska_presentation::{arguments::check::Format, localization::Localizer};
use serde_json::json;
use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

pub struct Reporter {
    enabled: bool,
    json: bool,
    labels: [String; 4],
    started: Instant,
    stage_started: Instant,
    phase: Option<&'static str>,
    last_counter: Option<Instant>,
}

impl Reporter {
    pub fn new(format: Format, localizer: &Localizer) -> Self {
        let json = matches!(format, Format::Json);
        let setting = std::env::var_os("ESKA_OPERATION_PROGRESS");
        let enabled = setting.as_deref().map_or(!json, |value| value == "1");
        let now = Instant::now();
        Self {
            enabled,
            json,
            labels: [
                "check-progress-structure",
                "check-progress-prepare",
                "check-progress-analyze",
                "check-progress-modules",
            ]
            .map(|key| localizer.text(key)),
            started: now,
            stage_started: now,
            phase: None,
            last_counter: None,
        }
    }

    pub fn prepare_analyzer(&mut self) {
        self.emit("prepare_analyzer", None);
    }

    pub fn observe(&mut self, progress: Progress) {
        match progress {
            Progress::CheckStructure => self.emit("check_structure", None),
            Progress::AnalyzeModules {
                processed_modules,
                total_modules,
            } => {
                let now = Instant::now();
                if processed_modules != 0
                    && processed_modules != 1
                    && processed_modules != total_modules
                    && self
                        .last_counter
                        .is_some_and(|last| now.duration_since(last) < Duration::from_millis(500))
                {
                    return;
                }
                self.last_counter = Some(now);
                self.emit("analyze_modules", Some((processed_modules, total_modules)));
            }
        }
    }

    fn emit(&mut self, phase: &'static str, modules: Option<(usize, usize)>) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        let changed = self.phase != Some(phase);
        if changed {
            self.phase = Some(phase);
            self.stage_started = now;
        }
        let mut stderr = io::stderr().lock();
        if self.json {
            let mut row = json!({"schema_version":1,"kind":"operation_progress","event":"stage","phase":phase,
                "elapsed_ms":now.duration_since(self.started).as_millis(),"stage_elapsed_ms":now.duration_since(self.stage_started).as_millis()});
            if let Some((processed, total)) = modules {
                row["processed_modules"] = json!(processed);
                row["total_modules"] = json!(total);
            }
            let _ = writeln!(stderr, "{row}");
        } else {
            if changed {
                let label = match phase {
                    "check_structure" => &self.labels[0],
                    "prepare_analyzer" => &self.labels[1],
                    _ => &self.labels[2],
                };
                let _ = writeln!(stderr, "{label}");
            }
            if let Some((processed, total)) = modules
                && processed > 0
            {
                let _ = writeln!(stderr, "{} {processed}/{total}", self.labels[3]);
            }
        }
    }
}
