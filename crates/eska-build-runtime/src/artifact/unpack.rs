//! Export native artifact snapshots through the existing platform runner.

use crate::native_path;
use std::{ffi::OsString, fs, io, path::Path};

use super::{
    ArtifactError, Identity, PublicationError,
    identity::inspect_identity,
    io_error,
    snapshot::{self, Snapshot},
};
use crate::build::{Ibcmd, ProcessStream};
use eska_project_core::{artifact::Staging, project::ProjectType};

/// An owned, unpacked artifact whose sources survive until publication or cancellation.
pub struct PreparedArtifact {
    staging: Staging,
    identity: Identity,
    snapshot: Snapshot,
}

impl PreparedArtifact {
    /// Unpack near the eventual destination into ignored storage and inspect metadata identity.
    ///
    /// # Errors
    /// Returns input, platform, cancellation or XML errors before any project is changed.
    pub fn unpack<F>(
        input: &Path,
        destination: &Path,
        tool: &Ibcmd,
        mut output: F,
    ) -> Result<Self, ArtifactError>
    where
        F: FnMut(ProcessStream, &[u8]),
    {
        tool.begin_interruptible_operation()
            .map_err(ArtifactError::Run)?;
        let input = fs::canonicalize(input).map_err(|error| io_error(input, error))?;
        let extension = input
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "cf" | "cfe" | "epf" | "erf") || !input.is_file() {
            return Err(ArtifactError::UnsupportedFile(input));
        }
        let staging = Staging::create(destination).map_err(super::structural_error)?;
        let snapshot = staging.path().join(format!("input.{extension}"));
        fs::copy(&input, &snapshot).map_err(|error| io_error(&input, error))?;
        let sources = staging.path().join("sources");
        let unpacked = staging.path().join("unpacked");
        fs::create_dir(&unpacked).map_err(|error| io_error(&unpacked, error))?;
        let export = unpacked.join(
            input
                .file_stem()
                .ok_or_else(|| ArtifactError::UnsupportedFile(input.clone()))?,
        );
        let data = staging.path().join("infobase");
        let pid = staging.path().join("ibcmd.pid");
        if !tool.supports_standalone_config_files() {
            run(
                tool,
                &pid,
                "create-infobase",
                vec!["infobase".into(), "create".into(), option("--data=", &data)],
                &mut output,
            )?;
        }
        export_sources(tool, &pid, &data, &snapshot, &export, &mut output)?;
        // External exports may replace a dotted filename's suffix with `.xml`.
        // Inspect the actual descriptor instead of predicting the platform's naming rules.
        let (exported, (descriptor, identity)) =
            if let Some(descriptor) = export_descriptor(&unpacked)? {
                (&unpacked, descriptor)
            } else {
                let descriptor = export_descriptor(&export)?
                    .ok_or_else(|| ArtifactError::MissingDescriptor(export.clone()))?;
                (&export, descriptor)
            };
        fs::rename(exported, &sources).map_err(|error| io_error(exported, error))?;
        if matches!(
            identity.project_type,
            ProjectType::Processing | ProjectType::Report
        ) {
            complete_external_container(&sources.join(descriptor))?;
        }
        let snapshot = snapshot::capture(&sources)?;
        Ok(Self {
            staging,
            identity,
            snapshot,
        })
    }

    /// Return the identity read from exported XML rather than the artifact filename.
    #[must_use]
    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Return the isolated source directory ready for publication.
    #[must_use]
    pub fn sources(&self) -> std::path::PathBuf {
        self.staging.path().join("sources")
    }

    /// Both import and unpack publish the same export that the platform prepared.
    pub(super) fn verified_sources(&self) -> Result<std::path::PathBuf, ArtifactError> {
        let sources = self.sources();
        if snapshot::capture(&sources)? != self.snapshot
            || inspect_identity(&sources)?.as_ref() != Some(&self.identity)
        {
            return Err(ArtifactError::SourceChanged(sources));
        }
        Ok(sources)
    }

    /// Publish only into an exclusively claimed new directory; an existing destination is preserved.
    ///
    /// No manifest, Git metadata or existing project source is replaced. The core transaction
    /// removes only the directory it claimed if moving one of the exported entries fails.
    ///
    /// # Errors
    /// Returns unsafe destinations, changed exported sources, I/O or retained rollback failures.
    pub fn publish_new(
        self,
        destination: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<std::path::PathBuf, ArtifactError> {
        if cancelled() {
            return Err(ArtifactError::Run(crate::build::RunError::Interrupted));
        }
        let destination = new_destination(destination)?;
        let sources = self.verified_sources()?;
        eska_project_core::project::create::in_new_directory(&destination, |owned| {
            let check_cancelled = || {
                if cancelled() {
                    Err(
                        eska_project_core::project::create::CreationError::Operation(
                            PublicationError::Cancelled,
                        ),
                    )
                } else {
                    Ok(())
                }
            };
            check_cancelled()?;
            let entries = fs::read_dir(&sources).map_err(publication_io)?;
            for entry in entries {
                check_cancelled()?;
                let entry = entry.map_err(publication_io)?;
                fs::rename(entry.path(), owned.path().join(entry.file_name()))
                    .map_err(publication_io)?;
            }
            check_cancelled()?;
            Ok(destination.clone())
        })
        .map_err(|source| match source {
            eska_project_core::project::create::CreationError::Operation(
                PublicationError::Cancelled,
            ) => ArtifactError::Run(crate::build::RunError::Interrupted),
            source => ArtifactError::Publication {
                path: destination,
                source: Box::new(source),
            },
        })
    }
}

