//! One known BSP dependency per external project, with machine-local installation ownership.

mod binding;
mod install;
mod write;

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    application::{ConfigurationReference, ContextError},
    config::{
        ProjectConfigError, WorkspaceConfigError,
        global::{self, GlobalConfig, GlobalConfigError},
    },
    project::{
        Project, ProjectPathError, ProjectType, Workspace,
        designer_source::{self, SourceError},
        metadata_model::ProjectScope,
    },
};
use directories::BaseDirs;
use write::Edit;

/// The initial registry is deliberately limited to two public BSP repositories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownLibrary {
    Ssl31,
    Ssl32,
}

impl KnownLibrary {
    /// Select an exact known name; arbitrary repositories are not an add-command input.
    ///
    /// # Errors
    /// Rejects unsupported names before performing any write or network operation.
    pub fn parse(name: &str) -> Result<Self, Error> {
        match name {
            "ssl_3_1" => Ok(Self::Ssl31),
            "ssl_3_2" => Ok(Self::Ssl32),
            _ => Err(Error::UnknownLibrary(name.into())),
        }
    }
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ssl31 => "ssl_3_1",
            Self::Ssl32 => "ssl_3_2",
        }
    }
    #[must_use]
    pub const fn repository(self) -> &'static str {
        match self {
            Self::Ssl31 => "https://github.com/1c-syntax/ssl_3_1.git",
            Self::Ssl32 => "https://github.com/1c-syntax/ssl_3_2.git",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    UnknownLibrary(String),
    HomeUnavailable,
    UnsupportedType(ProjectType),
    Conflict {
        path: PathBuf,
        field: String,
    },
    Changed(PathBuf),
    Locked(PathBuf),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    NonUtf8(PathBuf),
    Toml {
        path: PathBuf,
        source: toml_edit::TomlError,
    },
    Global(Box<GlobalConfigError>),
    Project(Box<ProjectConfigError>),
    Workspace(Box<WorkspaceConfigError>),
    Context(Box<ContextError>),
    Source(Box<SourceError>),
    ProjectPath(ProjectPathError),
    InvalidReceipt(PathBuf),
    Download,
    Cancelled,
    Rollback {
        path: PathBuf,
        source: Box<Self>,
    },
}

/// A complete fetch reports the exact checked-out commit, not just an advertised remote ref.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Revision(String);

impl Revision {
    /// Validate the SHA-1 commit returned by the currently supported Git transport.
    ///
    /// # Errors
    /// Rejects incomplete or invalid component replies.
    pub fn parse(value: String) -> Result<Self, Error> {
        if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(value))
        } else {
            Err(Error::Download)
        }
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct AddResult {
    pub library: KnownLibrary,
    pub source: PathBuf,
    pub revision: Option<String>,
    pub installed: bool,
    pub project_changed: bool,
    pub catalog_changed: bool,
}

/// Preflight captures the exact manifests before any long-running checkout.
pub struct AddPlan {
    library: KnownLibrary,
    binding: Edit,
    catalog: Edit,
    source: PathBuf,
    destination: Option<PathBuf>,
    roots: Vec<PathBuf>,
    inputs: Vec<Edit>,
    platform: Option<String>,
}

impl AddPlan {
    /// Validate project scope and resolve machine paths without changing any file.
    ///
    /// # Errors
    /// Rejects unsupported projects, conflicting references, invalid catalogs and unavailable homes.
    pub fn inspect(
        project: &Project,
        workspace: Option<&Workspace>,
        library: KnownLibrary,
    ) -> Result<Self, Error> {
        let home = BaseDirs::new().ok_or(Error::HomeUnavailable)?;
        let config = global::config_path().map_err(|error| Error::Global(Box::new(error)))?;
        Self::inspect_at(
            project,
            workspace,
            library,
            &home.home_dir().join(".eska/lib"),
            &config,
        )
    }

