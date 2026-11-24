//! Embedded Git and isolated capability fallbacks; no native platform or analyzer.

pub mod command;
pub mod diff;
pub mod git;
pub mod monitor;
pub mod network;
pub mod repository;
pub mod shelves;
pub(crate) mod snapshot;
pub mod status;
pub mod workflow;

pub(crate) mod scm_branches;
pub(crate) mod scm_bridge;

pub mod scm_details;

mod scm_stage_lines;
