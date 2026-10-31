//! Machine-local configuration stored outside project repositories.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::BaseDirs;
use serde::Deserialize;

const CONFIG_DIRECTORY_ENV: &str = "ESKA_CONFIG_DIR";
const DEFAULT_CONFIG: &str = "[build]\nrunner = \"auto\"\n";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RunnerKind {
    #[default]
    Auto,
    Host,
    Distrobox,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GlobalBuildConfig {
    pub runner: RunnerKind,
    pub container: Option<String>,
    pub platform_arch: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GlobalConfig {
    pub build: GlobalBuildConfig,
    pub libraries: BTreeMap<String, GlobalLibrary>,
}

/// One local Designer XML source, independently selected by a project manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GlobalLibrary {
    pub source: PathBuf,
}

impl GlobalConfig {
    /// Resolve only the selected catalog entry against its owning configuration file.
    ///
    /// # Errors
    /// Rejects unknown names, empty sources and unavailable absolute config locations.
    pub fn library_path(&self, config: &Path, name: &str) -> Result<PathBuf, GlobalConfigError> {
        let entry = self
            .libraries
            .get(name)
            .ok_or_else(|| GlobalConfigError::UnknownLibrary {
                path: config.to_owned(),
                name: name.to_owned(),
            })?;
        if entry.source.as_os_str().is_empty() {
            return Err(GlobalConfigError::EmptyLibrarySource {
                path: config.to_owned(),
                name: name.to_owned(),
            });
        }
        let config = std::path::absolute(config).map_err(|source| io_error(config, source))?;
        let parent = config
            .parent()
            .ok_or(GlobalConfigError::LocationUnavailable)?;
        Ok(parent.join(&entry.source))
    }

    /// Load the machine config, or return portable defaults when it does not exist.
    ///
    /// # Errors
    /// Rejects unreadable files, invalid TOML and contradictory runner settings.
    pub fn load(path: &Path) -> Result<Self, GlobalConfigError> {
        let input = match fs::read_to_string(path) {
            Ok(input) => input,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(io_error(path, source)),
        };
        Self::parse(path, &input)
    }

    /// Parse and validate the strict machine-local TOML schema.
    pub(crate) fn parse(path: &Path, input: &str) -> Result<Self, GlobalConfigError> {
        let raw: RawGlobalConfig =
            toml::from_str(input).map_err(|source| GlobalConfigError::Invalid {
                path: path.to_owned(),
                source,
            })?;
        let runner = match raw.build.runner {
            RawRunnerKind::Auto => RunnerKind::Auto,
            RawRunnerKind::Host => RunnerKind::Host,
            RawRunnerKind::Distrobox => RunnerKind::Distrobox,
        };
        if runner == RunnerKind::Distrobox && raw.build.container.is_none() {
            return Err(GlobalConfigError::DistroboxContainerMissing {
                path: path.to_owned(),
            });
        }
        if runner == RunnerKind::Host && raw.build.container.is_some() {
            return Err(GlobalConfigError::HostContainerUnexpected {
                path: path.to_owned(),
            });
        }
        Ok(Self {
            libraries: raw.libraries,
            build: GlobalBuildConfig {
                runner,
                container: raw.build.container,
                platform_arch: raw.build.platform_arch,
            },
        })
    }
}

/// Load the machine config from its resolved path.
///
/// # Errors
/// Returns unavailable user locations and the same validation errors as [`GlobalConfig::load`].
pub fn load() -> Result<(PathBuf, GlobalConfig), GlobalConfigError> {
    let path = config_path()?;
    let config = GlobalConfig::load(&path)?;
    Ok((path, config))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGlobalConfig {
    #[serde(default)]
    libraries: BTreeMap<String, GlobalLibrary>,
    #[serde(default)]
    build: RawGlobalBuildConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGlobalBuildConfig {
    #[serde(default)]
    runner: RawRunnerKind,
    container: Option<String>,
    platform_arch: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RawRunnerKind {
    #[default]
    Auto,
    Host,
    Distrobox,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitOutcome {
    Created(PathBuf),
    Existing(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditOutcome {
    Unchanged(PathBuf),
    Changed { path: PathBuf, backup: PathBuf },
}

#[derive(Debug)]
pub enum GlobalConfigError {
    UnknownLibrary {
        path: PathBuf,
        name: String,
    },
    EmptyLibrarySource {
        path: PathBuf,
        name: String,
    },
    LocationUnavailable,
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Invalid {
        path: PathBuf,
        source: toml::de::Error,
    },
    DistroboxContainerMissing {
        path: PathBuf,
    },
    HostContainerUnexpected {
        path: PathBuf,
    },
    Editor {
        source: io::Error,
    },
    EditorFailed,
    Replace {
        path: PathBuf,
        source: io::Error,
    },
}

impl GlobalConfigError {
    /// Return the owning config path when the failure has a filesystem location.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::UnknownLibrary { path, .. }
            | Self::EmptyLibrarySource { path, .. }
            | Self::Io { path, .. }
            | Self::Invalid { path, .. }
            | Self::DistroboxContainerMissing { path }
            | Self::HostContainerUnexpected { path }
            | Self::Replace { path, .. } => Some(path),
            Self::LocationUnavailable | Self::Editor { .. } | Self::EditorFailed => None,
        }
    }

    /// Stable machine categories do not depend on the locale of an I/O diagnostic.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownLibrary { .. } => "unknown_library",
            Self::EmptyLibrarySource { .. } => "empty_library_source",
            Self::LocationUnavailable => "location_unavailable",
            Self::Io { .. } => "io",
            Self::Invalid { .. } => "invalid_toml",
            Self::DistroboxContainerMissing { .. } => "distrobox_container_missing",
            Self::HostContainerUnexpected { .. } => "host_container_unexpected",
            Self::Editor { .. } => "editor",
            Self::EditorFailed => "editor_failed",
            Self::Replace { .. } => "replace",
        }
    }
}

/// Resolve the standard per-user config location on the current operating system.
///
/// # Errors
/// Returns an unavailable user location when no explicit config directory is set.
pub fn config_path() -> Result<PathBuf, GlobalConfigError> {
    if let Some(directory) = std::env::var_os(CONFIG_DIRECTORY_ENV) {
        return Ok(PathBuf::from(directory).join("config.toml"));
    }
    BaseDirs::new()
        .map(|directories| directories.config_dir().join("eska").join("config.toml"))
        .ok_or(GlobalConfigError::LocationUnavailable)
}

/// Create the default config without overwriting an existing file.
///
/// # Errors
/// Rejects inaccessible parents and failed writes without replacing an existing config.
pub fn init_at(path: &Path) -> Result<InitOutcome, GlobalConfigError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(GlobalConfigError::LocationUnavailable)?;
    fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Ok(InitOutcome::Existing(path.to_owned()));
        }
        Err(source) => return Err(io_error(path, source)),
    };
    if let Err(source) = file
        .write_all(DEFAULT_CONFIG.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(io_error(path, source));
    }
    Ok(InitOutcome::Created(path.to_owned()))
}

