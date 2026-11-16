//! Opt-in acceptance against a caller-selected real Designer XML configuration.
//!
//! The supplied source is read-only. Every mutation uses the public CLI in a
//! unique sibling playground directory; no private fixtures belong in Git.

#[path = "support/native_metadata_objects/mod.rs"]
mod native;
#[path = "support/mod.rs"]
mod support;

#[test]
#[ignore = "requires ESKA_NATIVE_METADATA_SOURCE, ESKA_NATIVE_IBCMD and a licensed 1C platform"]
fn object_twins_survive_cli_copy_native_roundtrip_and_cli_delete() {
    native::twins::run();
}

#[test]
#[ignore = "requires ESKA_NATIVE_METADATA_SOURCE, ESKA_NATIVE_IBCMD and a licensed 1C platform"]
fn blank_objects_survive_cli_creation_and_native_roundtrip() {
    native::blank::run();
}

#[test]
#[ignore = "requires the complete public ESKA_NATIVE_METADATA_SOURCE fixture and licensed ESKA_NATIVE_IBCMD"]
fn nested_objects_survive_cli_creation_copy_and_delete() {
    native::children::run();
}

#[test]
#[ignore = "requires ESKA_NATIVE_IBCMD and a licensed 1C platform"]
fn managed_forms_survive_semantic_cli_edits_and_native_roundtrip() {
    native::forms::run();
}

#[test]
#[ignore = "requires ESKA_NATIVE_IBCMD and a licensed 1C platform"]
fn new_configuration_survives_cli_bootstrap_and_native_roundtrip() {
    native::bootstrap();
}
