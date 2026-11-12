//! Plan and apply Designer XML changes to an existing test infobase.

pub mod application;
mod descriptor;
mod extension;
mod picture;
mod structure;

pub use extension::Identity as ExtensionIdentity;
pub use structure::{ObjectChange, ObjectKind, ObjectOperation};

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use super::{RuntimeError, checksum, launch_fingerprint, platform, profile};
use crate::{
    build::{Ibcmd, RunError, SourceSnapshotError, ibcmd_generation, source_snapshot},
    platform_path::native_path,
};
use eska_project_core::project::{
    Project, ProjectType,
    local_settings::{Action, Infobase, Profile, ProfileName, SettingsError},
};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Unknown,
    Unchanged,
    Changed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    UpToDate,
    LocalChanges,
    DatabaseChanged,
    BothChanged,
    UnknownBaseline,
    DatabaseUnavailable,
    Incomplete,
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub status: Status,
    pub sources: Comparison,
    pub database: Comparison,
    pub baseline_generation: Option<String>,
    pub database_generation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_generation: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dependency_generations: BTreeMap<String, String>,
    pub pending_apply: bool,
}

#[derive(Debug, Serialize)]
pub struct Review {
    pub inspection: Inspection,
    pub changes: Option<BTreeMap<PathBuf, Change>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewReason {
    BaselineRequired,
    BaselineChanged,
    InvalidBaseline,
    Incomplete,
    GenerationUnavailable,
    InvalidGeneration,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub mode: Mode,
    pub target: String,
    pub platform_version: String,
    pub launch: LaunchTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<ExtensionIdentity>,
    pub changes: BTreeMap<PathBuf, Change>,
    pub import_files: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub object_changes: Vec<ObjectChange>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub picture_references: Vec<PathBuf>,
    pub confirmation: Option<String>,
    pub inspection: Inspection,
    #[serde(skip)]
    descriptors: descriptor::Shapes,
    #[serde(skip)]
    root: Option<structure::Root>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LaunchTarget {
    pub project_dir: String,
    pub fingerprint: String,
}

#[derive(Debug)]
pub enum ApplyError {
    Runtime(RuntimeError),
    Io(io::Error),
    Snapshot(SourceSnapshotError),
    InvalidState,
    ProjectType,
    ExtensionApplication,
    ExtensionScope,
    ExtensionDescriptor,
    ExtensionAmbiguous,
    ExtensionUnavailable,
    ExtensionIdentityMismatch,
    ApplicationContext,
    SourceInsideInfobase,
    ReviewRequired(ReviewReason, Box<Review>),
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
    #[serde(default)]
    descriptors: descriptor::Shapes,
    #[serde(default)]
    root: Option<structure::Root>,
}

/// Inspect the applied generation and prepare a reviewable plan for saved sources.
/// # Errors
/// Returns a structured preflight error when the target or last successful baseline cannot be verified.
pub fn preview(project: &Project, name: ProfileName, initialize: bool) -> Result<Plan, ApplyError> {
    let target = Target::open(project, name)?;
    let generation = match target.observe() {
        Ok(generation) => Ok(generation),
        Err(ApplyError::PlatformFailed(Stage::Inspect)) => Err(ReviewReason::GenerationUnavailable),
        Err(ApplyError::InvalidState) => Err(ReviewReason::InvalidGeneration),
        Err(error) => return Err(error),
    };
    let current = target.capture(project, None)?;
    target.verify_source(project.source())?;
    plan(&target, generation, project.source(), &current, initialize)
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
    let StagedInput {
        source, current, ..
    } = target.stage_source(project, initialize)?;
    target.verify_source(&source)?;
    let generation = target.observe()?;
    let mut plan = plan(&target, Ok(generation), &source, &current, initialize)?;
    if plan.confirmation.as_deref() != Some(confirmation) {
        return Err(ApplyError::StalePreview);
    }
    write_plan(&target, &source, current, &mut plan, &mut progress)?;
    Ok(plan)
}

fn write_plan(
    target: &Target,
    source: &Path,
    current: Catalog,
    plan: &mut Plan,
    progress: &mut impl FnMut(Stage),
) -> Result<String, ApplyError> {
    let confirmation = plan
        .confirmation
        .as_deref()
        .ok_or(ApplyError::StalePreview)?;
    let descriptors = if plan.mode == Mode::Initialize {
        descriptor::baseline(source, target.project_type(), &current)
    } else {
        std::mem::take(&mut plan.descriptors)
    };
    let root = plan
        .root
        .take()
        .or_else(|| structure::Root::read(source, target.project_type()));
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
                &[native_path(source).as_os_str().to_owned()],
                None,
                None,
                Stage::Import,
            )?;
        }
        Mode::Incremental => {
            let mut input = tempfile::tempfile().map_err(ApplyError::Io)?;
            for path in plan
                .import_files
                .as_deref()
                .ok_or(ApplyError::StalePreview)?
            {
                let name = input_path(path).ok_or(ApplyError::UnrepresentablePath)?;
                writeln!(input, "{name}").map_err(ApplyError::Io)?;
            }
            input.rewind().map_err(ApplyError::Io)?;
            target.command(
                &["import", "files"],
                &[option("--base-dir", source)],
                Some(source),
                Some(input),
                Stage::Import,
            )?;
        }
        Mode::Unchanged | Mode::Unsupported => return Err(ApplyError::StalePreview),
    }
    progress(Stage::Apply);
    let applied = target.command(&["apply"], &[], None, None, Stage::Apply)?;
    let generation = target.applied_generation(&applied)?;
    let document = Baseline {
        generation: generation.clone(),
        files: current,
        descriptors,
        root,
    };
    let mut temp = tempfile::NamedTempFile::new_in(directory).map_err(ApplyError::Io)?;
    serde_json::to_writer(temp.as_file_mut(), &document).map_err(|_| ApplyError::InvalidState)?;
    temp.as_file().sync_all().map_err(ApplyError::Io)?;
    temp.persist(&target.state)
        .map_err(|error| ApplyError::Io(error.error))?;
    fs::remove_file(state).map_err(ApplyError::Io)?;
    Ok(generation)
}

