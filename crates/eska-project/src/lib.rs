//! Project command presentation shared by the standalone tool and the existing ESKA CLI.

pub mod doctor;
pub mod ide;
pub mod metadata;
pub mod onboarding;
pub mod version;

mod add;
mod analyzer_config;
mod cli;
mod context;
mod local_settings;
mod operations;
mod response;
mod settings;

pub use cli::run;

#[cfg(test)]
#[path = "../../../tests/support/mod.rs"]
mod test_support;
