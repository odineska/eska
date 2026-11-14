//! Launch a configured client through the same verified platform installation as ibcmd.

pub mod apply;
mod debug_server;
pub use debug_server::{DebugConnection, ManagedDebugServer};

use std::{
    env,
    ffi::OsString,
    fmt::Write as _,
    fs, io,
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc},
    thread,
    time::{Duration, Instant},
};

use eska_project_core::project::{
    Project,
    local_settings::{self, Action, Infobase, Profile, ProfileName, Runner, SettingsError},
};
use sha2::{Digest, Sha256};

use crate::{
    build::{
        ArtifactType, Ibcmd, PlatformVersion, ProcessingArtifactError, ProcessingSources, RunError,
        ToolError, ToolOptions, manifest::verify_processing,
    },
    platform_path::native_path,
};

#[derive(Debug)]
pub enum RuntimeError {
    Settings(SettingsError),
    Platform(ToolError),
    PlatformUnavailable,
    FileInfobaseUnavailable,
    EndpointUnavailable,
    LaunchTargetChanged,
    SecretUnavailable,
    Io(io::Error),
    Run(RunError),
    ClientFailed,
}

fn launch_fingerprint(root: &Path, profile: &mut Profile) -> Result<String, RuntimeError> {
    if let Infobase::File { path } = &mut profile.infobase {
        *path = file_infobase(root, path)?;
    }
    let bytes = serde_json::to_vec(&(&profile.infobase, &profile.runner))
        .map_err(|error| RuntimeError::Io(error.into()))?;
    Ok(checksum(&[&bytes]))
}