/// Edit a temporary copy, validate it and atomically retain the previous config as backup.
///
/// # Errors
/// Preserves the previous config on editor failures, invalid settings or failed publication.
pub fn edit_at<F>(path: &Path, edit: F) -> Result<EditOutcome, GlobalConfigError>
where
    F: FnOnce(&Path) -> Result<(), GlobalConfigError>,
{
    init_at(path)?;
    let original = fs::read(path).map_err(|source| io_error(path, source))?;
    let (temporary, mut temporary_file) = create_unique_sibling(path, "edit")?;
    if let Err(source) = temporary_file
        .write_all(&original)
        .and_then(|()| temporary_file.sync_all())
    {
        drop(temporary_file);
        let _ = fs::remove_file(&temporary);
        return Err(io_error(&temporary, source));
    }
    drop(temporary_file);
    if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(&temporary, metadata.permissions())
            .map_err(|source| io_error(&temporary, source))?;
    }
    if let Err(error) = edit(&temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let edited = match fs::read(&temporary) {
        Ok(edited) => edited,
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            return Err(io_error(&temporary, source));
        }
    };
    if edited == original {
        fs::remove_file(&temporary).map_err(|source| io_error(&temporary, source))?;
        return Ok(EditOutcome::Unchanged(path.to_owned()));
    }
    let text = match std::str::from_utf8(&edited) {
        Ok(text) => text,
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            return Err(GlobalConfigError::Io {
                path: temporary,
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            });
        }
    };
    if let Err(error) = GlobalConfig::parse(path, text) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let backup = unique_backup(path)?;
    fs::rename(path, &backup).map_err(|source| io_error(path, source))?;
    if let Err(source) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temporary);
        return Err(GlobalConfigError::Replace {
            path: path.to_owned(),
            source,
        });
    }
    Ok(EditOutcome::Changed {
        path: path.to_owned(),
        backup,
    })
}

/// Create a unique sibling file and keep the reservation open for its caller.
fn create_unique_sibling(path: &Path, purpose: &str) -> Result<(PathBuf, File), GlobalConfigError> {
    for sequence in 0..1000_u16 {
        let candidate = path.with_extension(format!("toml.{purpose}-{sequence}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(io_error(&candidate, source)),
        }
    }
    Err(io_error(
        path,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no unique config path available",
        ),
    ))
}

/// Choose a timestamped backup path while preserving every earlier backup.
fn unique_backup(path: &Path) -> Result<PathBuf, GlobalConfigError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    for sequence in 0..1000_u16 {
        let suffix = if sequence == 0 {
            format!("backup-{timestamp}")
        } else {
            format!("backup-{timestamp}-{sequence}")
        };
        let candidate = path.with_extension(format!("toml.{suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io_error(
        path,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no unique backup path available",
        ),
    ))
}

