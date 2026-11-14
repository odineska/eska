//! Establish a first snapshot from a verified, already applied file infobase.

use std::ffi::OsString;

use super::{
    ApplyError, Baseline, Connection, Observation, ReviewReason, Stage, Target, descriptor, lock,
    native_path, option, persist_baseline, read_baseline, read_pending, source_snapshot, structure,
};

impl Target {
    pub(super) fn preview_baseline(
        &self,
        observation: Option<&Observation>,
    ) -> Result<Result<Baseline, ReviewReason>, ApplyError> {
        let baseline = read_baseline(&self.state)?;
        if !matches!(baseline, Err(ReviewReason::BaselineRequired))
            || self.extension.is_some()
            || !matches!(self.connection, Connection::File(_))
            || read_pending(&self.state)?.is_some()
        {
            return Ok(baseline);
        }
        let Some(observation) = observation else {
            return Ok(baseline);
        };
        let _lock = lock(&self.lock)?;
        // Another preview may have established the snapshot while we were observing the base.
        let baseline = read_baseline(&self.state)?;
        if !matches!(baseline, Err(ReviewReason::BaselineRequired))
            || read_pending(&self.state)?.is_some()
        {
            return Ok(baseline);
        }
        match self.export_baseline(observation) {
            Ok(Some(document)) => {
                persist_baseline(&self.state, &document)?;
                Ok(Ok(document))
            }
            // Unsupported inspection never turns into an implicit database write.
            Ok(None) | Err(ApplyError::PlatformFailed(Stage::Inspect)) => Ok(baseline),
            Err(error) => Err(error),
        }
    }

    fn export_baseline(&self, observation: &Observation) -> Result<Option<Baseline>, ApplyError> {
        let database = self.scratch.path().join("database.cf");
        self.command(
            &["save"],
            &[
                OsString::from("--db"),
                native_path(&database).into_owned().into_os_string(),
            ],
            None,
            None,
            Stage::Inspect,
        )?;
        let export = self.scratch.path().join("database-source");
        self.command(
            &["export"],
            &[
                option("--file", &database),
                native_path(&export).into_owned().into_os_string(),
            ],
            None,
            None,
            Stage::Inspect,
        )?;
        let main = self.scratch.path().join("main.cf");
        self.command(
            &["save"],
            &[native_path(&main).into_owned().into_os_string()],
            None,
            None,
            Stage::Inspect,
        )?;
        // XML export alone reads the editable configuration, which may not be applied.
        // Equal native dumps and an unchanged generation bind this snapshot to both states.
        if source_snapshot::file_digest(&database, None).map_err(ApplyError::Snapshot)?
            != source_snapshot::file_digest(&main, None).map_err(ApplyError::Snapshot)?
            || self.observe()? != *observation
        {
            return Ok(None);
        }
        let Some(root) = structure::Root::read(&export, self.project_type()) else {
            return Ok(None);
        };
        let files = source_snapshot::catalog(&export, None, &[&export.join("ConfigDumpInfo.xml")])
            .map_err(ApplyError::Snapshot)?;
        let descriptors = descriptor::baseline(&export, self.project_type(), &files);
        Ok(Some(Baseline {
            generation: observation.generation.clone(),
            files,
            descriptors,
            root: Some(root),
        }))
    }
}