struct Target {
    platform: Ibcmd,
    connection: Connection,
    identity: String,
    launch: LaunchTarget,
    extension: Option<ExtensionIdentity>,
    dependencies: Vec<ExtensionIdentity>,
    lock: PathBuf,
    state: PathBuf,
    scratch: tempfile::TempDir,
    authentication: Vec<OsString>,
}

struct StagedInput {
    source: PathBuf,
    current: Catalog,
    complete: bool,
}

#[derive(Clone, Serialize)]
enum Connection {
    File(PathBuf),
    Standalone(u32),
}

#[derive(Clone, Eq, PartialEq)]
struct Observation {
    generation: String,
    configuration_generation: Option<String>,
    dependency_generations: BTreeMap<String, String>,
}

impl Target {
    const fn project_type(&self) -> ProjectType {
        if self.extension.is_some() {
            ProjectType::Extension
        } else {
            ProjectType::Configuration
        }
    }

    fn open(project: &Project, name: ProfileName) -> Result<Self, ApplyError> {
        if name == ProfileName::Prod {
            return Err(ApplyError::Runtime(RuntimeError::Settings(
                SettingsError::ForbiddenAction,
            )));
        }
        let selected = match project.configuration().project_type() {
            ProjectType::Configuration => None,
            ProjectType::Extension => Some(extension::resolve(project)?),
            _ => return Err(ApplyError::ProjectType),
        };
        let profile_project = selected
            .as_ref()
            .map_or(project, |selection| &selection.base);
        let mut profile =
            profile(profile_project, name, Action::Apply).map_err(ApplyError::Runtime)?;
        let launch = LaunchTarget {
            project_dir: native_path(profile_project.root())
                .to_str()
                .ok_or(ApplyError::UnrepresentablePath)?
                .to_owned(),
            fingerprint: launch_fingerprint(profile_project.root(), &mut profile)
                .map_err(ApplyError::Runtime)?,
        };
        let directory = profile_project.root().join(".eska");
        let (connection, identity, lock) = match &profile.infobase {
            Infobase::File { path } => {
                let base = path.clone();
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
        let extension = selected.as_ref().map(|selection| &selection.identity);
        let state = state_path(&directory, &identity, extension);
        let scratch = scratch(&directory)?;
        let platform = platform(profile_project, &profile).map_err(ApplyError::Runtime)?;
        platform
            .begin_interruptible_operation()
            .map_err(process_error)?;
        let (extension, dependencies) = selected.map_or_else(
            || (None, Vec::new()),
            |selection| (Some(selection.identity), selection.dependencies),
        );
        Ok(Self {
            platform,
            connection,
            identity,
            launch,
            extension,
            dependencies,
            lock,
            state,
            scratch,
            authentication: authentication(&profile)?,
        })
    }

    fn for_extension(
        &self,
        project: &Project,
        extension: ExtensionIdentity,
        dependencies: Vec<ExtensionIdentity>,
    ) -> Result<Self, ApplyError> {
        if let Connection::File(base) = &self.connection
            && project
                .source()
                .canonicalize()
                .map_err(ApplyError::Io)?
                .starts_with(base)
        {
            return Err(ApplyError::SourceInsideInfobase);
        }
        let directory = self.state.parent().ok_or(ApplyError::InvalidState)?;
        Ok(Self {
            platform: self.platform.clone(),
            connection: self.connection.clone(),
            identity: self.identity.clone(),
            launch: self.launch.clone(),
            lock: self.lock.clone(),
            state: state_path(directory, &self.identity, Some(&extension)),
            scratch: scratch(directory)?,
            extension: Some(extension),
            dependencies,
            authentication: self.authentication.clone(),
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

    fn stage_source(&self, project: &Project, initialize: bool) -> Result<StagedInput, ApplyError> {
        if !initialize && let Ok(baseline) = read_baseline(&self.state)? {
            let current = self.capture(project, None)?;
            let delta = changes(&baseline.files, &current);
            if delta.iter().all(|(path, change)| {
                *change == Change::Modified && descriptor::has_extension(path, "bsl")
            }) {
                let source = self.scratch.path().join("modules");
                let root = Path::new(structure::ROOT);
                // The extension identity and legacy structural baseline still need
                // the verified root descriptor. Only the listed modules are imported.
                let root = if self.extension.is_some() || baseline.root.is_none() {
                    current.get_key_value(root)
                } else {
                    None
                };
                let selected = delta
                    .keys()
                    .filter_map(|path| current.get_key_value(path))
                    .chain(root)
                    .map(|(path, digest)| (path.as_path(), digest.as_slice()));
                if !source_snapshot::copy_selected(project.source(), &source, selected)
                    .map_err(ApplyError::Snapshot)?
                {
                    return Err(ApplyError::StalePreview);
                }
                return Ok(StagedInput {
                    source,
                    current,
                    complete: false,
                });
            }
        }
        self.stage_full_source(project)
    }

    fn stage_full_source(&self, project: &Project) -> Result<StagedInput, ApplyError> {
        let source = self.scratch.path().join("source");
        let current = self.capture(project, Some(&source))?;
        Ok(StagedInput {
            source,
            current,
            complete: true,
        })
    }

    fn applied_generation(&self, output: &[u8]) -> Result<String, ApplyError> {
        if matches!(self.connection, Connection::File(_))
            && let Some(generation) =
                ibcmd_generation::from_apply_output(output, self.extension.is_some())
        {
            return Ok(generation);
        }
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
        self.generation_for(self.extension.as_ref())
    }

    fn verified_generation(&self) -> Result<String, ApplyError> {
        if let Some(extension) = &self.extension {
            self.verify_installed(extension)?;
        }
        self.generation()
    }

    fn generation_for(&self, extension: Option<&ExtensionIdentity>) -> Result<String, ApplyError> {
        let output = self.command_for(
            extension,
            &["generation-id"],
            &[],
            None,
            None,
            Stage::Inspect,
        )?;
        let id = std::str::from_utf8(&output).map_err(|_| ApplyError::InvalidState)?;
        ibcmd_generation::parse_id(id).ok_or(ApplyError::InvalidState)
    }

    fn verify_source(&self, source: &Path) -> Result<(), ApplyError> {
        if let Some(expected) = &self.extension
            && extension::identity(source).as_ref() != Some(expected)
        {
            return Err(ApplyError::StalePreview);
        }
        Ok(())
    }

    fn observe(&self) -> Result<Observation, ApplyError> {
        let generation = self.verified_generation()?;
        let configuration_generation = if self.extension.is_some() {
            Some(self.generation_for(None)?)
        } else {
            None
        };
        let mut dependency_generations = BTreeMap::new();
        for dependency in &self.dependencies {
            self.verify_installed(dependency)?;
            dependency_generations.insert(
                dependency.name.clone(),
                self.generation_for(Some(dependency))?,
            );
        }
        Ok(Observation {
            generation,
            configuration_generation,
            dependency_generations,
        })
    }

    fn verify_installed(&self, extension: &ExtensionIdentity) -> Result<(), ApplyError> {
        let output = tempfile::Builder::new()
            .prefix("extension-")
            .tempdir_in(self.scratch.path())
            .map_err(ApplyError::Io)?;
        let directory = output.path();
        // A PID connection reports success for export info without writing its XML.
        // Export only the configuration object to obtain its actual name and UUID.
        let standalone = matches!(self.connection, Connection::Standalone(_));
        let command: &[&str] = if standalone {
            &["export", "objects", "Configuration"]
        } else {
            &["export", "info"]
        };
        self.command_for(
            Some(extension),
            command,
            &[option("--out", directory)],
            None,
            None,
            Stage::Inspect,
        )
        .map_err(|error| match error {
            ApplyError::PlatformFailed(Stage::Inspect) => ApplyError::ExtensionUnavailable,
            error => error,
        })?;
        let matches = if standalone {
            extension::identity(directory).as_ref() == Some(extension)
        } else {
            extension::installed_uuid(directory, &extension.name).as_ref() == Some(&extension.uuid)
        };
        if !matches {
            return Err(ApplyError::ExtensionIdentityMismatch);
        }
        Ok(())
    }

    fn command(
        &self,
        command: &[&str],
        extra: &[OsString],
        cwd: Option<&Path>,
        input: Option<File>,
        stage: Stage,
    ) -> Result<Vec<u8>, ApplyError> {
        self.command_for(self.extension.as_ref(), command, extra, cwd, input, stage)
    }

    fn command_for(
        &self,
        extension: Option<&ExtensionIdentity>,
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
        if let Some(extension) = extension {
            arguments.push(format!("--extension={}", extension.name).into());
        }
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

fn scratch(directory: &Path) -> Result<tempfile::TempDir, ApplyError> {
    tempfile::Builder::new()
        .prefix("apply-")
        .tempdir_in(directory)
        .map_err(ApplyError::Io)
}

fn state_path(directory: &Path, identity: &str, extension: Option<&ExtensionIdentity>) -> PathBuf {
    let state_id = extension.map_or_else(
        || checksum(&[identity.as_bytes()]),
        |extension| {
            checksum(&[
                b"extension\0",
                identity.as_bytes(),
                b"\0",
                extension.uuid.as_bytes(),
            ])
        },
    );
    directory.join(format!("apply-{state_id}.json"))
}

fn process_error(error: RunError) -> ApplyError {
    match error {
        RunError::Interrupted => ApplyError::Interrupted,
        other => ApplyError::Runtime(RuntimeError::Run(other)),
    }
}

fn plan(
    target: &Target,
    observation: Result<Observation, ReviewReason>,
    source: &Path,
    current: &Catalog,
    initialize: bool,
) -> Result<Plan, ApplyError> {
    let pending = match fs::read(target.state.with_extension("pending")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(ApplyError::Io(error)),
    };
    let baseline = read_baseline(&target.state)?;
    let delta = baseline
        .as_ref()
        .ok()
        .map(|saved| changes(&saved.files, current));
    if delta
        .as_ref()
        .is_some_and(|changes| changes.keys().any(|path| path.to_str().is_none()))
    {
        return Err(ApplyError::UnrepresentablePath);
    }
    let generation_refusal = observation.as_ref().err().copied();
    let inspection = Inspection::new(
        delta.as_ref(),
        baseline
            .as_ref()
            .ok()
            .map(|saved| saved.generation.as_str()),
        observation.ok(),
        pending.is_some(),
    );
    let refusal = generation_refusal.or_else(|| {
        if initialize {
            None
        } else if inspection.pending_apply {
            Some(ReviewReason::Incomplete)
        } else if inspection.database == Comparison::Changed {
            Some(ReviewReason::BaselineChanged)
        } else {
            baseline.as_ref().err().copied()
        }
    });
    if let Some(reason) = refusal {
        return Err(ApplyError::ReviewRequired(
            reason,
            Box::new(Review {
                inspection,
                changes: delta,
            }),
        ));
    }
    let changes = delta.unwrap_or_else(|| changes(&Catalog::new(), current));
    assemble_plan(
        target,
        source,
        current,
        initialize,
        Reviewed {
            baseline: baseline.ok(),
            pending,
            changes,
            inspection,
        },
    )
}

struct Reviewed {
    baseline: Option<Baseline>,
    pending: Option<Vec<u8>>,
    changes: BTreeMap<PathBuf, Change>,
    inspection: Inspection,
}

fn assemble_plan(
    target: &Target,
    source: &Path,
    current: &Catalog,
    initialize: bool,
    reviewed: Reviewed,
) -> Result<Plan, ApplyError> {
    let Reviewed {
        baseline,
        pending,
        changes,
        inspection,
    } = reviewed;
    let empty = Catalog::new();
    let previous = baseline.as_ref().map_or(&empty, |saved| &saved.files);
    let structural = baseline.as_ref().filter(|_| !initialize).and_then(|saved| {
        structure::select(
            source,
            target.project_type(),
            &changes,
            &saved.files,
            current,
            saved.root.as_ref()?,
            &saved.descriptors,
        )
    });
    let picture_references = structural.as_ref().map_or_else(Vec::new, |selection| {
        picture::reference_files(source, current, &selection.objects)
    });
    if picture_references
        .iter()
        .any(|path| path.to_str().is_none())
    {
        return Err(ApplyError::UnrepresentablePath);
    }
    let empty_descriptors = descriptor::Shapes::new();
    let descriptors = baseline
        .as_ref()
        .map_or(&empty_descriptors, |saved| &saved.descriptors);
    let project_type = target.project_type();
    let (mode, import_files) = if initialize {
        (Mode::Initialize, None)
    } else if changes.is_empty() {
        (Mode::Unchanged, None)
    } else if !picture_references.is_empty() {
        (Mode::Unsupported, None)
    } else {
        structural
            .as_ref()
            .map_or_else(
                || descriptor::import_files(source, project_type, &changes, current, descriptors),
                |selection| Some(selection.files.clone()),
            )
            .filter(|paths| paths.iter().all(|path| input_path(path).is_some()))
            .map_or((Mode::Unsupported, None), |paths| {
                (Mode::Incremental, Some(paths))
            })
    };
    let confirmation = if matches!(mode, Mode::Initialize | Mode::Incremental) {
        let identity = (
            &target.connection,
            &target.identity,
            &target.launch,
            &target.extension,
            &target.dependencies,
            target.platform.version().as_str(),
            initialize,
            &inspection.database_generation,
            &inspection.configuration_generation,
            &inspection.dependency_generations,
            &inspection.baseline_generation,
            pending,
            &import_files,
            previous,
            current,
        );
        Some(checksum(&[
            &serde_json::to_vec(&identity).map_err(|_| ApplyError::UnrepresentablePath)?
        ]))
    } else {
        None
    };
    let (mut root, mut descriptors) = baseline.map_or((None, descriptor::Shapes::new()), |saved| {
        (saved.root, saved.descriptors)
    });
    let object_changes = if mode == Mode::Incremental
        && let Some(selection) = structural
    {
        selection.update_baseline(&mut root, &mut descriptors)
    } else {
        Vec::new()
    };
    Ok(Plan {
        mode,
        target: target.identity.clone(),
        platform_version: target.platform.version().as_str().to_owned(),
        launch: target.launch.clone(),
        extension: target.extension.clone(),
        changes,
        import_files,
        object_changes,
        picture_references,
        confirmation,
        inspection,
        descriptors,
        root: if initialize { None } else { root },
    })
}

fn read_baseline(path: &Path) -> Result<Result<Baseline, ReviewReason>, ApplyError> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).map_err(|_| ReviewReason::InvalidBaseline)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Err(ReviewReason::BaselineRequired))
        }
        Err(error) => Err(ApplyError::Io(error)),
    }
}

impl Inspection {
    fn new(
        delta: Option<&BTreeMap<PathBuf, Change>>,
        baseline_generation: Option<&str>,
        observation: Option<Observation>,
        pending_apply: bool,
    ) -> Self {
        let (database_generation, configuration_generation, dependency_generations) = observation
            .map_or_else(
                || (None, None, BTreeMap::new()),
                |observed| {
                    (
                        Some(observed.generation),
                        observed.configuration_generation,
                        observed.dependency_generations,
                    )
                },
            );
        let sources = match delta {
            Some(delta) if delta.is_empty() => Comparison::Unchanged,
            Some(_) => Comparison::Changed,
            None => Comparison::Unknown,
        };
        let database = match baseline_generation.zip(database_generation.as_deref()) {
            Some((baseline, actual)) if baseline == actual => Comparison::Unchanged,
            Some(_) => Comparison::Changed,
            None => Comparison::Unknown,
        };
        let status = if pending_apply {
            Status::Incomplete
        } else if database_generation.is_none() {
            Status::DatabaseUnavailable
        } else {
            match (sources, database) {
                (Comparison::Unknown, _) | (_, Comparison::Unknown) => Status::UnknownBaseline,
                (Comparison::Unchanged, Comparison::Unchanged) => Status::UpToDate,
                (Comparison::Changed, Comparison::Unchanged) => Status::LocalChanges,
                (Comparison::Unchanged, Comparison::Changed) => Status::DatabaseChanged,
                (Comparison::Changed, Comparison::Changed) => Status::BothChanged,
            }
        };
        Self {
            status,
            sources,
            database,
            baseline_generation: baseline_generation.map(str::to_owned),
            database_generation,
            configuration_generation,
            dependency_generations,
            pending_apply,
        }
    }
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
