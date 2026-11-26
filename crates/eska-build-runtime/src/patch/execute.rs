//! Isolated patch generation; only a checked candidate can become a final artifact.

use super::{PatchDescription, PatchError, extension};
use crate::build::Ibcmd;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

type Observer<'a> = &'a dyn Fn(&'static str, Option<crate::progress::Signal>);

struct Workspace<'a> {
    path: PathBuf,
    notify: Observer<'a>,
}

impl Drop for Workspace<'_> {
    /// Delete only the uniquely claimed workspace, including incomplete platform databases.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Build and validate a patch against its committed baseline without touching a user's infobase.
///
/// # Errors
/// Returns errors before publication for platform, source, validation or output failures.
pub fn execute(
    plan: &PatchDescription,
    tool: &Ibcmd,
    output: &Path,
    prepare: impl FnOnce(&Path) -> Result<(), PatchError>,
) -> Result<(), PatchError> {
    execute_observed(plan, tool, output, prepare, |_, _| {})
}

/// Build a patch while reporting verified phases and sanitized native activity.
/// # Errors
/// Returns the same preparation, validation and publication failures as [`execute`].
pub fn execute_observed(
    plan: &PatchDescription,
    tool: &Ibcmd,
    output: &Path,
    prepare: impl FnOnce(&Path) -> Result<(), PatchError>,
    progress: impl FnMut(&'static str, Option<crate::progress::Signal>),
) -> Result<(), PatchError> {
    let progress = std::cell::RefCell::new(progress);
    let observer = |stage, signal| (progress.borrow_mut())(stage, signal);
    validate_output(plan, tool, output)?;
    let parent = output
        .parent()
        .ok_or_else(|| PatchError::new("output", output.display().to_string()))?;
    fs::create_dir_all(parent).map_err(|e| PatchError::new("io", e.to_string()))?;
    let workspace = workspace(parent, &observer)?;
    tool.begin_interruptible_operation()
        .map_err(|e| PatchError::new("tool", format!("{e:?}")))?;
    (workspace.notify)("prepare_base", None);
    prepare(&workspace.path)?;
    let base = workspace.path.join("base");
    let candidate = build_candidate(plan, tool, &workspace, &base)?;
    if tool.was_interrupted() {
        return Err(PatchError::new("tool", "interrupted"));
    }
    (workspace.notify)("publish_artifact", None);
    fs::hard_link(candidate, output).map_err(|e| PatchError::new("output", e.to_string()))
}

/// Reject unsupported plans and unsafe publication paths before any write.
fn validate_output(plan: &PatchDescription, tool: &Ibcmd, output: &Path) -> Result<(), PatchError> {
    plan.validate()?;
    if plan.modules.is_empty() {
        return Err(PatchError::new("empty", ""));
    }
    // Compatibility is established by every required operation on the isolated database,
    // not a release-number allowlist. Fail before staging when the paired Designer is absent.
    if !tool
        .designer_available()
        .map_err(|e| PatchError::new("tool", e.to_string()))?
    {
        return Err(PatchError::new("tool", "1cv8"));
    }
    if output.extension() != Some(std::ffi::OsStr::new("cfe")) || output.exists() {
        return Err(PatchError::new("output", output.display().to_string()));
    }
    for ancestor in output.ancestors() {
        if let Ok(metadata) = fs::symlink_metadata(ancestor)
            && metadata.file_type().is_symlink()
        {
            return Err(PatchError::new("output", ancestor.display().to_string()));
        }
    }
    Ok(())
}

/// Produce and validate an unpublished candidate in the temporary database.
fn build_candidate(
    plan: &PatchDescription,
    tool: &Ibcmd,
    workspace: &Workspace,
    base: &Path,
) -> Result<PathBuf, PatchError> {
    let data = workspace.path.join("data");
    let extension = workspace.path.join("extension");
    let candidate = workspace.path.join("candidate.cfe");
    let data_arg = option("--data", &data);
    let extension_arg = OsString::from(format!("--extension={}", plan.name));
    prepare_infobase(tool, workspace, base, &data_arg)?;
    create_extension(tool, workspace, plan, &data_arg, &extension_arg, &extension)?;
    validate_extension(
        tool,
        workspace,
        &data,
        &plan.name,
        &data_arg,
        &extension_arg,
    )?;
    save_candidate(tool, workspace, &data_arg, &extension_arg, &candidate)?;
    if fs::metadata(&candidate)
        .map_err(|e| PatchError::new("io", e.to_string()))?
        .len()
        == 0
    {
        return Err(PatchError::new("empty", ""));
    }
    Ok(candidate)
}

/// Create an isolated infobase from the exact committed baseline.
fn prepare_infobase(
    tool: &Ibcmd,
    workspace: &Workspace,
    base: &Path,
    data_arg: &OsString,
) -> Result<(), PatchError> {
    run(
        tool,
        workspace,
        "create",
        vec!["infobase".into(), "create".into(), data_arg.clone()],
    )?;
    run(
        tool,
        workspace,
        "import-base",
        vec![
            "config".into(),
            "import".into(),
            data_arg.clone(),
            crate::native_path(base).as_os_str().to_owned(),
        ],
    )?;
    run(
        tool,
        workspace,
        "apply-base",
        vec!["config".into(), "apply".into(), data_arg.clone()],
    )
}

/// Create the platform skeleton and replace it with the generated extension sources.
fn create_extension(
    tool: &Ibcmd,
    workspace: &Workspace,
    plan: &PatchDescription,
    data_arg: &OsString,
    extension_arg: &OsString,
    extension: &Path,
) -> Result<(), PatchError> {
    run(
        tool,
        workspace,
        "create-extension",
        vec![
            "extension".into(),
            "create".into(),
            data_arg.clone(),
            format!("--name={}", plan.name).into(),
            format!("--name-prefix={}_", plan.name).into(),
            "--purpose=patch".into(),
        ],
    )?;
    run(
        tool,
        workspace,
        "export-extension",
        vec![
            "config".into(),
            "export".into(),
            data_arg.clone(),
            extension_arg.clone(),
            crate::native_path(extension).as_os_str().to_owned(),
        ],
    )?;
    extension::write(plan, &workspace.path.join("modules"), extension)?;
    run(
        tool,
        workspace,
        "import-extension",
        vec![
            "config".into(),
            "import".into(),
            data_arg.clone(),
            extension_arg.clone(),
            crate::native_path(extension).as_os_str().to_owned(),
        ],
    )
}

/// Run all platform checks required before publishing the candidate.
fn validate_extension(
    tool: &Ibcmd,
    workspace: &Workspace,
    data: &Path,
    name: &str,
    data_arg: &OsString,
    extension_arg: &OsString,
) -> Result<(), PatchError> {
    run(
        tool,
        workspace,
        "check-extension",
        vec![
            "config".into(),
            "check".into(),
            data_arg.clone(),
            extension_arg.clone(),
        ],
    )?;
    run(
        tool,
        workspace,
        "apply-extension",
        vec![
            "config".into(),
            "apply".into(),
            data_arg.clone(),
            extension_arg.clone(),
        ],
    )?;
    designer(
        tool,
        workspace,
        data,
        name,
        "bsl",
        vec!["/CheckConfig", "-Server", "-ExternalConnection"],
    )?;
    designer(
        tool,
        workspace,
        data,
        name,
        "applicability",
        vec!["/CheckCanApplyConfigurationExtensions"],
    )
}

/// Serialize the validated extension to its unpublished candidate path.
fn save_candidate(
    tool: &Ibcmd,
    workspace: &Workspace,
    data_arg: &OsString,
    extension_arg: &OsString,
    candidate: &Path,
) -> Result<(), PatchError> {
    run(
        tool,
        workspace,
        "save",
        vec![
            "config".into(),
            "save".into(),
            data_arg.clone(),
            extension_arg.clone(),
            crate::native_path(candidate).as_os_str().to_owned(),
        ],
    )
}

/// Claim a new staging directory beside the artifact without reusing another process's files.
fn workspace<'a>(parent: &Path, observer: Observer<'a>) -> Result<Workspace<'a>, PatchError> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| PatchError::new("io", e.to_string()))?
        .as_nanos();
    for attempt in 0..32 {
        let path = parent.join(format!(
            ".eska-patch-{}-{time}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                return Ok(Workspace {
                    path,
                    notify: observer,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(PatchError::new("io", e.to_string())),
        }
    }
    Err(PatchError::new("io", parent.display().to_string()))
}

