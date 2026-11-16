//! Native identities remain distinct across independent creations and exact across preview/apply.

use std::{collections::BTreeSet, fmt::Write};

use super::{CreationError, CreationIdentity, CreationKind, GeneratedIdentity};

impl CreationIdentity {
    /// Allocate native UUIDs without introducing a persistent counter or project-local cache.
    pub(crate) fn generate(kind: CreationKind) -> Result<Self, CreationError> {
        let generated = || -> Result<GeneratedIdentity, CreationError> {
            Ok(GeneratedIdentity {
                type_id: allocate_uuid().map_err(CreationError::Random)?,
                value_id: allocate_uuid().map_err(CreationError::Random)?,
            })
        };
        Ok(Self {
            uuid: allocate_uuid().map_err(CreationError::Random)?,
            table: (kind == CreationKind::TablePart)
                .then(generated)
                .transpose()?,
            row: (kind == CreationKind::TablePart)
                .then(generated)
                .transpose()?,
        })
    }

    /// Saved plans cannot reuse an existing descriptor identity or alias their own generated types.
    pub(super) fn validate(
        &self,
        kind: CreationKind,
        input: &roxmltree::Document<'_>,
    ) -> Result<(), CreationError> {
        let mut values = vec![self.uuid.as_str()];
        match (kind, &self.table, &self.row) {
            (CreationKind::Attribute, None, None) => (),
            (CreationKind::TablePart, Some(table), Some(row)) => {
                values.extend([
                    table.type_id.as_str(),
                    table.value_id.as_str(),
                    row.type_id.as_str(),
                    row.value_id.as_str(),
                ]);
            }
            _ => return Err(invalid("shape")),
        }
        let mut unique = BTreeSet::new();
        for value in values {
            if !valid_uuid(value) || !unique.insert(value.to_ascii_lowercase()) {
                return Err(invalid("invalid_or_duplicate"));
            }
        }
        for node in input.descendants().filter(roxmltree::Node::is_element) {
            let existing = node.attribute("uuid").or_else(|| {
                (node.tag_name().namespace() == Some(super::schema::XR)
                    && matches!(node.tag_name().name(), "TypeId" | "ValueId"))
                .then(|| node.text())
                .flatten()
            });
            if existing.is_some_and(|value| unique.contains(&value.to_ascii_lowercase())) {
                return Err(invalid("collision"));
            }
        }
        Ok(())
    }
}

const fn invalid(reason: &'static str) -> CreationError {
    CreationError::Invalid {
        field: "identity",
        reason,
    }
}

/// UUID v4 uses only OS entropy; independent checkouts must never allocate the same logical identity.
pub fn allocate_uuid() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut value = String::with_capacity(36);
    for (index, byte) in bytes.into_iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            value.push('-');
        }
        let _ = write!(value, "{byte:02x}");
    }
    Ok(value)
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
        && value.as_bytes()[14] == b'4'
        && matches!(
            value.as_bytes()[19],
            b'8' | b'9' | b'a' | b'A' | b'b' | b'B'
        )
}
