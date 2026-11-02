//! Form viewing shares checked node ownership and the declared application with other IDE tools.

use super::{ProjectSession, WorkspaceError};
use crate::{
    application,
    project::{
        forms::{self, FormOverlay, FormPreview},
        metadata_model::{MetadataKind, NodeId},
    },
};

#[derive(Debug)]
pub enum FormPreviewError {
    Workspace(Box<WorkspaceError>),
    Context(Box<application::ContextError>),
    Form(forms::FormContextError),
    NotManagedForm,
}

impl ProjectSession {
    /// Resolve the current form and its applicable extensions without writing any source.
    ///
    /// # Errors
    /// Returns checked ownership/context errors or a structured form parse failure.
    pub fn form_preview(
        &self,
        node: &NodeId,
        application: Option<&str>,
        overlays: &[FormOverlay],
        activity: Option<&crate::project::activity::State>,
    ) -> Result<FormPreview, FormPreviewError> {
        self.node(node)
            .map_err(|error| FormPreviewError::Workspace(Box::new(error)))?;
        let NodeId::Object(owner) = node else {
            return Err(FormPreviewError::NotManagedForm);
        };
        let object = self
            .object(owner)
            .map_err(|error| FormPreviewError::Workspace(Box::new(error)))?;
        if !matches!(object.kind, MetadataKind::Form | MetadataKind::CommonForm) {
            return Err(FormPreviewError::NotManagedForm);
        }
        let context = activity
            .map_or_else(
                || application::resolve_snapshot(self.project().root(), application),
                |activity| {
                    application::resolve_active_snapshot(
                        self.project().root(),
                        application,
                        activity,
                    )
                    .and_then(|snapshot| snapshot.ok_or(application::ContextError::EmptyWorkspace))
                },
            )
            .map_err(|error| FormPreviewError::Context(Box::new(error)))?;
        forms::load(&context, self.project().source(), owner, overlays)
            .map_err(FormPreviewError::Form)
    }
}