/// Run one platform stage and retain diagnostic output on failure.
fn run(
    tool: &Ibcmd,
    workspace: &Workspace,
    stage: &str,
    args: Vec<OsString>,
) -> Result<(), PatchError> {
    let phase = match stage {
        "create" => "create_infobase",
        "create-extension" | "export-extension" => "prepare_sources",
        "import-base" | "import-extension" => "import",
        "apply-base" | "apply-extension" => "apply",
        "check-extension" => "verify",
        "save" => "build",
        _ => "patch",
    };
    (workspace.notify)(phase, None);
    let result = run_observed(tool, workspace, phase, args)
        .map_err(|e| PatchError::new("tool", format!("{stage}: {e:?}")))?;
    if !result.status.success() {
        return Err(PatchError::new(
            "tool",
            format!(
                "{stage}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ),
        ));
    }
    Ok(())
}

/// Check both process status and Designer's independent result file.
fn designer(
    tool: &Ibcmd,
    workspace: &Workspace,
    data: &Path,
    name: &str,
    stage: &str,
    checks: Vec<&str>,
) -> Result<(), PatchError> {
    let log = workspace.path.join(format!("{stage}.log"));
    let result_file = workspace.path.join(format!("{stage}.result"));
    let mut args: Vec<OsString> = vec![
        "DESIGNER".into(),
        "/F".into(),
        crate::native_path(&data.join("db-data"))
            .as_os_str()
            .to_owned(),
        "/DisableStartupDialogs".into(),
        "/DisableStartupMessages".into(),
        "/Out".into(),
        crate::native_path(&log).as_os_str().to_owned(),
        "/DumpResult".into(),
        crate::native_path(&result_file).as_os_str().to_owned(),
    ];
    args.extend(checks.into_iter().map(OsString::from));
    args.extend(["-Extension".into(), name.into()]);
    (workspace.notify)("verify", None);
    let output = run_observed(&tool.sibling_program("1cv8"), workspace, "verify", args)
        .map_err(|e| PatchError::new("tool", format!("{e:?}")))?;
    if !output.status.success()
        || fs::read_to_string(result_file)
            .unwrap_or_default()
            .trim_start_matches('\u{feff}')
            .trim()
            != "0"
    {
        return Err(PatchError::new(
            "validation",
            fs::read_to_string(log)
                .unwrap_or_else(|_| String::from_utf8_lossy(&output.stderr).into_owned()),
        ));
    }
    Ok(())
}

fn run_observed(
    tool: &Ibcmd,
    workspace: &Workspace<'_>,
    phase: &'static str,
    arguments: Vec<OsString>,
) -> Result<std::process::Output, crate::build::RunError> {
    let mut decoder = crate::progress::Decoder::default();
    let output = tool.run_owned_bounded(
        arguments,
        &workspace.path.join("process.pid"),
        None,
        None,
        &mut |stream, bytes| {
            decoder.push(stream, bytes, &mut |signal| {
                (workspace.notify)(
                    phase,
                    Some(signal.unwrap_or(crate::progress::Signal::Activity)),
                );
            });
        },
        (None, 1024 * 1024, None),
    );
    decoder.finish(&mut |signal| {
        (workspace.notify)(
            phase,
            Some(signal.unwrap_or(crate::progress::Signal::Activity)),
        );
    });
    output
}

/// Preserve arbitrary filesystem bytes as a single option value.
fn option(name: &str, path: &Path) -> OsString {
    let mut value = OsString::from(name);
    value.push("=");
    value.push(crate::native_path(path).as_os_str());
    value
}