fn io_error(path: &Path, source: io::Error) -> GlobalConfigError {
    GlobalConfigError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_paths_are_relative_to_the_global_file_and_keep_build_settings() {
        let path = test_path("catalog");
        let config = GlobalConfig::parse(&path, "[build]\nrunner='host'\n[libraries.ssl_3_1]\nsource='БСП с пробелами/src/cf'\n[libraries.other]\nsource=''\n").expect("catalog");
        assert_eq!(config.build.runner, RunnerKind::Host);
        assert_eq!(
            config.library_path(&path, "ssl_3_1").expect("source"),
            path.parent()
                .expect("config directory")
                .join("БСП с пробелами/src/cf"),
        );
        assert!(matches!(
            config.library_path(&path, "other"),
            Err(GlobalConfigError::EmptyLibrarySource { name, .. }) if name == "other"
        ));
        assert!(matches!(
            config.library_path(&path, "unknown"),
            Err(GlobalConfigError::UnknownLibrary { name, .. }) if name == "unknown"
        ));
        assert!(!path.exists());
    }

    #[test]
    fn missing_config_preserves_defaults_without_creating_a_catalog() {
        let path = test_path("missing-catalog");
        let config = GlobalConfig::load(&path).expect("defaults");
        assert_eq!(config.build, GlobalBuildConfig::default());
        assert!(config.libraries.is_empty());
        assert!(matches!(
            config.library_path(&path, "ssl_3_1"),
            Err(GlobalConfigError::UnknownLibrary { .. })
        ));
        assert!(!path.parent().expect("config directory").exists());
    }

    #[test]
    fn transactional_edit_accepts_libraries_and_retains_the_original() {
        let path = test_path("catalog-edit");
        init_at(&path).expect("init");
        let original = "[build]\nrunner='distrobox'\ncontainer='onec'\n";
        fs::write(&path, original).expect("build config");
        let result = edit_at(&path, |temporary| {
            fs::write(
                temporary,
                format!("{original}[libraries.ssl_3_1]\nsource='src/cf'\n"),
            )
            .map_err(|source| io_error(temporary, source))
        })
        .expect("edit catalog");
        let EditOutcome::Changed { backup, .. } = result else {
            panic!("changed")
        };
        assert_eq!(fs::read_to_string(backup).expect("backup"), original);
        let config = GlobalConfig::load(&path).expect("catalog");
        assert_eq!(config.build.container.as_deref(), Some("onec"));
        assert_eq!(config.libraries["ssl_3_1"].source, Path::new("src/cf"));
        fs::remove_dir_all(path.parent().expect("config directory")).expect("cleanup");
    }

    /// Create an isolated config path for one transactional test.
    fn test_path(name: &str) -> PathBuf {
        let base =
            std::env::var_os("ESKA_TEST_ROOT").map_or_else(std::env::temp_dir, PathBuf::from);
        let root = base.join(format!(
            "eska-global-config-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        root.join("config.toml")
    }

    #[test]
    fn init_creates_default_once_without_overwrite() {
        let path = test_path("init");
        assert!(matches!(init_at(&path), Ok(InitOutcome::Created(_))));
        fs::write(&path, "[build]\nrunner = \"host\"\n").expect("replace fixture");
        assert!(matches!(init_at(&path), Ok(InitOutcome::Existing(_))));
        assert!(
            matches!(GlobalConfig::load(&path), Ok(config) if config.build.runner == RunnerKind::Host)
        );
        fs::remove_dir_all(path.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn changed_edit_keeps_previous_config_as_backup() {
        let path = test_path("backup");
        init_at(&path).expect("initialize");
        let outcome = edit_at(&path, |temporary| {
            fs::write(temporary, "[build]\nrunner = \"host\"\n")
                .map_err(|source| io_error(temporary, source))
        })
        .expect("edit config");
        let EditOutcome::Changed { backup, .. } = outcome else {
            panic!("expected changed config");
        };
        assert_eq!(fs::read_to_string(&backup).expect("backup"), DEFAULT_CONFIG);
        assert!(
            matches!(GlobalConfig::load(&path), Ok(config) if config.build.runner == RunnerKind::Host)
        );
        fs::remove_dir_all(path.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn invalid_edit_preserves_original_without_temporary_file() {
        let path = test_path("invalid");
        init_at(&path).expect("initialize");
        assert!(matches!(
            edit_at(&path, |temporary| {
                fs::write(temporary, "not toml").map_err(|source| io_error(temporary, source))
            }),
            Err(GlobalConfigError::Invalid { .. })
        ));
        assert_eq!(fs::read_to_string(&path).expect("original"), DEFAULT_CONFIG);
        assert_eq!(
            fs::read_dir(path.parent().expect("parent"))
                .expect("directory")
                .count(),
            1
        );
        fs::remove_dir_all(path.parent().expect("parent")).expect("cleanup");
    }
}