/// Read only the real directory written by the platform.
fn export_descriptor(directory: &Path) -> Result<Option<(OsString, Identity)>, ArtifactError> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| io_error(directory, error))?;
    if !metadata.is_dir() {
        return Err(ArtifactError::UnsafePath(directory.to_owned()));
    }
    eska_project_core::artifact::inspect_descriptor(directory).map_err(super::structural_error)
}

/// Native export omits empty external-object directories that native import requires.
fn complete_external_container(descriptor: &Path) -> Result<(), ArtifactError> {
    let object = descriptor.with_extension("");
    ensure_export_directory(&object)?;
    ensure_export_directory(&object.join("Ext"))
}

/// Validate each component before writing so an export cannot redirect directory creation.
fn ensure_export_directory(path: &Path) -> Result<(), ArtifactError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(ArtifactError::UnsafePath(path.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| io_error(path, error))
        }
        Err(error) => Err(io_error(path, error)),
    }
}

/// Retain filesystem errors separately from the explicit cancellation branch.
const fn publication_io(
    source: std::io::Error,
) -> eska_project_core::project::create::CreationError<PublicationError> {
    eska_project_core::project::create::CreationError::Operation(PublicationError::Io(source))
}

/// Resolve a new XML destination before starting the expensive platform process.
///
/// # Errors
/// Rejects existing destinations, missing parents and paths without a final directory name.
pub fn new_destination(destination: &Path) -> Result<std::path::PathBuf, ArtifactError> {
    let name = destination
        .file_name()
        .ok_or_else(|| ArtifactError::UnsafePath(destination.into()))?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| io_error(parent, error))?;
    let destination = parent.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => Err(ArtifactError::UnsafePath(destination)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(destination),
        Err(error) => Err(io_error(&destination, error)),
    }
}

/// Export files directly on supported platforms; older CF export needs a loaded database.
fn export_sources<F>(
    tool: &Ibcmd,
    pid: &Path,
    data: &Path,
    snapshot: &Path,
    destination: &Path,
    output: &mut F,
) -> Result<(), ArtifactError>
where
    F: FnMut(ProcessStream, &[u8]),
{
    if tool.supports_standalone_config_files() {
        let direct = run(
            tool,
            pid,
            "export",
            vec![
                "config".into(),
                "export".into(),
                option("--data=", data),
                option("--file=", snapshot),
                native_path(destination).as_os_str().to_owned(),
            ],
            output,
        );
        let Err(ArtifactError::Platform {
            output: direct_output,
            ..
        }) = direct
        else {
            return direct;
        };
        if snapshot
            .extension()
            .is_none_or(|extension| extension != "cf")
        {
            return Err(ArtifactError::Platform {
                stage: "export",
                output: direct_output,
            });
        }
        // Valid CFs can still fail standalone export. Retry only this owned snapshot;
        // cancellation/process failures never enter the compatibility pipeline.
        if destination.exists() {
            fs::remove_dir_all(destination).map_err(|error| io_error(destination, error))?;
        }
        let compatibility = run(
            tool,
            pid,
            "create-infobase",
            vec!["infobase".into(), "create".into(), option("--data=", data)],
            output,
        )
        .and_then(|()| export_via_infobase(tool, pid, data, snapshot, destination, output));
        return match compatibility {
            Err(ArtifactError::Platform { stage, output }) => Err(ArtifactError::Platform {
                stage,
                output: format!("{direct_output}\n{output}"),
            }),
            result => result,
        };
    }
    export_via_infobase(tool, pid, data, snapshot, destination, output)
}

/// Keep the compatible database pipeline shared by old platforms and failed CF file exports.
fn export_via_infobase<F>(
    tool: &Ibcmd,
    pid: &Path,
    data: &Path,
    snapshot: &Path,
    destination: &Path,
    output: &mut F,
) -> Result<(), ArtifactError>
where
    F: FnMut(ProcessStream, &[u8]),
{
    let mut arguments = vec![
        "infobase".into(),
        "config".into(),
        "export".into(),
        option("--data=", data),
    ];
    if snapshot
        .extension()
        .is_some_and(|extension| extension == "cf")
    {
        run(
            tool,
            pid,
            "load-configuration",
            vec![
                "infobase".into(),
                "config".into(),
                "load".into(),
                option("--data=", data),
                native_path(snapshot).as_os_str().to_owned(),
            ],
            output,
        )?;
    } else {
        arguments.push(option("--file=", snapshot));
    }
    arguments.push(native_path(destination).as_os_str().to_owned());
    run(tool, pid, "export", arguments, output)
}

