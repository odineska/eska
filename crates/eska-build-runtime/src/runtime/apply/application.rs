//! Apply a workspace application in dependency order under one infobase lock.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use eska_project_core::{
    application::{self, AnalysisContext, ContextSource, SourceRole},
    project::local_settings::{ProfileName, SettingsError},
};
use serde::Serialize;

use super::{
    ApplyError, Catalog, LaunchTarget, Mode, Observation, Plan, Review, ReviewReason, RuntimeError,
    Stage, StagedInput, Target, checksum, extension, lock, plan, read_baseline, write_plan,
};

#[derive(Debug, Serialize)]
pub struct ApplicationPlan {
    pub application: String,
    pub launch: LaunchTarget,
    pub components: Vec<ComponentPlan>,
    pub confirmation: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ComponentPlan {
    pub member: String,
    pub project_dir: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<ComponentReview>,
}

#[derive(Debug, Serialize)]
pub struct ComponentReview {
    pub reason: ReviewReason,
    #[serde(flatten)]
    pub review: Box<Review>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    Unchanged,
    NotStarted,
    Applied,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct ComponentOutcome {
    pub member: String,
    pub status: ComponentStatus,
}

#[derive(Debug, Serialize)]
pub struct AppliedApplication {
    pub launch: LaunchTarget,
    pub outcomes: Vec<ComponentOutcome>,
}

#[derive(Debug)]
pub struct Failure {
    pub error: Box<ApplyError>,
    pub member: Option<String>,
    pub outcomes: Vec<ComponentOutcome>,
}

impl From<ApplyError> for Failure {
    fn from(error: ApplyError) -> Self {
        Self {
            error: Box::new(error),
            member: None,
            outcomes: Vec::new(),
        }
    }
}

/// Preview all writable components of the explicitly named workspace application.
///
/// # Errors
/// Rejects an invalid application, unsupported target, or unverified installed identity.
pub fn preview(
    start: &Path,
    application: &str,
    profile: ProfileName,
    allow_full: bool,
) -> Result<ApplicationPlan, Failure> {
    let context = resolve(start, application, profile)?;
    let members = targets(&context, profile)?;
    prepare(&members, application, allow_full, false).map(|prepared| prepared.plan)
}

/// Revalidate every reviewed component before writing, then apply in dependency order.
///
/// # Errors
/// Returns the component outcomes if a verified sequence fails after work has started.
pub fn execute(
    start: &Path,
    application: &str,
    profile: ProfileName,
    allow_full: bool,
    confirmation: &str,
    mut progress: impl FnMut(&str, Stage),
) -> Result<AppliedApplication, Failure> {
    let context = resolve(start, application, profile)?;
    let members = targets(&context, profile)?;
    let _lock = lock(&members[0].target.lock)?;
    let prepared = prepare(&members, application, allow_full, true)?;
    if prepared.plan.confirmation.as_deref() != Some(confirmation) {
        return Err(ApplyError::StalePreview.into());
    }
    let launch = prepared.plan.launch.clone();
    let outcomes = apply(&members, prepared, &mut progress)?;
    Ok(AppliedApplication { launch, outcomes })
}

fn resolve(start: &Path, name: &str, profile: ProfileName) -> Result<AnalysisContext, ApplyError> {
    if profile == ProfileName::Prod {
        return Err(ApplyError::Runtime(RuntimeError::Settings(
            SettingsError::ForbiddenAction,
        )));
    }
    application::resolve(start, Some(name)).map_err(|_| ApplyError::ApplicationContext)
}

struct Member<'a> {
    name: &'a str,
    source: &'a ContextSource,
    target: Target,
}

fn targets(context: &AnalysisContext, profile: ProfileName) -> Result<Vec<Member<'_>>, ApplyError> {
    let (base, rest) = context
        .sources()
        .split_first()
        .filter(|(source, _)| source.role() == SourceRole::Configuration && !source.is_reference())
        .ok_or(ApplyError::ApplicationContext)?;
    let base = Member {
        name: base
            .member()
            .ok_or(ApplyError::ApplicationContext)?
            .as_str(),
        source: base,
        target: Target::open(base.project(), profile)?,
    };
    let mut identities = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut uuids = BTreeSet::new();
    let mut members = Vec::with_capacity(rest.len() + 1);
    for source in rest
        .iter()
        .filter(|source| source.role() == SourceRole::Extension)
    {
        if source
            .extension_scope()
            .is_some_and(|scope| !scope.is_default())
        {
            return Err(ApplyError::ExtensionScope);
        }
        let member = source.member().ok_or(ApplyError::ApplicationContext)?;
        let identity = extension::identity(source.project().source())
            .ok_or(ApplyError::ExtensionDescriptor)?;
        if !names.insert(identity.name.to_lowercase()) || !uuids.insert(identity.uuid.clone()) {
            return Err(ApplyError::ExtensionAmbiguous);
        }
        // AnalysisContext provides dependencies before every dependant.
        let dependencies = extension::dependency_closure(member, context.extension_dependencies())
            .into_iter()
            .map(|name| {
                identities
                    .get(name)
                    .cloned()
                    .ok_or(ApplyError::ApplicationContext)
            })
            .collect::<Result<_, _>>()?;
        let target = base
            .target
            .for_extension(source.project(), identity.clone(), dependencies)?;
        identities.insert(member, identity);
        members.push(Member {
            name: member.as_str(),
            source,
            target,
        });
    }
    members.insert(0, base);
    Ok(members)
}

