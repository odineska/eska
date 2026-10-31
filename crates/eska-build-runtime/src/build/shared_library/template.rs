//! Copy a verified closed file infobase; consumers never open the shared database itself.

use super::{
    BuildError, BuildStage, Ibcmd, PreparedLibrary, ProcessStream, check_cancel, execute, hex,
    source_snapshot, storage,
};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u8,
    cf_sha256: String,
    data_sha256: String,
}

impl PreparedLibrary {
    pub fn materialize<F>(
        &mut self,
        ibcmd: &Ibcmd,
        target: &Path,
        on_output: &mut F,
    ) -> Result<(), BuildError>
    where
        F: FnMut(BuildStage, ProcessStream, &[u8]),
    {
        let root = self.lease.root.join("template");
        for _ in 0..2 {
            check_cancel(ibcmd)?;
            if storage::owned(&root)?
                && let Some(state) = storage::read::<State>(&root.join("state.json"))
                && state.schema_version == 1
                && state.cf_sha256 == self.state.cf_sha256
                && fs::symlink_metadata(root.join("data"))
                    .is_ok_and(|metadata| metadata.file_type().is_dir())
            {
                let parent = target.parent().ok_or_else(|| {
                    storage::error(target, io::Error::from(io::ErrorKind::InvalidInput))
                })?;
                let staged = execute::Workspace::create(parent)?;
                let copy = staged.path.join("data");
                match source_snapshot::capture(&root.join("data"), &copy, &[]) {
                    Ok(digest) if hex(&digest) == state.data_sha256 => {
                        check_cancel(ibcmd)?;
                        if fs::symlink_metadata(target).is_ok() {
                            return Err(BuildError::InvalidExistingOutput(target.to_owned()));
                        }
                        fs::rename(&copy, target)
                            .map_err(|source| storage::error(target, source))?;
                        return Ok(());
                    }
                    Err(source_snapshot::Error::Io { path, source })
                        if path.starts_with(&staged.path) =>
                    {
                        return Err(storage::error(&path, source));
                    }
                    Ok(_) | Err(_) => {}
                }
            }
            self.prepare_template(ibcmd, &root, on_output)?;
        }
        Err(BuildError::BaseConfigurationChanged(root))
    }

    fn prepare_template<F>(
        &mut self,
        ibcmd: &Ibcmd,
        target: &Path,
        on_output: &mut F,
    ) -> Result<(), BuildError>
    where
        F: FnMut(BuildStage, ProcessStream, &[u8]),
    {
        storage::owned(target)?;
        let job = execute::Workspace::create(&self.lease.root)?;
        let ready = job.path.join("ready");
        storage::create_owned(&ready)?;
        let data = ready.join("data");
        let output = execute::create_infobase(
            ibcmd,
            &data,
            Some(&self.path),
            &job.path.join("template.pid"),
            BuildStage::PrepareBaseConfiguration,
            on_output,
        )?;
        check_cancel(ibcmd)?;
        let state = State {
            schema_version: 1,
            cf_sha256: self.state.cf_sha256.clone(),
            data_sha256: hex(
                &source_snapshot::digest(&data, &[]).map_err(BuildError::SourceSnapshot)?
            ),
        };
        storage::write(&ready.join("state.json"), &state)?;
        check_cancel(ibcmd)?;
        storage::publish_directory(&ready, target, &job.path.join("previous"))?;
        self.tool_output = execute::collect_tool_output(&self.tool_output, None, &output);
        Ok(())
    }
}