fn checksum(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn verify_launch_target(
    project: &Project,
    profile: &mut Profile,
    expected: Option<&str>,
) -> Result<(), RuntimeError> {
    if let Some(expected) = expected
        && launch_fingerprint(project.root(), profile)? != expected
    {
        return Err(RuntimeError::LaunchTargetChanged);
    }
    Ok(())
}

/// Load and authorize exactly one profile before reading any external connection state.
/// # Errors
/// Returns an error when the profile is absent, invalid or forbidden for the operation.
pub fn profile(
    project: &Project,
    name: ProfileName,
    action: Action,
) -> Result<Profile, RuntimeError> {
    if name == ProfileName::Prod && matches!(action, Action::Run | Action::Apply) {
        return Err(RuntimeError::Settings(SettingsError::ForbiddenAction));
    }
    let saved = local_settings::load(project.root()).map_err(RuntimeError::Settings)?;
    saved
        .settings
        .resolve(project.root(), name, action)
        .cloned()
        .map_err(RuntimeError::Settings)
}

/// Select the exact profile platform, falling back to the project's build version or an installed version.
/// # Errors
/// Returns a structured discovery or verification error.
pub fn platform(project: &Project, profile: &Profile) -> Result<Ibcmd, RuntimeError> {
    select_platform(
        project.configuration().build_settings().platform_version(),
        profile,
    )
}

fn select_platform(
    project_version: Option<&PlatformVersion>,
    profile: &Profile,
) -> Result<Ibcmd, RuntimeError> {
    let (version, ibcmd, container) = match &profile.runner {
        Runner::Local { version, ibcmd } => (version, ibcmd, None),
        Runner::Distrobox {
            version,
            ibcmd,
            container,
        } => (version, ibcmd, Some(container.clone())),
    };
    let options = ToolOptions::project_profile(ibcmd.clone(), container);
    let configured = version
        .as_deref()
        .map(PlatformVersion::parse)
        .transpose()
        .map_err(|_| RuntimeError::PlatformUnavailable)?;
    let installed;
    let version = if let Some(version) = configured.as_ref().or(project_version) {
        version
    } else {
        installed = Ibcmd::installed(&options).map_err(RuntimeError::Platform)?;
        installed
            .first()
            .ok_or(RuntimeError::PlatformUnavailable)?
            .version()
    };
    Ibcmd::discover(version, &options).map_err(RuntimeError::Platform)
}

/// Return the directory of an existing, regular file infobase.
/// # Errors
/// Returns an error if the path cannot be resolved or the platform database file is absent.
pub fn file_infobase(root: &Path, path: &Path) -> Result<PathBuf, RuntimeError> {
    let directory = root
        .join(path)
        .canonicalize()
        .map_err(|_| RuntimeError::FileInfobaseUnavailable)?;
    if directory.join("1Cv8.1CD").is_file() {
        Ok(directory)
    } else {
        Err(RuntimeError::FileInfobaseUnavailable)
    }
}

/// Check the configured installation and transport without launching a client or using credentials.
/// # Errors
/// Returns the first unavailable or invalid part of the saved profile.
pub fn inspect(project: &Project, name: ProfileName) -> Result<(), RuntimeError> {
    let profile = profile(project, name, Action::Inspect)?;
    inspect_profile(project, &profile)
}

/// Check a supplied profile using the same runtime as a saved profile.
/// # Errors
/// Returns an invalid field or the first unavailable runtime dependency.
pub fn inspect_draft(
    project: &Project,
    name: ProfileName,
    profile: &Profile,
) -> Result<(), RuntimeError> {
    profile.validate(name).map_err(RuntimeError::Settings)?;
    inspect_profile(project, profile)
}

fn inspect_profile(project: &Project, profile: &Profile) -> Result<(), RuntimeError> {
    let platform = platform(project, profile)?;
    if !platform
        .program_available("1cv8c")
        .map_err(RuntimeError::Io)?
    {
        return Err(RuntimeError::PlatformUnavailable);
    }
    match &profile.infobase {
        Infobase::File { path } => {
            file_infobase(project.root(), path)?;
        }
        Infobase::Cluster { host, .. } => {
            let url = url::Url::parse(&format!("tcp://{host}"))
                .map_err(|_| RuntimeError::EndpointUnavailable)?;
            inspect_endpoint(&url, 1541)?;
        }
        Infobase::Standalone { url, .. } => {
            let url = url::Url::parse(url).map_err(|_| RuntimeError::EndpointUnavailable)?;
            inspect_endpoint(&url, url.port_or_known_default().unwrap_or(80))?;
        }
    }
    if let Some(debug) = &profile.debug {
        if debug.managed {
            if !platform
                .program_available("dbgs")
                .map_err(RuntimeError::Io)?
            {
                return Err(RuntimeError::EndpointUnavailable);
            }
        } else if let DebugConnection::External(url) = DebugConnection::external(debug)? {
            inspect_endpoint(&url, 80)?;
        }
    }
    Ok(())
}

fn inspect_endpoint(url: &url::Url, default_port: u16) -> Result<(), RuntimeError> {
    let host = url
        .host_str()
        .ok_or(RuntimeError::EndpointUnavailable)?
        .trim_matches(['[', ']'])
        .to_owned();
    let port = url.port().unwrap_or(default_port);
    let timeout = Duration::from_secs(2);
    let deadline = Instant::now() + timeout;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .spawn(move || {
            let reachable = (host.as_str(), port)
                .to_socket_addrs()
                .is_ok_and(|mut addresses| {
                    addresses.any(|address| {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        !remaining.is_zero()
                            && TcpStream::connect_timeout(&address, remaining).is_ok()
                    })
                });
            let _ = sender.send(reachable);
        })
        .map_err(RuntimeError::Io)?;
    if receiver.recv_timeout(timeout) == Ok(true) {
        Ok(())
    } else {
        Err(RuntimeError::EndpointUnavailable)
    }
}

/// Run only the authorized test client. The command owns and waits for its child.
/// # Errors
/// Returns a profile, platform, startup or cancellation error without printing private settings.
pub fn run(
    project: &Project,
    name: ProfileName,
    expected: Option<&str>,
) -> Result<(), RuntimeError> {
    let mut profile = profile(project, name, Action::Run)?;
    verify_launch_target(project, &mut profile, expected)?;
    let platform = platform(project, &profile)?;
    launch_client(
        project.root(),
        Some(&project.root().join(".eska")),
        &profile,
        &platform,
        None,
        None,
    )
}

/// A debug client and its endpoint are resolved together before the DAP session is configured.
pub struct DebugLaunch {
    base: PathBuf,
    scratch_parent: Option<PathBuf>,
    profile: Profile,
    platform: Ibcmd,
    processing: Option<(tempfile::TempDir, PathBuf)>,
}

impl DebugLaunch {
    /// Check the production/test action and prepare the same platform runner used for ordinary launch.
    /// # Errors
    /// Returns profile, platform or file-infobase availability errors.
    pub fn prepare(
        project: &Project,
        name: ProfileName,
        expected: Option<&str>,
    ) -> Result<Self, RuntimeError> {
        let mut profile = profile(project, name, Action::DebugLaunch)?;
        verify_launch_target(project, &mut profile, expected)?;
        Self::prepare_launch(
            project.root().to_path_buf(),
            Some(project.root().join(".eska")),
            project.configuration().build_settings().platform_version(),
            profile,
        )
    }

    /// Prepare a native debug client without an ESKA project or local settings file.
    /// # Errors
    /// Rejects invalid runtime settings or an unavailable platform or file infobase.
    pub fn prepare_standalone(base: PathBuf, profile: Profile) -> Result<Self, RuntimeError> {
        let settings = local_settings::LocalSettings {
            test: Some(profile),
            prod: None,
        };
        let profile = settings
            .resolve(&base, ProfileName::Test, Action::DebugLaunch)
            .map_err(RuntimeError::Settings)?
            .clone();
        Self::prepare_launch(base, None, None, profile)
    }

    fn prepare_launch(
        base: PathBuf,
        scratch_parent: Option<PathBuf>,
        version: Option<&PlatformVersion>,
        profile: Profile,
    ) -> Result<Self, RuntimeError> {
        if let Infobase::File { path } = &profile.infobase {
            file_infobase(&base, path)?;
        }
        let platform = select_platform(version, &profile)?;
        Ok(Self {
            base,
            scratch_parent,
            profile,
            platform,
            processing: None,
        })
    }

    /// Stage an EPF at a private path unique to this launch and bind it to the built sources.
    /// # Errors
    /// Rejects an unreadable artifact or a mismatch with its build manifest.
    pub fn stage_processing(
        &mut self,
        source: &Path,
        artifact: &Path,
    ) -> Result<(ProcessingSources, PathBuf), ProcessingArtifactError> {
        use ProcessingArtifactError::{ArtifactMismatch, Io};
        if !artifact.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case(ArtifactType::Processing.extension())
        }) || !artifact.is_file()
        {
            return Err(ArtifactMismatch);
        }
        let mut builder = tempfile::Builder::new();
        builder.prefix("eska-debug-epf-");
        let scratch = self
            .scratch_parent
            .as_deref()
            .map_or_else(|| builder.tempdir(), |parent| builder.tempdir_in(parent))
            .map_err(Io)?;
        let path = scratch
            .path()
            .canonicalize()
            .map_err(Io)?
            .join(artifact.file_name().ok_or(ArtifactMismatch)?);
        fs::copy(artifact, &path).map_err(Io)?;
        let sources = verify_processing(source, artifact, &path)?;
        let path = native_path(&path).into_owned();
        self.processing = Some((scratch, path.clone()));
        Ok((sources, path))
    }

    #[must_use]
    pub const fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Obtain the debugger endpoint or reuse the verified platform for owned dbgs startup.
    /// # Errors
    /// Returns an endpoint error when the debug settings are missing or invalid.
    pub fn connection(&self) -> Result<DebugConnection, RuntimeError> {
        let debug = self
            .profile
            .debug
            .as_ref()
            .ok_or(RuntimeError::EndpointUnavailable)?;
        if debug.managed {
            Ok(DebugConnection::Managed {
                platform: self.platform.clone(),
                port: debug.port,
            })
        } else {
            DebugConnection::external(debug)
        }
    }

    /// Start the debugger-enabled client only after initial DAP configuration has completed.
    /// # Errors
    /// Returns native launch or cancellation errors.
    pub fn run(&self, endpoint: &url::Url, cancelled: &AtomicBool) -> Result<(), RuntimeError> {
        launch_client(
            &self.base,
            self.scratch_parent.as_deref(),
            &self.profile,
            &self.platform,
            Some((
                endpoint,
                self.processing.as_ref().map(|(_, path)| path.as_path()),
            )),
            Some(cancelled),
        )
    }
}

