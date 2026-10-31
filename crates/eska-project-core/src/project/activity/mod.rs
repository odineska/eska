//! Machine-local IDE activity is independent of manifests, source formats and CLI selection.

mod store;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

use super::metadata_model::ObjectId;
pub use store::{Snapshot, Store};

pub const MAX_PROJECTS: usize = 8192;
const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;

/// A disabled row needs no descriptor cache, metadata index or source observer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RootSummary {
    pub id: ObjectId,
    pub name: String,
}

/// Only explicit disabled choices are stored: new members are active by default.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub schema_version: u32,
    pub disabled: BTreeMap<String, RootSummary>,
    pub hide_disabled: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema_version: 1,
            disabled: BTreeMap::new(),
            hide_disabled: false,
        }
    }
}

impl State {
    #[must_use]
    pub fn enabled(&self, root: &Path) -> bool {
        !self.disabled.contains_key(&path_key(root))
    }

    fn valid(&self) -> bool {
        self.schema_version == 1
            && self.disabled.len() <= MAX_PROJECTS
            && self.disabled.iter().all(|(key, summary)| {
                is_key(key)
                    && !summary.name.is_empty()
                    && summary.name.len() <= 65_536
                    && !summary.name.contains('\0')
                    && !summary.id.as_str().is_empty()
                    && summary.id.as_str().len() <= 65_536
                    && !summary.id.as_str().contains('\0')
            })
    }
}

/// Canonical discovery paths supply the identity; names, groups and row positions do not participate.
#[must_use]
pub fn path_key(root: &Path) -> String {
    digest(root.as_os_str().as_encoded_bytes())
}

fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}
fn is_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Activity failures never contain localized messages or authorize an implicit source write.
#[derive(Debug)]
pub enum Error {
    Invalid,
    ResourceLimit,
    UnsafePath,
    Conflict,
    Busy,
    Io(std::io::Error),
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests;
