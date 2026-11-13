//! Project-local runtime settings shared by IDE and command-line operations.

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::file_lock::FileLock;

use super::build::PlatformVersion;

pub const SETTINGS_PATH: &str = ".eska/settings_local.toml";
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;
pub const DEBUG_TARGET_TYPES: &[&str] = &[
    "Client",
    "ManagedClient",
    "WEBClient",
    "COMConnector",
    "Server",
    "ServerEmulation",
    "WEBService",
    "HTTPService",
    "OData",
    "JOB",
    "JobFileMode",
    "MobileClient",
    "MobileServer",
    "MobileJobFileMode",
    "MobileManagedClient",
    "MobileManagedServer",
];
pub const DEFAULT_DEBUG_TARGET_TYPES: &[&str] =
    &["ManagedClient", "Client", "Server", "ServerEmulation"];

#[must_use]
pub fn valid_auto_attach_types(types: &[String]) -> bool {
    types.len() <= DEBUG_TARGET_TYPES.len()
        && types.iter().enumerate().all(|(index, kind)| {
            DEBUG_TARGET_TYPES.contains(&kind.as_str()) && !types[..index].contains(kind)
        })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileName {
    Test,
    Prod,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Inspect,
    Run,
    DebugLaunch,
    DebugAttach,
    Apply,
}

#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSettings {
    pub test: Option<Profile>,
    pub prod: Option<Profile>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub infobase: Infobase,
    pub runner: Runner,
    pub debug: Option<DebugEndpoint>,
    pub authentication: Option<Authentication>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Infobase {
    File {
        path: PathBuf,
    },
    Cluster {
        host: String,
        name: String,
    },
    Standalone {
        url: String,
        name: String,
        admin_pid: Option<u32>,
    },
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Runner {
    Local {
        version: Option<String>,
        ibcmd: Option<PathBuf>,
    },
    Distrobox {
        version: Option<String>,
        container: String,
        ibcmd: Option<PathBuf>,
    },
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DebugEndpoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub managed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_attach_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_processing: Option<DebugProcessing>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DebugProcessing {
    pub artifact: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_url: Option<String>,
}

impl DebugEndpoint {
    #[must_use]
    pub fn auto_attach_types(&self) -> Vec<String> {
        self.auto_attach_types.clone().unwrap_or_else(|| {
            DEFAULT_DEBUG_TARGET_TYPES
                .iter()
                .map(|kind| (*kind).to_owned())
                .collect()
        })
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Authentication {
    pub user: String,
    pub password_env: Option<String>,
}

#[derive(Debug)]
pub enum SettingsError {
    Io(io::Error),
    InvalidFile,
    TooLarge,
    InvalidField {
        profile: ProfileName,
        field: &'static str,
    },
    MissingProfile(ProfileName),
    DebugEndpointRequired(ProfileName),
    AdminEndpointRequired,
    ForbiddenAction,
    ClusterApplyUnsupported,
    SharedInfobase,
    Changed,
}

pub struct Snapshot {
    pub settings: LocalSettings,
    pub revision: Option<String>,
}

impl LocalSettings {
    /// Validate both declarations before saving. Error coordinates never include private values.
    /// # Errors
    /// Returns the profile and first invalid field.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if let Some(profile) = &self.test {
            profile.validate(ProfileName::Test)?;
        }
        if let Some(profile) = &self.prod {
            profile.validate(ProfileName::Prod)?;
        }
        Ok(())
    }

    /// Resolve a single operation. A production debug session needs only the production profile.
    /// # Errors
    /// Returns a policy, validation, or missing-settings error before a platform command can run.
    pub fn resolve(
        &self,
        root: &Path,
        name: ProfileName,
        action: Action,
    ) -> Result<&Profile, SettingsError> {
        let profile = match name {
            ProfileName::Prod if matches!(action, Action::Apply | Action::Run) => {
                return Err(SettingsError::ForbiddenAction);
            }
            ProfileName::Test => self.test.as_ref(),
            ProfileName::Prod => self.prod.as_ref(),
        }
        .ok_or(SettingsError::MissingProfile(name))?;
        profile.validate(name)?;
        if matches!(action, Action::DebugLaunch | Action::DebugAttach) && profile.debug.is_none() {
            return Err(SettingsError::DebugEndpointRequired(name));
        }
        if action == Action::Apply {
            match &profile.infobase {
                Infobase::Cluster { .. } => return Err(SettingsError::ClusterApplyUnsupported),
                Infobase::Standalone {
                    admin_pid: None, ..
                } => {
                    return Err(SettingsError::AdminEndpointRequired);
                }
                _ => {}
            }
            if self
                .prod
                .as_ref()
                .is_some_and(|prod| profile.infobase.same_target(&prod.infobase, root))
            {
                return Err(SettingsError::SharedInfobase);
            }
        }
        Ok(profile)
    }
}

impl Profile {
    /// Validate the declaration independently of its source or runtime availability.
    /// # Errors
    /// Returns the selected profile and the first invalid field.
    pub fn validate(&self, name: ProfileName) -> Result<(), SettingsError> {
        let require = |condition: bool, field| {
            condition.then_some(()).ok_or(SettingsError::InvalidField {
                profile: name,
                field,
            })
        };
        match &self.infobase {
            Infobase::File { path } => require(path_text(path), "infobase.path")?,
            Infobase::Cluster { host, name } => {
                require(host_only(&format!("http://{host}")), "infobase.host")?;
                require(infobase_name(name), "infobase.name")?;
            }
            Infobase::Standalone {
                url,
                name,
                admin_pid,
            } => {
                require(http_url(url).is_some(), "infobase.url")?;
                require(infobase_name(name), "infobase.name")?;
                require(admin_pid.is_none_or(|pid| pid > 0), "infobase.admin_pid")?;
            }
        }
        let (version, ibcmd) = match &self.runner {
            Runner::Local { version, ibcmd } => (version, ibcmd),
            Runner::Distrobox {
                version,
                container,
                ibcmd,
            } => {
                require(identifier(container, b"-_."), "runner.container")?;
                (version, ibcmd)
            }
        };
        require(
            version
                .as_ref()
                .is_none_or(|version| PlatformVersion::parse(version).is_ok()),
            "runner.version",
        )?;
        require(
            ibcmd
                .as_ref()
                .is_none_or(|path| path.is_absolute() && path_text(path)),
            "runner.ibcmd",
        )?;
        if let Some(debug) = &self.debug {
            if debug.managed {
                require(
                    matches!(self.infobase, Infobase::File { .. }),
                    "debug.managed",
                )?;
                require(debug.url.is_none(), "debug.url")?;
                require(debug.port != Some(0), "debug.port")?;
            } else {
                require(
                    debug.url.as_deref().and_then(debug_server_url).is_some(),
                    "debug.url",
                )?;
                require(debug.port.is_none(), "debug.port")?;
            }
            require(
                debug
                    .auto_attach_types
                    .as_deref()
                    .is_none_or(valid_auto_attach_types),
                "debug.auto_attach_types",
            )?;
            if let Some(processing) = &debug.external_processing {
                require(
                    path_text(&processing.artifact),
                    "debug.external_processing.artifact",
                )?;
                require(
                    processing.debug_url.as_deref().is_none_or(text),
                    "debug.external_processing.debug_url",
                )?;
            }
        }
        if let Some(auth) = &self.authentication {
            require(text(&auth.user), "authentication.user")?;
            require(
                auth.password_env
                    .as_ref()
                    .is_none_or(|key| identifier(key, b"_") && !key.as_bytes()[0].is_ascii_digit()),
                "authentication.password_env",
            )?;
        }
        Ok(())
    }
}

impl Infobase {
    fn same_target(&self, other: &Self, root: &Path) -> bool {
        match (self, other) {
            (Self::File { path: a }, Self::File { path: b }) => {
                let a = root.join(a);
                let b = root.join(b);
                a == b
                    || a.canonicalize()
                        .ok()
                        .zip(b.canonicalize().ok())
                        .is_some_and(|(a, b)| a == b)
            }
            (
                Self::Standalone {
                    url: a,
                    admin_pid: aa,
                    ..
                },
                Self::Standalone {
                    url: b,
                    admin_pid: ba,
                    ..
                },
            ) => equal_url(a, b) || aa.is_some() && aa == ba,
            _ => false,
        }
    }
}

fn equal_url(a: &str, b: &str) -> bool {
    http_url(a).zip(http_url(b)).is_some_and(|(a, b)| a == b)
}

fn text(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
}

fn path_text(path: &Path) -> bool {
    path.to_str().is_some_and(text)
}

fn identifier(value: &str, extra: &[u8]) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || extra.contains(&byte))
}

fn infobase_name(value: &str) -> bool {
    text(value) && value == value.trim() && !value.contains(['/', '\\', ';', '"', '?', '#'])
}

fn host_only(value: &str) -> bool {
    http_url(value).is_some_and(|url| url.path() == "/")
}

/// Validate the origin of a plain HTTP debugger endpoint.
#[must_use]
pub fn debug_server_url(value: &str) -> Option<Url> {
    http_url(value).filter(|url| url.scheme() == "http" && url.path() == "/")
}

fn http_url(value: &str) -> Option<Url> {
    if !(value.starts_with("http://") || value.starts_with("https://"))
        || value.chars().any(char::is_whitespace)
        || value.contains(['\\', '@', '?', '#'])
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.has_host() && url.port() != Some(0))
        .then_some(url)
}

