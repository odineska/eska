//! Shared project discovery and selection diagnostics; no native runtime.

pub use eska_presentation::diagnostics::*;

use crate::{
    cli::localization::Localizer,
    project::discovery::{ContextDiscoveryError, DiscoveryError},
};

pub(super) fn present_repository_context_error(
    error: &ContextDiscoveryError,
    localizer: &Localizer,
) -> String {
    if matches!(
        error,
        ContextDiscoveryError::Project(DiscoveryError::NotFound { .. })
    ) {
        localizer.text("vcs-project-required")
    } else {
        present_context_error(error, localizer)
    }
}
