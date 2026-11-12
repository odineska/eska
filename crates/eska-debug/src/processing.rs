//! Verify and bind one external processing for the shared DAP session.
use std::path::PathBuf;

use eska_build_runtime::{
    build::{ProcessingArtifactError, ProcessingSources},
    runtime::DebugLaunch,
};
use eska_project_core::project::{Project, ProjectType, local_settings::DebugProcessing};
use serde::Deserialize;

use crate::source::Sources;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalProcessing {
    pub source_root: PathBuf,
    pub artifact: PathBuf,
    pub debug_url: Option<String>,
}

impl ExternalProcessing {
    pub fn from_project(
        project: &Project,
        settings: &DebugProcessing,
    ) -> Result<Self, &'static str> {
        const INVALID: &str = "debug-epf-project-type";
        if project.configuration().project_type() != ProjectType::Processing {
            return Err(INVALID);
        }
        let source_root = project.source().canonicalize().map_err(|_| INVALID)?;
        let root = project.root().canonicalize().map_err(|_| INVALID)?;
        if !source_root.starts_with(&root) {
            return Err(INVALID);
        }
        Ok(Self {
            source_root,
            artifact: root.join(&settings.artifact),
            debug_url: settings.debug_url.clone(),
        })
    }

    pub fn configure(
        self,
        sources: &mut Sources,
        launch: Option<&mut DebugLaunch>,
    ) -> Result<(), &'static str> {
        let (provenance, url) = if let Some(launch) = launch {
            let (provenance, staged) = launch
                .stage_processing(&self.source_root, &self.artifact)
                .map_err(|error| processing_failure(&error))?;
            let path = staged.to_str().ok_or("debug-epf-manifest")?;
            #[cfg(windows)]
            let path = format!("/{}", path.replace('\\', "/"));
            (provenance, format!("file://{path}"))
        } else {
            let provenance = ProcessingSources::verify(&self.source_root, &self.artifact)
                .map_err(|error| processing_failure(&error))?;
            let url = self.debug_url.ok_or("debug-epf-attach-url")?;
            if !verified_processing_url(&url, provenance.artifact_sha256()) {
                return Err("debug-epf-attach-url");
            }
            (provenance, url)
        };
        sources
            .add_processing(&self.source_root, url, provenance)
            .map_err(|_| "debug-epf-source-mismatch")
    }
}

const fn processing_failure(error: &ProcessingArtifactError) -> &'static str {
    match error {
        ProcessingArtifactError::SourceMismatch => "debug-epf-source-mismatch",
        ProcessingArtifactError::ArtifactMismatch => "debug-epf-artifact-mismatch",
        ProcessingArtifactError::Io(_) | ProcessingArtifactError::Manifest => "debug-epf-manifest",
    }
}

fn verified_processing_url(url: &str, digest: &str) -> bool {
    let Some(path) = url
        .strip_prefix("file:///")
        .or_else(|| url.strip_prefix("sfile:///"))
    else {
        return false;
    };
    if path
        .chars()
        .any(|character| character.is_control() || character == '\\')
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return false;
    }
    let mut parts = path.rsplit('/');
    parts
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case("epf"))
        && parts.next() == Some(digest)
}

#[cfg(test)]
mod tests {
    use super::verified_processing_url;

    #[test]
    fn attach_url_is_bound_to_the_complete_artifact_digest() {
        let digest = "0123456789abcdef".repeat(4);
        for scheme in ["file", "sfile"] {
            let url = format!("{scheme}:///Рабочие файлы/{digest}/Обработка.EPF");
            assert!(verified_processing_url(&url, &digest));
            assert!(!verified_processing_url(&url, &"f".repeat(64)));
        }
        for path in [
            format!("file:///work/{digest}/../Probe.epf"),
            format!("file:///work/{digest}/Probe.erf"),
            format!("file://host/work/{digest}/Probe.epf"),
            format!("https:///work/{digest}/Probe.epf"),
            "file:///work/Probe.epf".to_owned(),
        ] {
            assert!(!verified_processing_url(&path, &digest), "{path}");
        }
    }
}
