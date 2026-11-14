//! Platform execution shares the Designer project model and never opens a Git repository.

pub mod doctor;

pub mod artifact;
pub mod build;
pub mod patch;
mod platform_path;
pub use platform_path::native_path;
pub mod runtime;

#[cfg(test)]
#[path = "../../../tests/support/mod.rs"]
mod test_support;
