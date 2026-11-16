//! Type qualifiers are validated before native XML can silently normalize unsupported values.

use std::collections::BTreeSet;

use super::{
    CreateRequest, CreateType, CreationError, CreationKind, CreationReference, capabilities,
};
use crate::project::metadata_model::PropertyKey;

pub(super) fn validate(
    request: &CreateRequest,
    references: &[CreationReference],
) -> Result<Vec<PropertyKey>, CreationError> {
    if request.kind == CreationKind::TablePart {
        return if request.types.is_empty() {
            Ok(Vec::new())
        } else {
            Err(invalid("not_applicable"))
        };
    }
    if request.types.is_empty() || request.types.len() > 64 {
        return Err(invalid("required_or_too_many"));
    }
    let limits = capabilities().limits;
    let mut keys = Vec::with_capacity(request.types.len());
    let mut unique = BTreeSet::new();
    for value in &request.types {
        let name = match value {
            CreateType::String { length, fixed } => {
                if *length > limits.string_length_max
                    || (*fixed
                        && (*length < limits.string_fixed_length_min
                            || *length > limits.string_fixed_length_max))
                {
                    return Err(invalid("string_length"));
                }
                "string"
            }
            CreateType::Number {
                digits,
                fraction_digits,
                ..
            } => {
                if *digits < limits.number_digits_min
                    || *digits > limits.number_digits_max
                    || fraction_digits > digits
                {
                    return Err(invalid("number_qualifiers"));
                }
                "decimal"
            }
            CreateType::Boolean {} => "boolean",
            CreateType::Date { .. } => "dateTime",
            CreateType::Reference { object } => {
                let reference = references
                    .iter()
                    .find(|reference| &reference.object == object)
                    .ok_or_else(|| invalid("unknown_reference"))?;
                if !unique.insert((reference.key.namespace.clone(), reference.key.name.clone())) {
                    return Err(invalid("duplicate"));
                }
                keys.push(reference.key.clone());
                continue;
            }
        };
        let key = PropertyKey {
            namespace: Some(super::XS.to_owned()),
            name: name.to_owned(),
        };
        if !unique.insert((key.namespace.clone(), key.name.clone())) {
            return Err(invalid("duplicate"));
        }
        keys.push(key);
    }
    Ok(keys)
}

const fn invalid(reason: &'static str) -> CreationError {
    CreationError::Invalid {
        field: "types",
        reason,
    }
}