/// Load the exact project's settings. A missing document represents two unconfigured profiles.
/// # Errors
/// Returns a sanitized parse error or an error reading the private settings.
pub fn load(root: &Path) -> Result<Snapshot, SettingsError> {
    let directory = root.join(".eska");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(SettingsError::InvalidFile),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(empty_snapshot()),
        Err(error) => return Err(SettingsError::Io(error)),
    }
    let path = root.join(SETTINGS_PATH);
    let file = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => File::open(&path).map_err(SettingsError::Io)?,
        Ok(_) => return Err(SettingsError::InvalidFile),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(empty_snapshot()),
        Err(error) => return Err(SettingsError::Io(error)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(SettingsError::Io)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(SettingsError::TooLarge);
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| SettingsError::InvalidFile)?;
    let settings = toml::from_str(source.trim_start_matches('\u{feff}'))
        .map_err(|_| SettingsError::InvalidFile)?;
    Ok(Snapshot {
        settings,
        revision: Some(URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes))),
    })
}

fn empty_snapshot() -> Snapshot {
    Snapshot {
        settings: LocalSettings::default(),
        revision: None,
    }
}

/// Publish a checked document atomically. The caller verifies Git exclusion before writing.
/// # Errors
/// Rejects concurrent changes, invalid documents, unsafe paths and failed writes.
pub fn save(
    root: &Path,
    expected_revision: Option<&str>,
    settings: &LocalSettings,
) -> Result<Snapshot, SettingsError> {
    settings.validate()?;
    let contents = toml::to_string_pretty(settings).map_err(|_| SettingsError::InvalidFile)?;
    if contents.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(SettingsError::TooLarge);
    }
    let directory = root.join(".eska");
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&directory)
                .map_err(SettingsError::Io)?
                .is_dir()
            {
                return Err(SettingsError::InvalidFile);
            }
        }
        Err(error) => return Err(SettingsError::Io(error)),
    }
    let lock_path = directory.join("settings_local.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(SettingsError::Io)?;
    let _lock = FileLock::try_lock(lock).map_err(|error| match error {
        TryLockError::WouldBlock => SettingsError::Changed,
        TryLockError::Error(error) => SettingsError::Io(error),
    })?;
    if load(root)?.revision.as_deref() != expected_revision {
        return Err(SettingsError::Changed);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(SettingsError::Io)?;
    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(SettingsError::Io)?;
    temporary
        .persist(root.join(SETTINGS_PATH))
        .map_err(|error| SettingsError::Io(error.error))?;
    Ok(Snapshot {
        settings: settings.clone(),
        revision: Some(URL_SAFE_NO_PAD.encode(Sha256::digest(contents))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    fn file_profile(path: &str) -> Profile {
        Profile {
            infobase: Infobase::File { path: path.into() },
            runner: Runner::Local {
                version: None,
                ibcmd: None,
            },
            debug: Some(DebugEndpoint {
                url: Some("http://127.0.0.1:1550".into()),
                managed: false,
                port: None,
                auto_attach_types: None,
                external_processing: None,
            }),
            authentication: None,
        }
    }

    #[test]
    fn debug_processing_is_optional_and_round_trips_with_the_profile() {
        let mut profile = file_profile("demo");
        let processing = DebugProcessing {
            artifact: "artifacts/Демо.epf".into(),
            debug_url: Some(format!("file:///EPF/{}/Демо.epf", "a".repeat(64))),
        };
        profile.debug.as_mut().unwrap().external_processing = Some(processing);
        let mut settings = LocalSettings {
            test: None,
            prod: Some(profile),
        };
        let toml = toml::to_string(&settings).unwrap();
        assert!(toml.contains("[prod.debug.external_processing]"));
        let decoded: LocalSettings = toml::from_str(&toml).unwrap();
        assert!(decoded == settings);
        assert!(decoded.validate().is_ok());
        settings
            .prod
            .as_mut()
            .unwrap()
            .debug
            .as_mut()
            .unwrap()
            .external_processing
            .as_mut()
            .unwrap()
            .artifact = PathBuf::new();
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::InvalidField {
                profile: ProfileName::Prod,
                field: "debug.external_processing.artifact",
            })
        ));
    }

    #[test]
    fn prod_debug_does_not_require_test_and_rejects_administration() {
        let settings = LocalSettings {
            test: None,
            prod: Some(file_profile("release")),
        };
        for action in [Action::DebugLaunch, Action::DebugAttach] {
            assert!(
                settings
                    .resolve(Path::new("."), ProfileName::Prod, action)
                    .is_ok()
            );
        }
        for action in [Action::Run, Action::Apply] {
            assert!(matches!(
                settings.resolve(Path::new("."), ProfileName::Prod, action),
                Err(SettingsError::ForbiddenAction)
            ));
        }
        assert!(matches!(
            settings.resolve(Path::new("."), ProfileName::Test, Action::Run),
            Err(SettingsError::MissingProfile(ProfileName::Test))
        ));
    }

    #[test]
    fn apply_rejects_a_test_path_resolving_to_the_prod_directory() {
        let directory = TestDir::new();
        fs::create_dir(directory.0.join("база")).unwrap();
        let settings = LocalSettings {
            test: Some(file_profile("./база")),
            prod: Some(file_profile(directory.0.join("база").to_str().unwrap())),
        };
        assert!(matches!(
            settings.resolve(&directory.0, ProfileName::Test, Action::Apply),
            Err(SettingsError::SharedInfobase)
        ));
        assert!(
            settings
                .resolve(&directory.0, ProfileName::Test, Action::Run)
                .is_ok()
        );
    }

    #[test]
    fn local_settings_round_trip_independently_and_reject_stale_writes() {
        let first = TestDir::new();
        let second = TestDir::new();
        let settings = LocalSettings {
            test: Some(file_profile("Тестовая база")),
            prod: None,
        };
        let initial = save(&first.0, None, &settings).unwrap();
        assert!(load(&first.0).unwrap().settings == settings);
        assert!(load(&second.0).unwrap().settings.test.is_none());
        assert!(matches!(
            save(&first.0, None, &LocalSettings::default()),
            Err(SettingsError::Changed)
        ));
        save(
            &first.0,
            initial.revision.as_deref(),
            &LocalSettings::default(),
        )
        .unwrap();
        assert!(load(&first.0).unwrap().settings.test.is_none());
    }

    #[test]
    fn debug_endpoints_reject_credentials_and_non_http_transports() {
        for invalid in [
            "http://user:secret@127.0.0.1:1550",
            "http://127.0.0.1:0",
            "tcp://127.0.0.1:1550",
            "http://127.0.0.1:1550/debugger",
        ] {
            let mut prod = file_profile("release");
            prod.debug = Some(DebugEndpoint {
                url: Some(invalid.into()),
                managed: false,
                port: None,
                auto_attach_types: None,
                external_processing: None,
            });
            let settings = LocalSettings {
                test: None,
                prod: Some(prod),
            };
            assert!(matches!(
                settings.validate(),
                Err(SettingsError::InvalidField {
                    profile: ProfileName::Prod,
                    field: "debug.url",
                })
            ));
        }
    }

    #[test]
    fn auto_attach_distinguishes_defaults_from_disabled_and_validates_types() {
        let mut profile = file_profile("test");
        let debug = profile.debug.as_mut().unwrap();
        assert_eq!(debug.auto_attach_types(), DEFAULT_DEBUG_TARGET_TYPES);
        debug.auto_attach_types = Some(vec![]);
        assert_eq!(debug.auto_attach_types(), Vec::<String>::new());
        assert!(profile.validate(ProfileName::Test).is_ok());
        for kinds in [vec!["Unknown"], vec!["Client", "Client"], vec!["client"]] {
            profile.debug.as_mut().unwrap().auto_attach_types =
                Some(kinds.into_iter().map(str::to_owned).collect());
            assert!(matches!(
                profile.validate(ProfileName::Test),
                Err(SettingsError::InvalidField {
                    field: "debug.auto_attach_types",
                    ..
                })
            ));
        }
    }
}
