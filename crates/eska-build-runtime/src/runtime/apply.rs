//! Plan and apply Designer XML changes to an existing test infobase.

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use eska_project_core::project::{
    Project, ProjectType,
    local_settings::{Action, Infobase, Profile, ProfileName},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{RuntimeError, file_infobase, platform, profile};
use crate::{
    build::{Ibcmd, RunError, SourceSnapshotError, source_snapshot},
    platform_path::native_path,
};

type Catalog = BTreeMap<PathBuf, Vec<u8>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Initialize,
    Incremental,
    Unchanged,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Modified,
    Removed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Inspect,
    Import,
    Apply,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub mode: Mode,
    pub target: String,
    pub platform_version: String,
    pub changes: BTreeMap<PathBuf, Change>,
    pub confirmation: Option<String>,
}

#[derive(Debug)]
pub enum ApplyError {
    Runtime(RuntimeError),
    Io(io::Error),
    Snapshot(SourceSnapshotError),
    InvalidState,
    ProjectType,
    SourceInsideInfobase,
    BaselineRequired,
    BaselineChanged,
    Incomplete,
    Busy,
    StalePreview,
    UnrepresentablePath,
    PlatformFailed(Stage),
    Interrupted,
}

#[derive(Deserialize, Serialize)]
struct Baseline {
    generation: String,
    files: Catalog,
}

/// Inspect the applied generation and prepare a reviewable plan for saved sources.
/// # Errors
/// Returns a structured preflight error when the target or last successful baseline cannot be verified.
pub fn preview(project: &Project, name: ProfileName, initialize: bool) -> Result<Plan, ApplyError> {
    let target = Target::open(project, name)?;
    let generation = target.generation()?;
    let current = target.capture(project, None)?;
    plan(&target, generation, &current, initialize)
}

/// Revalidate the reviewed plan under the infobase lock, then apply its exact source snapshot.
/// # Errors
/// A stale preview or any failed phase leaves the last successful baseline intact.
pub fn execute(
    project: &Project,
    name: ProfileName,
    initialize: bool,
    confirmation: &str,
    mut progress: impl FnMut(Stage),
) -> Result<Plan, ApplyError> {
    let target = Target::open(project, name)?;
    let _lock = lock(&target.lock)?;
    let generation = target.generation()?;
    let source = target.scratch.path().join("source");
    let current = target.capture(project, Some(&source))?;
    let plan = plan(&target, generation, &current, initialize)?;
    if plan.confirmation.as_deref() != Some(confirmation) {
        return Err(ApplyError::StalePreview);
    }
    let state = target.state.with_extension("pending");
    let directory = target.state.parent().ok_or(ApplyError::InvalidState)?;
    let mut marker = tempfile::NamedTempFile::new_in(directory).map_err(ApplyError::Io)?;
    marker
        .write_all(confirmation.as_bytes())
        .map_err(ApplyError::Io)?;
    marker.as_file().sync_all().map_err(ApplyError::Io)?;
    marker
        .persist(&state)
        .map_err(|error| ApplyError::Io(error.error))?;

    progress(Stage::Import);
    match plan.mode {
        Mode::Initialize => {
            target.command(
                &["import"],
                &[native_path(&source).as_os_str().to_owned()],
                None,
                None,
                Stage::Import,
            )?;
        }
        Mode::Incremental => {
            let mut input = tempfile::tempfile().map_err(ApplyError::Io)?;
            for path in plan.changes.keys() {
                let name = input_path(path).ok_or(ApplyError::UnrepresentablePath)?;
                writeln!(input, "{name}").map_err(ApplyError::Io)?;
            }
            input.rewind().map_err(ApplyError::Io)?;
            target.command(
                &["import", "files"],
                &[option("--base-dir", &source)],
                Some(&source),
                Some(input),
                Stage::Import,
            )?;
        }
        Mode::Unchanged | Mode::Unsupported => return Err(ApplyError::StalePreview),
    }
    progress(Stage::Apply);
    target.command(&["apply"], &[], None, None, Stage::Apply)?;
    let generation = target.applied_generation()?;
    let document = Baseline {
        generation,
        files: current,
    };
    let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(ApplyError::Io)?;
    serde_json::to_writer(temp.as_file_mut(), &document).map_err(|_| ApplyError::InvalidState)?;
    temp.as_file().sync_all().map_err(ApplyError::Io)?;
    temp.persist(&target.state)
        .map_err(|error| ApplyError::Io(error.error))?;
    fs::remove_file(state).map_err(ApplyError::Io)?;
    Ok(plan)
}

struct Target {
    platform: Ibcmd,
    connection: Connection,
    identity: String,
    lock: PathBuf,
    state: PathBuf,
    scratch: tempfile::TempDir,
    authentication: Vec<OsString>,
}

