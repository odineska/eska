//! Compatibility import for external objects requiring configuration metadata context.

use std::{ffi::OsString, fs, io, path::Path, process::Output};

use super::{BuildError, BuildStage, Ibcmd, ProcessStream, RunError, execute};
use crate::native_path;

/// Use the already prepared infobase and the matching installation's interruptible runner.
pub(super) fn import<F>(
    tool: &Ibcmd,
    descriptor: &Path,
    data: &Path,
    artifact: &Path,
    pid: &Path,
    on_output: &mut F,
) -> Result<Output, BuildError>
where
    F: FnMut(BuildStage, ProcessStream, &[u8]),
{
    let stage = BuildStage::ImportSources;
    execute::reject_interrupted_import(tool, stage)?;
    // A failed ibcmd import may leave a partial file. Designer must produce a new artifact.
    if let Err(error) = fs::remove_file(artifact)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(run_error(error));
    }
    let result = pid.with_extension("designer.result");
    let log = pid.with_extension("designer.log");
    let args: Vec<OsString> = vec![
        "DESIGNER".into(),
        "/F".into(),
        native_path(&data.join("db-data")).as_os_str().to_owned(),
        "/LoadExternalDataProcessorOrReportFromFiles".into(),
        native_path(descriptor).as_os_str().to_owned(),
        native_path(artifact).as_os_str().to_owned(),
        "/DisableStartupDialogs".into(),
        "/DisableStartupMessages".into(),
        "/Out".into(),
        native_path(&log).as_os_str().to_owned(),
        "/DumpResult".into(),
        native_path(&result).as_os_str().to_owned(),
    ];
    let mut output = tool
        .sibling_program("1cv8")
        .run_interruptible(args, pid, &mut |stream, bytes| {
            on_output(stage, stream, bytes);
        })
        .map_err(|source| BuildError::Run { stage, source })?;
    execute::reject_interrupted_import(tool, stage)?;
    let diagnostics = fs::read(&log).unwrap_or_default();
    for line in diagnostics.split_inclusive(|byte| *byte == b'\n') {
        on_output(stage, ProcessStream::Stdout, line);
    }
    output.stdout.extend_from_slice(&diagnostics);
    // A failed process may never write /DumpResult; retain its native diagnostics first.
    if !output.status.success()
        || fs::read_to_string(&result)
            .map_err(run_error)?
            .trim_start_matches('\u{feff}')
            .trim()
            != "0"
    {
        return Err(BuildError::CommandFailed {
            stage,
            stderr: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    execute::validate_artifact(artifact)?;
    Ok(output)
}

const fn run_error(source: io::Error) -> BuildError {
    BuildError::Run {
        stage: BuildStage::ImportSources,
        source: RunError::Io(source),
    }
}