    fn inspect_at(
        project: &Project,
        workspace: Option<&Workspace>,
        library: KnownLibrary,
        library_root: &Path,
        config: &Path,
    ) -> Result<Self, Error> {
        let binding = binding::prepare(project, workspace, library)?;
        let mut catalog = Edit::read(config)?;
        let global = GlobalConfig::parse(config, catalog.text()?)
            .map_err(|error| Error::Global(Box::new(error)))?;
        let (source, destination) = if global.libraries.contains_key(library.name()) {
            (
                global
                    .library_path(config, library.name())
                    .map_err(|error| Error::Global(Box::new(error)))?,
                None,
            )
        } else {
            let destination = library_root.join(library.name());
            let source = destination.join("src/cf");
            let source_text = source
                .to_str()
                .ok_or_else(|| Error::NonUtf8(source.clone()))?;
            let mut document = catalog.document()?;
            binding::table(&mut document["libraries"])?[library.name()]["source"] =
                toml_edit::value(source_text);
            catalog.set(document.to_string().into_bytes());
            GlobalConfig::parse(config, catalog.text()?)
                .map_err(|error| Error::Global(Box::new(error)))?;
            (source, Some(destination))
        };
        let roots = workspace.map_or_else(
            || vec![project.root().to_owned()],
            |workspace| {
                workspace
                    .members()
                    .iter()
                    .map(|member| member.root().to_owned())
                    .collect()
            },
        );
        if destination.is_none() {
            validate_source(&source, &roots)?;
        }
        let inputs = workspace.map_or_else(
            || Ok(Vec::new()),
            |workspace| {
                workspace
                    .members()
                    .iter()
                    .map(|member| {
                        let edit = Edit::read(&member.root().join("eska.toml"))?;
                        if edit.before.is_none() {
                            return Err(Error::Changed(edit.path));
                        }
                        Ok(edit)
                    })
                    .collect::<Result<Vec<_>, Error>>()
            },
        )?;
        binding.writable()?;
        catalog.writable()?;
        Ok(Self {
            library,
            binding,
            catalog,
            source,
            destination,
            roots,
            inputs,
            platform: project
                .configuration()
                .build_settings()
                .platform_version()
                .map(|version| version.as_str().into()),
        })
    }

    /// Install if absent, then publish a valid catalog before attaching the project.
    ///
    /// # Errors
    /// Preserves prior files on checkout, validation, cancellation, concurrent edit and write failures.
    /// The fetch callback runs only for a new installation and never runs received scripts.
    pub fn execute<F, C>(&self, fetch: F, mut cancelled: C) -> Result<AddResult, Error>
    where
        F: FnOnce(KnownLibrary, &Path) -> Result<Revision, Error>,
        C: FnMut() -> bool,
    {
        check_cancel(&mut cancelled)?;
        let mut installation = self
            .destination
            .as_ref()
            .map(|destination| {
                install::prepare(
                    destination,
                    self.library,
                    self.platform.as_deref(),
                    fetch,
                    &mut cancelled,
                )
            })
            .transpose()?;
        let input = installation
            .as_ref()
            .map_or(self.source.as_path(), install::Prepared::source);
        validate_source(input, &self.roots)?;
        let _catalog_lock = write::lock(&self.catalog.path.with_extension("toml.eska-add.lock"))?;
        let parent = self
            .binding
            .path
            .parent()
            .ok_or_else(|| Error::Changed(self.binding.path.clone()))?;
        let _project_lock = write::lock(&parent.join(".eska/library-add.lock"))?;
        self.binding.validate()?;
        self.catalog.validate()?;
        self.validate_inputs()?;
        check_cancel(&mut cancelled)?;
        if let Some(installation) = &mut installation {
            installation.publish()?;
        }
        let result = self.publish_configuration(&mut cancelled);
        if let Err(error) = result {
            if matches!(error, Error::Rollback { .. }) {
                if let Some(installation) = &mut installation {
                    installation.keep();
                }
                return Err(error);
            }
            if let Err(source) = self.catalog.restore() {
                if let Some(installation) = &mut installation {
                    installation.keep();
                }
                return Err(Error::Rollback {
                    path: self.catalog.path.clone(),
                    source: Box::new(source),
                });
            }
            return Err(error);
        }
        let installed = installation.as_ref().is_some_and(install::Prepared::is_new);
        let revision = installation
            .as_ref()
            .map(|installation| installation.revision().as_str().to_owned());
        if let Some(installation) = &mut installation {
            installation.keep();
        }
        Ok(AddResult {
            library: self.library,
            source: self.source.clone(),
            revision,
            installed,
            project_changed: self.binding.changed(),
            catalog_changed: self.catalog.changed(),
        })
    }

    fn publish_configuration(&self, cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
        self.catalog.publish()?;
        check_cancel(cancelled)?;
        self.validate_inputs()?;
        self.binding.publish()
    }

    fn validate_inputs(&self) -> Result<(), Error> {
        for input in &self.inputs {
            input.validate()?;
        }
        Ok(())
    }
}

fn check_cancel(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn validate_source(source: &Path, roots: &[PathBuf]) -> Result<(), Error> {
    let path = fs::canonicalize(source).map_err(|error| io_error(source, error))?;
    if roots
        .iter()
        .any(|root| path.starts_with(root) || root.starts_with(&path))
    {
        return Err(Error::Conflict {
            path,
            field: "configuration".into(),
        });
    }
    let project = Project::new(
        path.clone(),
        path,
        crate::project::ProjectConfiguration::new(
            ProjectType::Configuration,
            crate::project::SourceFormat::DesignerXml,
        ),
    )
    .map_err(Error::ProjectPath)?;
    designer_source::open_analysis_project(project, ProjectScope::Standalone)
        .map_err(|error| Error::Source(Box::new(error)))?;
    Ok(())
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests;