#[derive(Serialize)]
enum Connection {
    File(PathBuf),
    Standalone(u32),
}

impl Target {
    fn open(project: &Project, name: ProfileName) -> Result<Self, ApplyError> {
        let profile = profile(project, name, Action::Apply).map_err(ApplyError::Runtime)?;
        if project.configuration().project_type() != ProjectType::Configuration {
            return Err(ApplyError::ProjectType);
        }
        let directory = project.root().join(".eska");
        let (connection, identity, lock) = match &profile.infobase {
            Infobase::File { path } => {
                let base = file_infobase(project.root(), path).map_err(ApplyError::Runtime)?;
                let source = project.source().canonicalize().map_err(ApplyError::Io)?;
                if source.starts_with(&base) {
                    return Err(ApplyError::SourceInsideInfobase);
                }
                let identity = base
                    .to_str()
                    .ok_or(ApplyError::UnrepresentablePath)?
                    .to_owned();
                let lock = base.join(".eska-apply.lock");
                (Connection::File(base), identity, lock)
            }
            Infobase::Standalone {
                url,
                admin_pid: Some(pid),
                ..
            } => {
                // Projects sharing a local server must also share its apply lock.
                let lock = env::temp_dir().join(format!("eska-apply-standalone-{pid}.lock"));
                (Connection::Standalone(*pid), url.clone(), lock)
            }
            _ => return Err(ApplyError::InvalidState),
        };
        let state = directory.join(format!("apply-{}.json", checksum(identity.as_bytes())));
        let scratch = tempfile::Builder::new()
            .prefix("apply-")
            .tempdir_in(directory)
            .map_err(ApplyError::Io)?;
        let platform = platform(project, &profile).map_err(ApplyError::Runtime)?;
        platform
            .begin_interruptible_operation()
            .map_err(process_error)?;
        Ok(Self {
            platform,
            connection,
            identity,
            lock,
            state,
            scratch,
            authentication: authentication(&profile)?,
        })
    }

    fn capture(&self, project: &Project, target: Option<&Path>) -> Result<Catalog, ApplyError> {
        let mut ignored = vec![
            project.root().join(".eska"),
            project.root().join(".gitignore"),
            project.root().join("eska.toml"),
            project.root().join("bsl-analyzer.toml"),
            project.source().join("ConfigDumpInfo.xml"),
        ];
        if let Connection::File(base) = &self.connection {
            ignored.push(base.clone());
        }
        let excluded: Vec<_> = ignored.iter().map(PathBuf::as_path).collect();
        source_snapshot::catalog(project.source(), target, &excluded).map_err(ApplyError::Snapshot)
    }

    fn applied_generation(&self) -> Result<String, ApplyError> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match self.generation() {
                Err(ApplyError::PlatformFailed(Stage::Inspect))
                    if matches!(self.connection, Connection::Standalone(_))
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(250));
                }
                result => return result,
            }
        }
    }

    fn generation(&self) -> Result<String, ApplyError> {
        let output = self.command(&["generation-id"], &[], None, None, Stage::Inspect)?;
        let id = std::str::from_utf8(&output)
            .map_err(|_| ApplyError::InvalidState)?
            .trim();
        if id.len() != 40 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApplyError::InvalidState);
        }
        Ok(id.to_ascii_lowercase())
    }

    fn command(
        &self,
        command: &[&str],
        extra: &[OsString],
        cwd: Option<&Path>,
        input: Option<File>,
        stage: Stage,
    ) -> Result<Vec<u8>, ApplyError> {
        let mut arguments = Vec::new();
        if let Connection::Standalone(pid) = self.connection {
            arguments.push(format!("--pid={pid}").into());
        }
        arguments.extend(["infobase".into(), "config".into()]);
        arguments.extend(command.iter().map(OsString::from));
        if let Connection::File(base) = &self.connection {
            arguments.extend([
                option("--data", &self.scratch.path().join("platform")),
                option("--db-path", base),
            ]);
        }
        arguments.extend_from_slice(&self.authentication);
        arguments.extend_from_slice(extra);
        let output = self
            .platform
            .run_owned(
                arguments,
                &self.scratch.path().join("process.pid"),
                cwd,
                input,
                &mut |_, _| {},
                None,
            )
            .map_err(process_error)?;
        if self.platform.was_interrupted() {
            return Err(ApplyError::Interrupted);
        }
        if !output.status.success() {
            return Err(ApplyError::PlatformFailed(stage));
        }
        Ok(output.stdout)
    }
}

fn process_error(error: RunError) -> ApplyError {
    match error {
        RunError::Interrupted => ApplyError::Interrupted,
        other => ApplyError::Runtime(RuntimeError::Run(other)),
    }
}