/// Construct one native argument without lossy path conversion or shell interpolation.
fn option(prefix: &str, path: &Path) -> OsString {
    let mut value = OsString::from(prefix);
    value.push(native_path(path).as_os_str());
    value
}

/// Preserve native diagnostics and stage information for the presentation layer.
fn run<F>(
    tool: &Ibcmd,
    pid: &Path,
    stage: &'static str,
    arguments: Vec<OsString>,
    output: &mut F,
) -> Result<(), ArtifactError>
where
    F: FnMut(ProcessStream, &[u8]),
{
    let result = tool
        .run_interruptible(arguments, pid, output)
        .map_err(ArtifactError::Run)?;
    if !result.status.success() {
        return Err(ArtifactError::Platform {
            stage,
            output: format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    /// Owned staging models the result of native export without pretending to test a platform.
    fn prepared(destination: &Path) -> PreparedArtifact {
        let staging = Staging::create(destination).unwrap();
        let sources = staging.path().join("sources");
        fs::create_dir(&sources).unwrap();
        fs::write(sources.join("Configuration.xml"),"<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Configuration uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>Example</Name></Properties></Configuration></MetaDataObject>\r\n").unwrap();
        fs::create_dir(sources.join("Ext")).unwrap();
        fs::write(sources.join("Ext/ManagedApplicationModule.bsl"), "A = 1;").unwrap();
        let identity = inspect_identity(&sources).unwrap().unwrap();
        let snapshot = snapshot::capture(&sources).unwrap();
        PreparedArtifact {
            staging,
            identity,
            snapshot,
        }
    }

    #[cfg(unix)]
    #[test]
    fn empty_external_export_cannot_create_a_container_through_a_symlink() {
        use std::os::unix::fs::symlink;

        let directory = TestDir::new();
        let descriptor = directory.0.join("Object.xml");
        let object = directory.0.join("Object");
        let outside = directory.0.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &object).unwrap();

        assert!(matches!(
            complete_external_container(&descriptor),
            Err(ArtifactError::UnsafePath(path)) if path == object
        ));
        assert!(!outside.join("Ext").exists());
    }

    #[test]
    fn modified_export_is_rejected_before_unpack_or_import_publication() {
        use crate::artifact::ImportPlan;
        use eska_project_core::{config::ProjectConfig, project::ProjectType};

        let directory = TestDir::new();
        let config = ProjectConfig::new(ProjectType::Configuration);
        fs::write(directory.0.join("eska.toml"), config.to_toml().unwrap()).unwrap();
        let src = directory.0.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join(".gitkeep"), "").unwrap();
        let project = config.into_project(directory.0.clone()).unwrap();
        let import = ImportPlan::inspect(&project, &|_, _, _| Ok(0)).unwrap();
        let destination = directory.0.join("export");
        let artifact = prepared(&destination);
        let sources = artifact.sources();
        fs::write(sources.join("Ext/ManagedApplicationModule.bsl"), "A = 2;").unwrap();

        assert!(
            matches!(import.apply(&artifact, true), Err(ArtifactError::SourceChanged(path)) if path == sources)
        );
        assert!(src.join(".gitkeep").is_file());
        assert_eq!(fs::read_dir(src).unwrap().count(), 1);
        assert!(
            matches!(artifact.publish_new(&destination, &|| false), Err(ArtifactError::SourceChanged(path)) if path == sources)
        );
        assert!(!destination.exists());
    }

    /// Existing directories are never replaced, including an otherwise empty destination.
    #[test]
    fn publication_claims_only_new_destinations() {
        let directory = TestDir::new();
        let destination = directory.0.join("export");
        fs::create_dir(&destination).unwrap();
        assert!(matches!(
            prepared(&destination).publish_new(&destination, &|| false),
            Err(ArtifactError::UnsafePath(_))
        ));
        assert!(destination.is_dir());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        fs::remove_dir(&destination).unwrap();
        let source = prepared(&destination);
        let bytes = fs::read(source.sources().join("Configuration.xml")).unwrap();
        assert_eq!(
            source.publish_new(&destination, &|| false).unwrap(),
            destination
        );
        assert_eq!(
            fs::read(destination.join("Configuration.xml")).unwrap(),
            bytes
        );
        assert!(!destination.join("eska.toml").exists());
        assert!(!destination.join(".git").exists());
    }

    /// Cancellation after the first move cleans only the claimed output and preserves sibling bytes.
    #[test]
    fn interrupted_publication_removes_its_partial_output() {
        let directory = TestDir::new();
        let destination = directory.0.join("export");
        fs::write(directory.0.join("keep"), b"user bytes").unwrap();
        let result = prepared(&destination).publish_new(&destination, &|| {
            destination.is_dir() && fs::read_dir(&destination).unwrap().next().is_some()
        });
        assert!(matches!(
            result,
            Err(ArtifactError::Run(crate::build::RunError::Interrupted))
        ));
        assert!(!destination.exists());
        assert_eq!(fs::read(directory.0.join("keep")).unwrap(), b"user bytes");
    }
}
