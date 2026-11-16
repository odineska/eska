//! New-object selectors are resolved lazily after the caller chooses a kind.

use super::ProjectSession;
use crate::project::{
    metadata_edit::{
        ScalarSchema, TextDomain, initial_field, reference_lists, references, type_sets,
    },
    metadata_model::{MetadataKind, ObjectId},
    metadata_objects::{ObjectError, ObjectInitialChoices, ObjectInitialField, templates},
    metadata_workspace::PropertyEditError,
};

impl ProjectSession {
    /// Discover allowed values for required properties without creating a temporary metadata object.
    ///
    /// # Errors
    /// Rejects unsupported kinds/owners or unreadable reference declarations.
    pub fn object_initial_fields(
        &mut self,
        parent: Option<&ObjectId>,
        kind: MetadataKind,
    ) -> Result<Vec<ObjectInitialField>, ObjectError> {
        if !self.object_schema(parent)?.kinds.contains(&kind) {
            return Err(ObjectError::Unsupported(kind));
        }
        let id = ObjectId::from_parts(parent, kind.as_str(), "Object");
        let mut result = Vec::new();
        for property in templates::required_properties(kind) {
            let choices = if let Some(rule) = type_sets::rule(&id, property) {
                ObjectInitialChoices::Types(
                    self.type_set_choices(rule.domain).map_err(property_error)?,
                )
            } else if let Some(rule) = reference_lists::rule(kind.designer_tag(), property) {
                ObjectInitialChoices::References {
                    multiple: true,
                    choices: self
                        .reference_list_choices(rule.domain)
                        .map_err(property_error)?,
                }
            } else {
                match initial_field(kind, property) {
                    Some(ScalarSchema::Reference { domain, .. }) => {
                        ObjectInitialChoices::References {
                            multiple: false,
                            choices: self
                                .reference_list_choices(&domain)
                                .map_err(property_error)?,
                        }
                    }
                    Some(ScalarSchema::Text {
                        domain: Some(TextDomain::CommandGroup),
                    }) => {
                        let mut values: Vec<_> = references::COMMAND_GROUPS
                            .iter()
                            .map(|value| (*value).to_owned())
                            .collect();
                        values.extend(
                            self.objects
                                .values()
                                .filter(|object| {
                                    object.kind == MetadataKind::CommandGroup
                                        && object.parent.is_none()
                                })
                                .map(|object| format!("CommandGroup.{}", object.name)),
                        );
                        ObjectInitialChoices::Values {
                            domain: "StandardCommandsGroup",
                            values,
                        }
                    }
                    _ => continue,
                }
            };
            result.push(ObjectInitialField {
                property: (*property).to_owned(),
                choices,
            });
        }
        Ok(result)
    }
}

fn property_error(error: PropertyEditError) -> ObjectError {
    match error {
        PropertyEditError::Workspace(error) => ObjectError::Workspace(error),
        PropertyEditError::Edit(error) | PropertyEditError::Related { error, .. } => {
            ObjectError::Edit(error)
        }
        PropertyEditError::Committed(error) => ObjectError::Committed(error),
    }
}
