//! Filesystem-backed project discovery, independent of CLI presentation.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::config::{
    FILE_NAME, ManifestConfig, ManifestConfigError, ProjectConfig, ProjectConfigError,
    WorkspaceConfig,
};

use super::{
    Project, ProjectConfiguration, ProjectName, Workspace, WorkspaceMember,
    build::{BuildSettings, BuildSettingsError},
};

/// The validated project context found from a starting directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveryContext {
    Standalone(Project),
    Workspace {
        workspace: Workspace,
        current_member: Option<ProjectName>,
    },
}

/// Finds and validates either a standalone project or its containing workspace.
///
/// A workspace root validates every explicitly listed member. Starting within a
/// member additionally records that member as the current selection context.
///
/// # Errors
/// Returns a structured project, manifest, member layout, or inheritance error.
pub fn discover_context(start: &Path) -> Result<DiscoveryContext, ContextDiscoveryError> {
    let start = canonicalize_directory(start).map_err(ContextDiscoveryError::Project)?;
    find_context(&start, None)?.ok_or(ContextDiscoveryError::Project(DiscoveryError::NotFound {
        start,
    }))
}

/// Find a project or workspace without searching above an inclusive directory boundary.
///
/// Both paths are canonicalized. A missing manifest inside the boundary returns `None`.
/// An existing invalid manifest remains authoritative.
///
/// # Errors
/// Returns discovery errors, including invalid input when `start` is outside `boundary`.
pub fn discover_context_within(
    start: &Path,
    boundary: &Path,
) -> Result<Option<DiscoveryContext>, ContextDiscoveryError> {
    let (start, boundary) =
        bounded_directories(start, boundary).map_err(ContextDiscoveryError::Project)?;
    find_context(&start, Some(&boundary))
}

fn find_context(
    start: &Path,
    boundary: Option<&Path>,
) -> Result<Option<DiscoveryContext>, ContextDiscoveryError> {
    let Some((root, path)) =
        nearest_manifest(start.ancestors(), boundary).map_err(ContextDiscoveryError::Project)?
    else {
        return Ok(None);
    };
    resolve_manifest_within(root, &path, load_manifest(&path)?, boundary).map(Some)
}

/// Validate a candidate at its existing project root without first writing it to disk.
pub(crate) fn resolve_manifest(
    nearest_root: &Path,
    nearest_path: &Path,
    nearest: ManifestConfig,
) -> Result<DiscoveryContext, ContextDiscoveryError> {
    resolve_manifest_within(nearest_root, nearest_path, nearest, None)
}

fn resolve_manifest_within(
    nearest_root: &Path,
    nearest_path: &Path,
    nearest: ManifestConfig,
    boundary: Option<&Path>,
) -> Result<DiscoveryContext, ContextDiscoveryError> {
    match nearest {
        ManifestConfig::Workspace(config) => {
            load_workspace(nearest_root, &config, None).map(|workspace| {
                DiscoveryContext::Workspace {
                    workspace,
                    current_member: None,
                }
            })
        }
        ManifestConfig::Project(config) => {
            if let Some((ancestor, path)) =
                nearest_manifest(nearest_root.ancestors().skip(1), boundary)
                    .map_err(ContextDiscoveryError::Project)?
                && let ManifestConfig::Workspace(workspace_config) = load_manifest(&path)?
            {
                let workspace =
                    load_workspace(ancestor, &workspace_config, Some((nearest_root, &config)))?;
                let current_member = workspace
                    .members()
                    .iter()
                    .find(|member| member.root() == nearest_root)
                    .map(|member| member.name().clone())
                    .ok_or_else(|| ContextDiscoveryError::UnlistedProject {
                        project: nearest_root.to_path_buf(),
                        workspace: ancestor.to_path_buf(),
                    })?;
                return Ok(DiscoveryContext::Workspace {
                    workspace,
                    current_member: Some(current_member),
                });
            }
            load_project_config(nearest_root, nearest_path, config)
                .map(DiscoveryContext::Standalone)
                .map_err(ContextDiscoveryError::Project)
        }
    }
}

