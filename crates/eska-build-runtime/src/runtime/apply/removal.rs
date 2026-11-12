//! Review and remove one installed extension from a test infobase.

use std::{ffi::OsString, fs, io, path::Path};

use eska_project_core::project::{Project, ProjectType, local_settings::ProfileName};
use serde::Serialize;

use super::{
    ApplyError, ExtensionIdentity, Stage, Target, catalog_digest, checksum, extension, lock,
    mark_pending, native_path, require_test, source_snapshot,
};

const MARKER: &[u8] = b"extension-remove\0";

#[derive(Serialize)]
pub struct Plan {
    pub target: String,
    pub platform_version: String,
    pub extension: ExtensionIdentity,
    pub configuration_generation: String,
    pub extension_generation: Option<String>,
    pub pending_operation: bool,
    pub confirmation: String,
    #[serde(skip)]
    inventory: Vec<u8>,
}

/// Review or explicitly confirm deletion from the selected test infobase.
///
/// # Errors
/// Refuses ambiguous identities, installed dependents, stale confirmation, and platform failures.
pub fn remove(
    project: &Project,
    profile: ProfileName,
    confirmation: Option<&str>,
    force: bool,
    mut removing: impl FnMut(),
) -> Result<Plan, ApplyError> {
    require_test(profile)?;
    if confirmation.is_some() && !force {
        return Err(ApplyError::ForceRequired);
    }
    if project.configuration().project_type() != ProjectType::Extension {
        return Err(ApplyError::ProjectType);
    }
    let mut selection = extension::resolve(project, true)?;
    let dependents = std::mem::take(&mut selection.dependents);
    let target = Target::open_selected(project, profile, Some(selection))?;
    let _lock = confirmation.map(|_| lock(&target.lock)).transpose()?;
    target.verify_source(project.source())?;
    let plan = review(&target, &dependents)?;
    let Some(confirmation) = confirmation else {
        return Ok(plan);
    };
    if confirmation != plan.confirmation {
        return Err(ApplyError::StalePreview);
    }
    if target.extension_listing()?.as_deref() != Some(&plan.inventory) {
        return Err(ApplyError::StalePreview);
    }
    if plan.extension_generation.is_some() {
        let marker = removal_marker(&plan.extension.name);
        mark_pending(&target.state, &[&marker, confirmation.as_bytes()].concat())?;
        removing();
        target.command_for(
            None,
            &["extension", "delete"],
            &[OsString::from(format!("--name={}", plan.extension.name))],
            None,
            None,
            Stage::Remove,
        )?;
        if target.extension_listed(&plan.extension.name)? != Some(false) {
            return Err(ApplyError::PlatformFailed(Stage::Remove));
        }
    }
    remove_if_present(&target.state)?;
    remove_if_present(&target.state.with_extension("pending"))?;
    Ok(plan)
}

fn review(target: &Target, dependents: &[String]) -> Result<Plan, ApplyError> {
    let identity = target.extension.as_ref().ok_or(ApplyError::InvalidState)?;
    let listing = target
        .extension_listing()?
        .ok_or(ApplyError::ExtensionUnavailable)?;
    let inventory = extension::inventory(&listing).ok_or(ApplyError::ExtensionUnavailable)?;
    let pending = match fs::read(target.state.with_extension("pending")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(ApplyError::Io(error)),
    };
    let name = identity.name.to_lowercase();
    let mut matches = inventory
        .iter()
        .filter(|(listed, _)| listed.to_lowercase() == name);
    let (extension_generation, export) = match (matches.next(), matches.next()) {
        (Some((_, true)), None) => {
            for dependent in dependents {
                let dependent_name = dependent.to_lowercase();
                if inventory
                    .iter()
                    .any(|(listed, _)| listed.to_lowercase() == dependent_name)
                {
                    return Err(ApplyError::ExtensionHasDependent(dependent.clone()));
                }
            }
            (
                Some(target.generation()?),
                Some(export_digest(target, identity)?),
            )
        }
        (None, None)
            if pending
                .as_deref()
                .is_some_and(|bytes| bytes.starts_with(&removal_marker(&identity.name))) =>
        {
            (None, None)
        }
        (None, None) => return Err(ApplyError::ExtensionNotInstalled(identity.name.clone())),
        (Some((_, false)), None) => return Err(ApplyError::ExtensionScope),
        _ => return Err(ApplyError::ExtensionAmbiguous),
    };
    let configuration_generation = target.generation_for(None)?;
    let platform_version = target.platform.version().as_str().to_owned();
    let key = serde_json::to_vec(&(
        &target.connection,
        &target.identity,
        &target.launch,
        &platform_version,
        identity,
        &configuration_generation,
        &extension_generation,
        export,
        dependents,
        &pending,
    ))
    .map_err(|_| ApplyError::InvalidState)?;
    Ok(Plan {
        target: target.identity.clone(),
        platform_version,
        extension: identity.clone(),
        configuration_generation,
        extension_generation,
        pending_operation: pending.is_some(),
        confirmation: checksum(&[MARKER, &key, &listing]),
        inventory: listing,
    })
}

fn removal_marker(name: &str) -> Vec<u8> {
    [MARKER, name.as_bytes(), b"\0"].concat()
}

fn export_digest(target: &Target, identity: &ExtensionIdentity) -> Result<[u8; 32], ApplyError> {
    let export = target.scratch.path().join("extension-review");
    target.command(
        &["export"],
        &[native_path(&export).as_os_str().to_owned()],
        None,
        None,
        Stage::Inspect,
    )?;
    if extension::identity(&export).as_ref() != Some(identity) {
        return Err(ApplyError::ExtensionIdentityMismatch);
    }
    let catalog = source_snapshot::catalog(&export, None, &[&export.join("ConfigDumpInfo.xml")])
        .map_err(ApplyError::Snapshot)?;
    catalog_digest(&catalog)
}

fn remove_if_present(path: &Path) -> Result<(), ApplyError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ApplyError::Io(error)),
    }
}