fn plan(
    target: &Target,
    generation: String,
    current: &Catalog,
    initialize: bool,
) -> Result<Plan, ApplyError> {
    if !initialize && target.state.with_extension("pending").exists() {
        return Err(ApplyError::Incomplete);
    }
    let baseline = match fs::read(&target.state) {
        Ok(bytes) => match serde_json::from_slice::<Baseline>(&bytes) {
            Ok(baseline) => Some(baseline),
            Err(_) if initialize => None,
            Err(_) => return Err(ApplyError::InvalidState),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(ApplyError::Io(error)),
    };
    if !initialize {
        let saved = baseline.as_ref().ok_or(ApplyError::BaselineRequired)?;
        if saved.generation != generation {
            return Err(ApplyError::BaselineChanged);
        }
    }
    let empty = Catalog::new();
    let previous = baseline.as_ref().map_or(&empty, |saved| &saved.files);
    let changes = changes(previous, current);
    let mode = if initialize {
        Mode::Initialize
    } else if changes.is_empty() {
        Mode::Unchanged
    } else if changes.iter().all(|(path, kind)| {
        *kind == Change::Modified
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bsl"))
            && input_path(path).is_some()
    }) {
        Mode::Incremental
    } else {
        Mode::Unsupported
    };
    let confirmation = if matches!(mode, Mode::Initialize | Mode::Incremental) {
        let identity = (
            &target.connection,
            &target.identity,
            target.platform.version().as_str(),
            initialize,
            generation,
            previous,
            current,
        );
        Some(checksum(
            &serde_json::to_vec(&identity).map_err(|_| ApplyError::UnrepresentablePath)?,
        ))
    } else {
        None
    };
    Ok(Plan {
        mode,
        target: target.identity.clone(),
        platform_version: target.platform.version().as_str().to_owned(),
        changes,
        confirmation,
    })
}

fn changes(previous: &Catalog, current: &Catalog) -> BTreeMap<PathBuf, Change> {
    let mut changes = BTreeMap::new();
    for (path, digest) in current {
        match previous.get(path) {
            None => {
                changes.insert(path.clone(), Change::Added);
            }
            Some(old) if old != digest => {
                changes.insert(path.clone(), Change::Modified);
            }
            Some(_) => {}
        }
    }
    for path in previous.keys().filter(|path| !current.contains_key(*path)) {
        changes.insert(path.clone(), Change::Removed);
    }
    changes
}

fn lock(path: &Path) -> Result<File, ApplyError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(ApplyError::InvalidState),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(ApplyError::Io(error)),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(ApplyError::Io)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(ApplyError::Busy),
        Err(fs::TryLockError::Error(error)) => Err(ApplyError::Io(error)),
    }
}

fn authentication(profile: &Profile) -> Result<Vec<OsString>, ApplyError> {
    let mut arguments = Vec::new();
    if let Some(auth) = &profile.authentication {
        arguments.push(format!("--user={}", auth.user).into());
        if let Some(key) = &auth.password_env {
            let mut argument = OsString::from("--password=");
            argument.push(
                env::var_os(key).ok_or(ApplyError::Runtime(RuntimeError::SecretUnavailable))?,
            );
            arguments.push(argument);
        }
    }
    Ok(arguments)
}

fn option(name: &str, path: &Path) -> OsString {
    let mut result = OsString::from(name);
    result.push("=");
    result.push(native_path(path).as_os_str());
    result
}

fn input_path(path: &Path) -> Option<String> {
    let name = path.to_str()?;
    (!name.chars().any(char::is_whitespace)).then(|| name.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn checksum(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_delta_includes_removed_and_added_paths() {
        let previous = Catalog::from([
            ("Ext/Old.bsl".into(), vec![1]),
            ("Ext/Changed.bsl".into(), vec![2]),
            ("Configuration.xml".into(), vec![3]),
        ]);
        let current = Catalog::from([
            ("Ext/New.bsl".into(), vec![1]),
            ("Ext/Changed.bsl".into(), vec![4]),
            ("Configuration.xml".into(), vec![3]),
        ]);
        assert_eq!(
            changes(&previous, &current),
            BTreeMap::from([
                ("Ext/Old.bsl".into(), Change::Removed),
                ("Ext/New.bsl".into(), Change::Added),
                ("Ext/Changed.bsl".into(), Change::Modified),
            ])
        );
        assert_eq!(
            input_path(Path::new("Ext/Модуль.bsl")).as_deref(),
            Some("Ext/Модуль.bsl")
        );
        assert_eq!(input_path(Path::new("Ext/line\nBreak.bsl")), None);
    }
}
