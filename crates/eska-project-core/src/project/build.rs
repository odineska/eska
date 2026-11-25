//! Portable build settings, without platform discovery or process execution.

use std::{
    cmp::Ordering,
    path::{Component, Path, PathBuf},
};

pub const DEFAULT_ARTIFACTS_DIRECTORY: &str = "build";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PlatformVersion {
    text: String,
    series: [u32; 3],
    build: Option<u32>,
}

impl PlatformVersion {
    /// Parse a 1C platform series or an exact installed version.
    ///
    /// # Errors
    /// Returns the original value unless it contains three or four numeric components.
    pub fn parse(value: &str) -> Result<Self, BuildSettingsError> {
        let invalid = || BuildSettingsError::InvalidPlatformVersion {
            value: value.to_owned(),
        };
        let mut components = value.split('.').map(|part| {
            if part.bytes().all(|byte| byte.is_ascii_digit()) {
                part.parse::<u32>().ok()
            } else {
                None
            }
        });
        let series = [
            components.next().flatten().ok_or_else(invalid)?,
            components.next().flatten().ok_or_else(invalid)?,
            components.next().flatten().ok_or_else(invalid)?,
        ];
        let build = if let Some(build) = components.next() {
            Some(build.ok_or_else(invalid)?)
        } else {
            None
        };
        if components.next().is_some() {
            return Err(invalid());
        }
        Ok(Self {
            text: value.to_owned(),
            series,
            build,
        })
    }

    /// Parse the full version reported by an installed platform.
    ///
    /// # Errors
    /// Returns the original value when it does not specify a build number.
    pub fn parse_exact(value: &str) -> Result<Self, BuildSettingsError> {
        let version = Self::parse(value)?;
        if version.is_series() {
            return Err(BuildSettingsError::InvalidPlatformVersion {
                value: value.to_owned(),
            });
        }
        Ok(version)
    }

    #[must_use]
    pub const fn is_series(&self) -> bool {
        self.build.is_none()
    }

    #[must_use]
    pub const fn series_components(&self) -> [u32; 3] {
        self.series
    }

    /// A series accepts any build of that series. An exact request pins the build.
    #[must_use]
    pub fn matches_installation(&self, actual: &Self) -> bool {
        !actual.is_series()
            && if self.is_series() {
                self.series_components() == actual.series_components()
            } else {
                self == actual
            }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl Ord for PlatformVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.series
            .cmp(&other.series)
            .then_with(|| self.build.cmp(&other.build))
            .then_with(|| self.text.cmp(&other.text))
    }
}

impl PartialOrd for PlatformVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BuildSettings {
    platform_version: Option<PlatformVersion>,
    artifacts_directory: PathBuf,
}

impl Default for BuildSettings {
    fn default() -> Self {
        Self {
            platform_version: None,
            artifacts_directory: PathBuf::from(DEFAULT_ARTIFACTS_DIRECTORY),
        }
    }
}

impl BuildSettings {
    /// Validate portable project build settings.
    ///
    /// # Errors
    /// An empty version remains unconfigured. Returns a structured error for a malformed
    /// non-empty platform version or unsafe artifacts path.
    pub fn new(
        platform_version: &str,
        artifacts_directory: PathBuf,
    ) -> Result<Self, BuildSettingsError> {
        let platform_version = if platform_version.is_empty() {
            None
        } else {
            Some(PlatformVersion::parse(platform_version)?)
        };
        validate_artifacts_directory(&artifacts_directory)?;
        Ok(Self {
            platform_version,
            artifacts_directory,
        })
    }

    #[must_use]
    pub const fn platform_version(&self) -> Option<&PlatformVersion> {
        self.platform_version.as_ref()
    }

    #[must_use]
    pub fn artifacts_directory(&self) -> &Path {
        &self.artifacts_directory
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidArtifactsDirectoryReason {
    Empty,
    Absolute,
    ContainsParentTraversal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildSettingsError {
    InvalidPlatformVersion {
        value: String,
    },
    InvalidArtifactsDirectory {
        path: PathBuf,
        reason: InvalidArtifactsDirectoryReason,
    },
}

/// Enforce a project-relative artifacts directory without upward traversal.
fn validate_artifacts_directory(path: &Path) -> Result<(), BuildSettingsError> {
    let reason = if path.as_os_str().is_empty() {
        Some(InvalidArtifactsDirectoryReason::Empty)
    } else if path.has_root()
        || path
            .components()
            .any(|part| matches!(part, Component::Prefix(_)))
    {
        Some(InvalidArtifactsDirectoryReason::Absolute)
    } else if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        Some(InvalidArtifactsDirectoryReason::ContainsParentTraversal)
    } else {
        None
    };
    reason.map_or(Ok(()), |reason| {
        Err(BuildSettingsError::InvalidArtifactsDirectory {
            path: path.to_owned(),
            reason,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_request_accepts_a_series_or_an_exact_build() {
        assert_eq!(
            PlatformVersion::parse("8.3.27.2325")
                .expect("valid version")
                .as_str(),
            "8.3.27.2325"
        );
        for value in [
            "",
            "8.3",
            "8.3.27.x",
            "8.3.27.2325.1",
            " 8.3.27.2325",
            "8.+5.4",
        ] {
            assert!(matches!(
                PlatformVersion::parse(value),
                Err(BuildSettingsError::InvalidPlatformVersion { .. })
            ));
        }
        let series = PlatformVersion::parse("8.5.4").expect("series");
        let first_build = PlatformVersion::parse_exact("8.5.4.2072").expect("exact version");
        let next_build = PlatformVersion::parse_exact("8.5.4.2073").expect("exact version");
        let other_series = PlatformVersion::parse_exact("8.5.1.1150").expect("exact version");
        assert!(series.matches_installation(&first_build));
        assert!(series.matches_installation(&next_build));
        assert!(!series.matches_installation(&other_series));
        assert!(!first_build.matches_installation(&next_build));
        assert!(!series.matches_installation(&series));
        assert!(PlatformVersion::parse_exact("8.5.4").is_err());
    }

    #[test]
    /// Sort version components numerically instead of lexicographically.
    fn platform_versions_have_numeric_order() {
        assert!(
            PlatformVersion::parse("8.5.10.1").expect("version")
                > PlatformVersion::parse("8.5.4.9999").expect("version")
        );
    }

    #[test]
    /// Represent an empty project value as an unconfigured build version.
    fn empty_build_version_is_unconfigured() {
        let settings = BuildSettings::new("", PathBuf::from("build")).expect("settings");
        assert_eq!(settings.platform_version(), None);
    }

    #[test]
    /// Keep configured artifacts inside the project-relative namespace.
    fn artifacts_directory_is_project_relative_without_parent_traversal() {
        assert!(BuildSettings::new("8.3.27.2325", PathBuf::from("artifacts/onec")).is_ok());
        for path in [
            PathBuf::new(),
            PathBuf::from("/build"),
            PathBuf::from("build/../out"),
        ] {
            assert!(matches!(
                BuildSettings::new("8.3.27.2325", path),
                Err(BuildSettingsError::InvalidArtifactsDirectory { .. })
            ));
        }
    }
}