fn nearest_manifest<'a>(
    ancestors: impl Iterator<Item = &'a Path>,
    boundary: Option<&Path>,
) -> Result<Option<(&'a Path, PathBuf)>, DiscoveryError> {
    for root in ancestors.take_while(|root| boundary.is_none_or(|bound| root.starts_with(bound))) {
        let path = root.join(FILE_NAME);
        // A dangling link is a broken manifest, so its error must stop discovery.
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(Some((root, path))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(DiscoveryError::Io { path, source }),
        }
    }
    Ok(None)
}

fn load_manifest(path: &Path) -> Result<ManifestConfig, ContextDiscoveryError> {
    if !metadata(path)
        .map_err(ContextDiscoveryError::Project)?
        .is_file()
    {
        return Err(ContextDiscoveryError::Project(
            DiscoveryError::ConfigNotFile {
                path: path.to_path_buf(),
            },
        ));
    }
    ManifestConfig::load(path).map_err(|source| ContextDiscoveryError::Manifest {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

fn load_workspace(
    root: &Path,
    config: &WorkspaceConfig,
    candidate: Option<(&Path, &ProjectConfig)>,
) -> Result<Workspace, ContextDiscoveryError> {
    let mut members: Vec<WorkspaceMember> = Vec::with_capacity(config.members().len());
    for configured_path in config.members() {
        let member_root = validate_member_root(root, configured_path, &members)?;
        let override_config = candidate
            .filter(|(path, _)| *path == member_root)
            .map(|(_, config)| config);
        let member = load_workspace_member(config, member_root, override_config)?;
        if members
            .iter()
            .any(|existing| existing.name() == member.name())
        {
            return Err(ContextDiscoveryError::DuplicateName {
                name: member.name().clone(),
            });
        }
        members.push(member);
    }

    let analysis =
        config
            .resolve_analysis(&members)
            .map_err(|source| ContextDiscoveryError::Manifest {
                path: root.join(FILE_NAME),
                source: Box::new(ManifestConfigError::Workspace(source)),
            })?;
    Ok(Workspace::new(
        root.to_path_buf(),
        config.build_settings().clone(),
        config.workflow_settings().cloned(),
        members,
        analysis,
    ))
}

fn validate_member_root(
    workspace_root: &Path,
    configured_path: &Path,
    existing_members: &[WorkspaceMember],
) -> Result<PathBuf, ContextDiscoveryError> {
    let unresolved_root = workspace_root.join(configured_path);
    if !metadata(&unresolved_root)
        .map_err(ContextDiscoveryError::Project)?
        .is_dir()
    {
        return Err(ContextDiscoveryError::MemberNotDirectory {
            path: unresolved_root,
        });
    }
    let member_root = canonicalize(&unresolved_root).map_err(ContextDiscoveryError::Project)?;
    if !member_root.starts_with(workspace_root) {
        return Err(ContextDiscoveryError::MemberOutsideWorkspace {
            workspace: workspace_root.to_path_buf(),
            member: member_root,
        });
    }
    for existing in existing_members {
        if existing.root() == member_root {
            return Err(ContextDiscoveryError::DuplicateMember { path: member_root });
        }
        if existing.root().starts_with(&member_root) || member_root.starts_with(existing.root()) {
            return Err(ContextDiscoveryError::NestedMembers {
                first: existing.root().to_path_buf(),
                second: member_root,
            });
        }
    }
    Ok(member_root)
}

fn load_workspace_member(
    workspace_config: &WorkspaceConfig,
    member_root: PathBuf,
    candidate: Option<&ProjectConfig>,
) -> Result<WorkspaceMember, ContextDiscoveryError> {
    let manifest_path = member_root.join(FILE_NAME);
    let loaded;
    let member_config = if let Some(config) = candidate {
        config
    } else {
        loaded = match load_manifest(&manifest_path)? {
            ManifestConfig::Project(config) => config,
            ManifestConfig::Workspace(_) => {
                return Err(ContextDiscoveryError::MemberManifestNotProject {
                    path: manifest_path,
                });
            }
        };
        &loaded
    };
    let name =
        member_config
            .name()
            .cloned()
            .ok_or_else(|| ContextDiscoveryError::MemberNameMissing {
                path: manifest_path.clone(),
            })?;
    if member_config.configuration().workflow_settings().is_some() {
        return Err(ContextDiscoveryError::MemberWorkflowUnsupported {
            path: manifest_path,
        });
    }
    if member_config.overrides_artifacts_directory() {
        return Err(ContextDiscoveryError::MemberArtifactsDirectoryUnsupported {
            path: manifest_path,
        });
    }

    let configuration = resolve_member_configuration(workspace_config, member_config)?;
    let source = member_root.join(member_config.source());
    let project = Project::new(member_root.clone(), source, configuration)
        .map_err(ProjectConfigError::ProjectPath)
        .map_err(|source| {
            ContextDiscoveryError::Project(DiscoveryError::Config {
                path: member_root.join(FILE_NAME),
                source,
            })
        })?;
    if !metadata(project.source())
        .map_err(ContextDiscoveryError::Project)?
        .is_dir()
    {
        return Err(ContextDiscoveryError::Project(
            DiscoveryError::SourceNotDirectory {
                path: project.source().to_path_buf(),
            },
        ));
    }
    let source = canonicalize(project.source()).map_err(ContextDiscoveryError::Project)?;
    let project = Project::new(member_root, source, project.configuration().clone())
        .map_err(ProjectConfigError::ProjectPath)
        .map_err(|source| {
            ContextDiscoveryError::Project(DiscoveryError::Config {
                path: manifest_path,
                source,
            })
        })?;
    Ok(WorkspaceMember::new(name, project))
}

fn resolve_member_configuration(
    workspace: &WorkspaceConfig,
    member: &ProjectConfig,
) -> Result<ProjectConfiguration, ContextDiscoveryError> {
    let version = member
        .platform_version_override()
        .or_else(|| {
            workspace
                .build_settings()
                .platform_version()
                .map(crate::project::build::PlatformVersion::as_str)
        })
        .unwrap_or_default();
    let build = BuildSettings::new(
        version,
        workspace
            .build_settings()
            .artifacts_directory()
            .to_path_buf(),
    )
    .map_err(ContextDiscoveryError::ResolvedBuild)?;
    let mut configuration = ProjectConfiguration::new(
        member.configuration().project_type(),
        member.configuration().source_format(),
    )
    .with_analysis_settings(member.configuration().analysis_settings().clone())
    .with_formatting_settings(member.configuration().formatting_settings().clone())
    .with_build_settings(build);
    if let Some(workflow) = workspace.workflow_settings() {
        configuration = configuration.with_workflow_settings(workflow.clone());
    }
    Ok(configuration)
}

/// Finds and validates the nearest project starting from a directory.
///
/// Relative paths are resolved against the current working directory. Symlinks
/// are resolved before walking ancestors and before checking source containment.
/// The first configuration encountered is authoritative, even when invalid.
///
/// # Errors
///
/// Returns a structured error for an inaccessible or invalid directory, a missing
/// project, invalid configuration, or sources outside the project root.
pub fn discover(start: &Path) -> Result<Project, DiscoveryError> {
    let start = canonicalize_directory(start)?;
    find_project(&start, None)?.ok_or(DiscoveryError::NotFound { start })
}

/// Find the nearest standalone project manifest inside the inclusive boundary.
///
/// # Errors
/// Returns the same project validation errors as [`discover`], or an error when
/// `start` is outside the boundary.
pub fn discover_within(start: &Path, boundary: &Path) -> Result<Option<Project>, DiscoveryError> {
    let (start, boundary) = bounded_directories(start, boundary)?;
    find_project(&start, Some(&boundary))
}

fn find_project(start: &Path, boundary: Option<&Path>) -> Result<Option<Project>, DiscoveryError> {
    nearest_manifest(start.ancestors(), boundary)?
        .map(|(root, path)| load_project(root, &path))
        .transpose()
}

fn bounded_directories(
    start: &Path,
    boundary: &Path,
) -> Result<(PathBuf, PathBuf), DiscoveryError> {
    let start = canonicalize_directory(start)?;
    let boundary = canonicalize_directory(boundary)?;
    if !start.starts_with(&boundary) {
        return Err(DiscoveryError::Io {
            path: start,
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "start is outside discovery boundary",
            ),
        });
    }
    Ok((start, boundary))
}

fn canonicalize_directory(path: &Path) -> Result<PathBuf, DiscoveryError> {
    let path = canonicalize(path)?;
    if !metadata(&path)?.is_dir() {
        return Err(DiscoveryError::StartNotDirectory { path });
    }
    Ok(path)
}

fn load_project(root: &Path, path: &Path) -> Result<Project, DiscoveryError> {
    // Reject directories and special files before reading them (e.g. FIFOs).
    if !metadata(path)?.is_file() {
        return Err(DiscoveryError::ConfigNotFile {
            path: path.to_path_buf(),
        });
    }

    let config_error = |source| DiscoveryError::Config {
        path: path.to_path_buf(),
        source,
    };
    let config = ProjectConfig::load(path).map_err(config_error)?;
    load_project_config(root, path, config)
}

fn load_project_config(
    root: &Path,
    path: &Path,
    config: ProjectConfig,
) -> Result<Project, DiscoveryError> {
    let config_error = |source| DiscoveryError::Config {
        path: path.to_path_buf(),
        source,
    };
    let project = config
        .into_project(root.to_path_buf())
        .map_err(config_error)?;

    if !metadata(project.source())?.is_dir() {
        return Err(DiscoveryError::SourceNotDirectory {
            path: project.source().to_path_buf(),
        });
    }
    let source = canonicalize(project.source())?;
    Project::new(root.to_path_buf(), source, project.configuration().clone())
        .map_err(ProjectConfigError::ProjectPath)
        .map_err(config_error)
}

fn canonicalize(path: &Path) -> Result<PathBuf, DiscoveryError> {
    fs::canonicalize(path).map_err(|source| DiscoveryError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn metadata(path: &Path) -> Result<fs::Metadata, DiscoveryError> {
    fs::metadata(path).map_err(|source| DiscoveryError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Structured discovery failures; localized text belongs to the CLI layer.
#[derive(Debug)]
pub enum DiscoveryError {
    NotFound {
        start: PathBuf,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    StartNotDirectory {
        path: PathBuf,
    },
    ConfigNotFile {
        path: PathBuf,
    },
    SourceNotDirectory {
        path: PathBuf,
    },
    Config {
        path: PathBuf,
        source: ProjectConfigError,
    },
}

/// Structured discovery failures specific to project workspaces.
#[derive(Debug)]
pub enum ContextDiscoveryError {
    Project(DiscoveryError),
    Manifest {
        path: PathBuf,
        source: Box<ManifestConfigError>,
    },
    MemberNotDirectory {
        path: PathBuf,
    },
    MemberOutsideWorkspace {
        workspace: PathBuf,
        member: PathBuf,
    },
    DuplicateMember {
        path: PathBuf,
    },
    NestedMembers {
        first: PathBuf,
        second: PathBuf,
    },
    MemberManifestNotProject {
        path: PathBuf,
    },
    MemberNameMissing {
        path: PathBuf,
    },
    DuplicateName {
        name: ProjectName,
    },
    MemberWorkflowUnsupported {
        path: PathBuf,
    },
    MemberArtifactsDirectoryUnsupported {
        path: PathBuf,
    },
    UnlistedProject {
        project: PathBuf,
        workspace: PathBuf,
    },
    ResolvedBuild(BuildSettingsError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    const PROJECT: &str = "[project]\nname = 'member'\ntype = 'report'\nsource = '.'\n";
    const WORKSPACE: &str = "[workspace]\n[workspace.members.app]\nexternal_objects = ['member']\n";

    #[test]
    fn bounded_discovery_stops_both_searches_at_the_repository_root() {
        let fixture = TestDir::new();
        fs::write(fixture.0.join(FILE_NAME), "invalid parent manifest").expect("parent");
        let repository = fixture.0.join("nested-git");
        let nested = repository.join("nested");
        fs::create_dir_all(repository.join(".git")).expect("Git boundary");
        fs::create_dir(&nested).expect("nested directory");
        assert!(
            discover_context_within(&nested, &repository)
                .expect("missing")
                .is_none()
        );
        fs::write(repository.join(FILE_NAME), PROJECT).expect("project");
        assert!(matches!(
            discover_context_within(&nested, &repository).expect("bounded project"),
            Some(DiscoveryContext::Standalone(project)) if project.root() == repository
        ));
        assert!(
            discover_context(&nested).is_err(),
            "unbounded parent validation stays intact"
        );
    }

    #[test]
    fn bounded_workspace_discovery_includes_boundary_and_validates_members() {
        let fixture = TestDir::new();
        let root = &fixture.0;
        let member = root.join("member");
        fs::create_dir(&member).expect("member");
        fs::write(root.join(FILE_NAME), WORKSPACE).expect("workspace");
        fs::write(member.join(FILE_NAME), PROJECT).expect("project");
        assert!(matches!(
            discover_context_within(root, root).expect("workspace"),
            Some(DiscoveryContext::Workspace { workspace, current_member: None })
                if workspace.members().len() == 1
        ));
        assert!(matches!(
            discover_context_within(&member, root).expect("member workspace"),
            Some(DiscoveryContext::Workspace {
                current_member: Some(_),
                ..
            })
        ));
        assert!(matches!(
            discover_context_within(&member, &member).expect("nested Git boundary"),
            Some(DiscoveryContext::Standalone(_))
        ));
        fs::write(member.join(FILE_NAME), "broken = [").expect("invalid member");
        assert!(discover_context_within(root, root).is_err());
    }

    #[test]
    fn bounded_discovery_rejects_invalid_nearest_manifests_and_outside_start() {
        let fixture = TestDir::new();
        let root = fixture.0.join("repository");
        fs::create_dir(&root).expect("repository");
        assert!(discover_context_within(&fixture.0, &root).is_err());
        fs::create_dir(root.join(FILE_NAME)).expect("manifest directory");
        assert!(matches!(
            discover_context_within(&root, &root),
            Err(ContextDiscoveryError::Project(
                DiscoveryError::ConfigNotFile { .. }
            ))
        ));
        fs::remove_dir(root.join(FILE_NAME)).expect("remove manifest directory");
        fs::write(root.join(FILE_NAME), "broken = [").expect("invalid manifest");
        assert!(matches!(
            discover_context_within(&root, &root),
            Err(ContextDiscoveryError::Manifest { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_discovery_does_not_skip_a_dangling_manifest() {
        let fixture = TestDir::new();
        std::os::unix::fs::symlink("missing", fixture.0.join(FILE_NAME))
            .expect("dangling manifest");
        assert!(matches!(
            discover_context_within(&fixture.0, &fixture.0),
            Err(ContextDiscoveryError::Project(DiscoveryError::Io { .. }))
        ));
    }

    #[test]
    fn bounded_single_project_ignores_parent_manifest_and_rejects_a_manifest_directory() {
        let fixture = TestDir::new();
        fs::write(fixture.0.join(FILE_NAME), "invalid parent TOML").expect("parent manifest");
        let root = fixture.0.join("clone");
        fs::create_dir(&root).expect("clone root");

        assert!(discover_within(&root, &root).expect("root check").is_none());

        let manifest = root.join(FILE_NAME);
        fs::create_dir(&manifest).expect("manifest directory");
        assert!(matches!(
            discover_within(&root, &root),
            Err(DiscoveryError::ConfigNotFile { path }) if path == manifest
        ));
    }
}