#[derive(Default)]
struct Generations {
    base: Option<String>,
    extensions: BTreeMap<String, String>,
}

impl Generations {
    fn set(&mut self, target: &Target, generation: String) {
        if let Some(extension) = &target.extension {
            self.extensions.insert(extension.name.clone(), generation);
        } else {
            self.base = Some(generation);
        }
    }

    fn for_target(&self, target: &Target) -> Result<Observation, ApplyError> {
        let generation = target
            .extension
            .as_ref()
            .map_or(self.base.as_ref(), |extension| {
                self.extensions.get(&extension.name)
            });
        let dependency_generations = target
            .dependencies
            .iter()
            .map(|dependency| {
                let generation = self
                    .extensions
                    .get(&dependency.name)
                    .ok_or(ApplyError::InvalidState)?;
                Ok((dependency.name.clone(), generation.clone()))
            })
            .collect::<Result<_, _>>()?;
        Ok(Observation {
            generation: generation.ok_or(ApplyError::InvalidState)?.clone(),
            configuration_generation: self.base.clone().filter(|_| target.extension.is_some()),
            dependency_generations,
        })
    }
}

struct Prepared {
    plan: ApplicationPlan,
    generations: Generations,
    inputs: Vec<(PathBuf, Catalog)>,
}

fn prepare(
    members: &[Member<'_>],
    application: &str,
    allow_full: bool,
    stage: bool,
) -> Result<Prepared, Failure> {
    let mut generations = Generations::default();
    let mut components = Vec::with_capacity(members.len());
    let mut inputs = Vec::with_capacity(members.len());
    for member in members {
        let prepared =
            prepare_component(member, &mut generations, allow_full, stage).map_err(|error| {
                Failure {
                    error: Box::new(error),
                    member: Some(member.name.to_owned()),
                    outcomes: Vec::new(),
                }
            })?;
        components.push(prepared.0);
        inputs.push(prepared.1);
    }
    let launch = members[0].target.launch.clone();
    let eligible = components.iter().all(|part| {
        part.plan
            .as_ref()
            .is_some_and(|plan| plan.mode != Mode::Unsupported)
    });
    let writes = components.iter().any(|part| {
        part.plan
            .as_ref()
            .is_some_and(|plan| plan.confirmation.is_some())
    });
    let confirmation = if eligible && writes {
        let bytes = serde_json::to_vec(&(application, &launch, allow_full, &components))
            .map_err(|_| ApplyError::UnrepresentablePath)?;
        Some(checksum(&[b"application\0", &bytes]))
    } else {
        None
    };
    Ok(Prepared {
        plan: ApplicationPlan {
            application: application.to_owned(),
            launch,
            components,
            confirmation,
        },
        generations,
        inputs,
    })
}

fn prepare_component(
    member: &Member<'_>,
    generations: &mut Generations,
    allow_full: bool,
    stage: bool,
) -> Result<(ComponentPlan, (PathBuf, Catalog)), ApplyError> {
    let project = member.source.project();
    let target = &member.target;
    let staged_baseline = stage.then(|| read_baseline(&target.state)).transpose()?;
    let mut input = if stage {
        target.stage_source(
            project,
            staged_baseline
                .as_ref()
                .and_then(|saved| saved.as_ref().ok()),
        )?
    } else {
        StagedInput {
            source: project.source().to_owned(),
            current: target.capture(project, None)?,
            complete: false,
        }
    };
    target.verify_source(&input.source)?;
    generations.set(target, target.verified_generation()?);
    let observation = generations.for_target(target)?;
    let baseline = match staged_baseline {
        Some(baseline) => baseline,
        None => target.preview_baseline(Some(&observation))?,
    };
    let incremental = plan(
        target,
        Ok(observation.clone()),
        &input.source,
        &input.current,
        false,
        baseline,
    );
    let needs_full = matches!(&incremental, Ok(plan) if plan.mode == Mode::Unsupported)
        || matches!(
            &incremental,
            Err(ApplyError::ReviewRequired(
                ReviewReason::BaselineRequired
                    | ReviewReason::BaselineChanged
                    | ReviewReason::Incomplete
                    | ReviewReason::InvalidBaseline,
                _
            ))
        );
    let result = if allow_full && needs_full {
        if stage && !input.complete {
            input = target.stage_full_source(project)?;
            target.verify_source(&input.source)?;
        }
        plan(
            target,
            Ok(observation),
            &input.source,
            &input.current,
            true,
            read_baseline(&target.state)?,
        )
    } else {
        incremental
    };
    let (plan, review) = match result {
        Ok(plan) => (Some(plan), None),
        Err(ApplyError::ReviewRequired(reason, review)) => {
            (None, Some(ComponentReview { reason, review }))
        }
        Err(error) => return Err(error),
    };
    if stage && let Some(plan) = &plan {
        input.stage_import_files(project, plan)?;
    }
    Ok((
        ComponentPlan {
            member: member.name.to_owned(),
            project_dir: project.root().to_owned(),
            plan,
            review,
        },
        (input.source, input.current),
    ))
}

fn apply(
    members: &[Member<'_>],
    mut prepared: Prepared,
    progress: &mut impl FnMut(&str, Stage),
) -> Result<Vec<ComponentOutcome>, Failure> {
    let mut outcomes: Vec<_> = prepared
        .plan
        .components
        .iter()
        .map(|part| ComponentOutcome {
            member: part.member.clone(),
            status: if part
                .plan
                .as_ref()
                .is_some_and(|plan| plan.mode == Mode::Unchanged)
            {
                ComponentStatus::Unchanged
            } else {
                ComponentStatus::NotStarted
            },
        })
        .collect();
    for (index, ((member, (source, catalog)), part)) in members
        .iter()
        .zip(prepared.inputs)
        .zip(&mut prepared.plan.components)
        .enumerate()
    {
        if outcomes[index].status == ComponentStatus::Unchanged {
            continue;
        }
        let mut started = false;
        let result = apply_component(
            &member.target,
            &source,
            catalog,
            part,
            &mut prepared.generations,
            &mut |stage| {
                started = true;
                progress(member.name, stage);
            },
        );
        if let Err(error) = result {
            if started {
                outcomes[index].status = ComponentStatus::Failed;
            }
            return Err(Failure {
                error: Box::new(error),
                member: Some(member.name.to_owned()),
                outcomes,
            });
        }
        outcomes[index].status = ComponentStatus::Applied;
    }
    for member in members {
        let expected = prepared.generations.for_target(&member.target)?;
        let observed = member.target.verified_generation().and_then(|generation| {
            (generation == expected.generation)
                .then_some(())
                .ok_or(ApplyError::StalePreview)
        });
        if let Err(error) = observed {
            return Err(Failure {
                error: Box::new(error),
                member: Some(member.name.to_owned()),
                outcomes,
            });
        }
    }
    Ok(outcomes)
}

fn apply_component(
    target: &Target,
    source: &Path,
    catalog: Catalog,
    part: &mut ComponentPlan,
    generations: &mut Generations,
    progress: &mut impl FnMut(Stage),
) -> Result<(), ApplyError> {
    if target.observe()? != generations.for_target(target)? {
        return Err(ApplyError::StalePreview);
    }
    let plan = part.plan.as_mut().ok_or(ApplyError::StalePreview)?;
    let generation = write_plan(target, source, catalog, plan, progress)?;
    generations.set(target, generation);
    Ok(())
}