/// Start a native client through a verified platform and reap it on cancellation.
/// # Errors
/// Returns a settings, secret or process lifecycle error.
fn launch_client(
    base: &Path,
    scratch_parent: Option<&Path>,
    profile: &Profile,
    platform: &Ibcmd,
    debug: Option<(&url::Url, Option<&Path>)>,
    cancelled: Option<&AtomicBool>,
) -> Result<(), RuntimeError> {
    let mut arguments = vec![OsString::from("ENTERPRISE")];
    match &profile.infobase {
        Infobase::File { path } => {
            let path = file_infobase(base, path)?;
            arguments.extend(["/F".into(), native_path(&path).as_os_str().to_owned()]);
        }
        Infobase::Cluster { host, name } => {
            arguments.extend(["/S".into(), format!("{host}\\{name}").into()]);
        }
        Infobase::Standalone { url, .. } => arguments.extend(["/WS".into(), url.into()]),
    }
    if let Some((endpoint, processing)) = debug {
        arguments.extend([
            "/DEBUG".into(),
            "-http".into(),
            "-attach".into(),
            "/DEBUGGERURL".into(),
            endpoint.as_str().into(),
            "/DisableStartupMessages".into(),
            "/DisableStartupDialogs".into(),
        ]);
        if let Some(path) = processing {
            arguments.extend(["/Execute".into(), path.as_os_str().to_owned()]);
        }
    }
    if let Some(auth) = &profile.authentication {
        arguments.extend(["/N".into(), (&auth.user).into()]);
        if let Some(key) = &auth.password_env {
            let password = env::var_os(key).ok_or(RuntimeError::SecretUnavailable)?;
            arguments.extend(["/WA-".into(), "/P".into(), password]);
        }
    }
    let mut scratch = tempfile::Builder::new();
    scratch.prefix("eska-launch-");
    let scratch = scratch_parent
        .map_or_else(|| scratch.tempdir(), |parent| scratch.tempdir_in(parent))
        .map_err(RuntimeError::Io)?;
    let client = platform.sibling_program("1cv8c");
    // The adapter already owns signal handling and cancels the client via this flag.
    if cancelled.is_none() {
        client
            .begin_interruptible_operation()
            .map_err(RuntimeError::Run)?;
    }
    let output = client
        .run_owned(
            arguments,
            &scratch.path().join("process.pid"),
            None,
            None,
            &mut |_, _| {},
            cancelled,
        )
        .map_err(RuntimeError::Run)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RuntimeError::ClientFailed)
    }
}
