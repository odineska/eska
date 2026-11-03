//! Form viewing shares checked node ownership and the declared application with other IDE tools.

use super::{ProjectSession, WorkspaceError};
use crate::{
    application,
    project::{
        forms::{self, FormDependency, FormInput, FormOverlay, FormPreparation, FormPreview},
        metadata_model::{MetadataKind, NodeId, ObjectId},
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
        previous: &[FormDependency],
        inputs: Vec<FormInput>,
        activity: Option<&crate::project::activity::State>,
    ) -> Result<FormPreview, FormPreviewError> {
        let (owner, context) = self.form_context(node, application, activity)?;
        forms::load(
            &context,
            self.project().source(),
            owner,
            overlays,
            previous,
            inputs,
        )
        .map_err(FormPreviewError::Form)
    }

    /// Resolve checked form input locations before parsing any form payload.
    ///
    /// # Errors
    /// Returns ownership/context failures or unavailable form sources.
    pub fn prepare_form(
        &self,
        node: &NodeId,
        application: Option<&str>,
        activity: Option<&crate::project::activity::State>,
    ) -> Result<FormPreparation, FormPreviewError> {
        let (owner, context) = self.form_context(node, application, activity)?;
        forms::prepare(&context, self.project().source(), owner).map_err(FormPreviewError::Form)
    }

    fn form_context<'a>(
        &self,
        node: &'a NodeId,
        application: Option<&str>,
        activity: Option<&crate::project::activity::State>,
    ) -> Result<(&'a ObjectId, application::ContextSnapshot), FormPreviewError> {
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
        Ok((owner, context))
    }
}
