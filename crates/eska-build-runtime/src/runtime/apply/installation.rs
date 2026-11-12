//! Review and install an absent Designer XML extension in a test infobase.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use eska_project_core::project::{Project, ProjectType, local_settings::ProfileName};
use serde::Serialize;

use super::{
    ApplyError, Baseline, Catalog, ExtensionIdentity, Stage, Target, catalog_digest, checksum,
    descriptor, extension, lock, mark_pending, read_pending, require_test, save_baseline,
    structure,
};

const MARKER: &[u8] = b"extension-install\0";

#[derive(Serialize)]
pub struct Plan {
    pub target: String,
    pub platform_version: String,
    pub extension: ExtensionIdentity,
    pub configuration_generation: String,
    pub dependency_generations: BTreeMap<String, String>,
    pub import_files: Vec<PathBuf>,
    pub pending_operation: bool,
    pub confirmation: String,
    #[serde(skip)]
    inventory: Vec<u8>,
}

/// Review an installation or apply the confirmed immutable source snapshot.
///
/// # Errors
/// Refuses an installed name or UUID, missing dependencies and stale confirmations.
pub fn install(
    project: &Project,
    profile: ProfileName,
    confirmation: Option<&str>,
    mut progress: impl FnMut(Stage),
) -> Result<Plan, ApplyError> {
    require_test(profile)?;
    if project.configuration().project_type() != ProjectType::Extension {
        return Err(ApplyError::ProjectType);
    }
    let target = Target::open(project, profile)?;
    let _lock = confirmation.map(|_| lock(&target.lock)).transpose()?;
    let Some(confirmation) = confirmation else {
        let current = target.capture(project, None)?;
        target.verify_source(project.source())?;
        return review(&target, &current);
    };
    let input = target.stage_full_source(project)?;
    target.verify_source(&input.source)?;
    let plan = review(&target, &input.current)?;
    if confirmation != plan.confirmation {
        return Err(ApplyError::StalePreview);
    }
    let descriptors = descriptor::baseline(&input.source, ProjectType::Extension, &input.current);
    let root = structure::Root::read(&input.source, ProjectType::Extension);
    verify_base(&target, &plan)?;
    if target.extension_listing()?.as_deref() != Some(&plan.inventory) {
        return Err(ApplyError::StalePreview);
    }
    let marker = installation_marker(&plan.extension.name);
    let pending = mark_pending(&target.state, &[&marker, confirmation.as_bytes()].concat())?;
    progress(Stage::Import);
    target.import_full(&input.source)?;
    target.verify_installed(&plan.extension)?;
    verify_base(&target, &plan)?;
    progress(Stage::Apply);
    let applied = target.command(&["apply"], &[], None, None, Stage::Apply)?;
    let generation = target.applied_generation(&applied)?;
    target.verify_installed(&plan.extension)?;
    let baseline = Baseline {
        generation,
        files: input.current,
        descriptors,
        root,
    };
    save_baseline(&target.state, &pending, &baseline)?;
    Ok(plan)
}

fn review(target: &Target, current: &Catalog) -> Result<Plan, ApplyError> {
    let identity = target.extension.as_ref().ok_or(ApplyError::InvalidState)?;
    let listing = target
        .extension_listing()?
        .ok_or(ApplyError::ExtensionUnavailable)?;
    let inventory = extension::inventory(&listing).ok_or(ApplyError::ExtensionUnavailable)?;
    let pending = read_pending(&target.state)?;
    let name = identity.name.to_lowercase();
    if inventory
        .iter()
        .any(|(listed, _)| listed.to_lowercase() == name)
    {
        if pending
            .as_deref()
            .is_some_and(|bytes| bytes.starts_with(&installation_marker(&identity.name)))
            && target.installed_uuid(&identity.name)?.as_deref() == Some(identity.uuid.as_str())
        {
            return Err(ApplyError::ExtensionInstallationIncomplete(
                identity.name.clone(),
            ));
        }
        return Err(ApplyError::ExtensionAlreadyInstalled(identity.name.clone()));
    }
    let dependency_generations = target.observe_dependencies()?;
    verify_inventory(target, identity, &inventory)?;
    let configuration_generation = target.generation_for(None)?;
    let platform_version = target.platform.version().as_str().to_owned();
    let key = serde_json::to_vec(&(
        &target.connection,
        &target.identity,
        &target.launch,
        &platform_version,
        identity,
        &configuration_generation,
        &dependency_generations,
        &pending,
    ))
    .map_err(|_| ApplyError::InvalidState)?;
    Ok(Plan {
        target: target.identity.clone(),
        platform_version,
        extension: identity.clone(),
        configuration_generation,
        dependency_generations,
        import_files: current.keys().cloned().collect(),
        pending_operation: pending.is_some(),
        confirmation: checksum(&[MARKER, &key, &listing, &catalog_digest(current)?]),
        inventory: listing,
    })
}

fn verify_inventory(
    target: &Target,
    identity: &ExtensionIdentity,
    inventory: &[(&str, bool)],
) -> Result<(), ApplyError> {
    let mut names = BTreeSet::new();
    for &(name, infobase_scope) in inventory {
        if !names.insert(name.to_lowercase()) {
            return Err(ApplyError::ExtensionAmbiguous);
        }
        if target
            .dependencies
            .iter()
            .any(|dependency| dependency.name == name)
        {
            if !infobase_scope {
                return Err(ApplyError::ExtensionScope);
            }
            // The declared dependency identities were checked before this inventory sweep.
            continue;
        }
        let uuid = target
            .installed_uuid(name)
            .map_err(|error| match error {
                ApplyError::PlatformFailed(Stage::Inspect) => ApplyError::ExtensionUnavailable,
                other => other,
            })?
            .ok_or(ApplyError::ExtensionUnavailable)?;
        if uuid == identity.uuid {
            return Err(ApplyError::ExtensionUuidInUse(name.to_owned()));
        }
    }
    Ok(())
}

fn verify_base(target: &Target, plan: &Plan) -> Result<(), ApplyError> {
    if target.generation_for(None)? != plan.configuration_generation
        || target.observe_dependencies()? != plan.dependency_generations
    {
        return Err(ApplyError::StalePreview);
    }
    Ok(())
}

fn installation_marker(name: &str) -> Vec<u8> {
    [MARKER, name.as_bytes(), b"\0"].concat()
}
