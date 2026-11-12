//! Resolve platform module identities from the existing Designer XML object model.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read as _,
    path::{Path, PathBuf},
};

use eska_build_runtime::build::ProcessingSources;
use eska_project_core::{
    application::project_application,
    project::{
        Project, ProjectType, designer_xml,
        discovery::{self, DiscoveryContext},
        object_model,
    },
};

use crate::dbgs::{Module, ModuleOrigin};

pub struct Sources {
    modules: BTreeMap<PathBuf, Module>,
    paths: BTreeMap<Module, Option<PathBuf>>,
    processing: Option<ProcessingSources>,
}

pub enum SourceError {
    Project,
    ApplicationChoice,
    DuplicateExtension,
}

impl Sources {
    const fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
            paths: BTreeMap::new(),
            processing: None,
        }
    }

    pub fn load(project: &Project) -> Result<Self, SourceError> {
        let mut sources = Self::new();
        let mut extensions = BTreeSet::new();
        let context =
            discovery::discover_context(project.root()).map_err(|_| SourceError::Project)?;
        if let DiscoveryContext::Workspace { workspace, .. } = context
            && let Some((_, application)) = project_application(project, &workspace)
                .map_err(|_| SourceError::ApplicationChoice)?
        {
            for member in workspace.members().iter().filter(|member| {
                application.configuration() == Some(member.name())
                    || application.extensions().contains(member.name())
            }) {
                sources.add_project(member.project(), &mut extensions)?;
            }
        } else {
            sources.add_project(project, &mut extensions)?;
        }
        Ok(sources)
    }

    fn add_project(
        &mut self,
        project: &Project,
        extensions: &mut BTreeSet<String>,
    ) -> Result<(), SourceError> {
        if !matches!(
            project.configuration().project_type(),
            ProjectType::Configuration | ProjectType::Extension
        ) {
            return Ok(());
        }
        self.add_source(
            project.source(),
            project.configuration().project_type(),
            extensions,
        )
    }

    pub fn from_designer(
        configuration: Option<&Path>,
        extension_roots: &[PathBuf],
    ) -> Result<Self, SourceError> {
        let mut sources = Self::new();
        let mut extensions = BTreeSet::new();
        for (root, kind) in configuration
            .into_iter()
            .map(|root| (root, ProjectType::Configuration))
            .chain(
                extension_roots
                    .iter()
                    .map(|root| (root.as_path(), ProjectType::Extension)),
            )
        {
            let root = root.canonicalize().map_err(|_| SourceError::Project)?;
            validate_descriptor(&root, kind)?;
            sources.add_source(&root, kind, &mut extensions)?;
        }
        Ok(sources)
    }

    pub fn add_processing(
        &mut self,
        source: &Path,
        artifact: &Path,
        provenance: ProcessingSources,
    ) -> Result<(), SourceError> {
        let root = source.canonicalize().map_err(|_| SourceError::Project)?;
        let artifact = artifact.to_str().ok_or(SourceError::Project)?;
        #[cfg(windows)]
        let artifact = format!("/{}", artifact.replace('\\', "/"));
        let model = object_model::discover_from_source(&root, ProjectType::Processing)
            .map_err(|_| SourceError::Project)?;
        if !model
            .objects()
            .any(|object| object.metadata_type() == "data-processor")
            || model
                .objects()
                .map(object_model::LogicalObject::descriptor_path)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .any(|path| !provenance.matches_file(&root.join(path)))
        {
            return Err(SourceError::Project);
        }
        self.index(
            &model,
            &root,
            &ModuleOrigin::External(format!("file://{artifact}")),
        );
        self.processing = Some(provenance);
        Ok(())
    }

    fn add_source(
        &mut self,
        root: &Path,
        kind: ProjectType,
        extensions: &mut BTreeSet<String>,
    ) -> Result<(), SourceError> {
        let model =
            object_model::discover_from_source(root, kind).map_err(|_| SourceError::Project)?;
        let origin = if kind == ProjectType::Extension {
            let name = model
                .objects()
                .find(|object| object.descriptor_path() == Path::new("Configuration.xml"))
                .map(object_model::LogicalObject::name)
                .filter(|name| !name.is_empty())
                .ok_or(SourceError::Project)?;
            if !extensions.insert(name.to_lowercase()) {
                return Err(SourceError::DuplicateExtension);
            }
            ModuleOrigin::Extension(name.to_owned())
        } else {
            ModuleOrigin::Configuration
        };
        self.index(&model, root, &origin);
        Ok(())
    }

    fn index(&mut self, model: &object_model::ObjectModel, root: &Path, origin: &ModuleOrigin) {
        for owner in model.objects().filter(|owner| !owner.uuid().is_empty()) {
            for path in owner.module_paths() {
                let Some(property) = property_id(owner.metadata_type(), path) else {
                    continue;
                };
                let path = root.join(path);
                let module = Module {
                    object: owner.uuid().to_ascii_lowercase(),
                    property: property.to_owned(),
                    origin: origin.clone(),
                };
                self.modules.insert(path.clone(), module.clone());
                self.paths
                    .entry(module)
                    .and_modify(|mapped| *mapped = None)
                    .or_insert(Some(path));
            }
        }
    }

    pub fn module(&self, path: &Path) -> Option<&Module> {
        self.modules
            .get(path)
            .or_else(|| {
                path.canonicalize()
                    .ok()
                    .and_then(|path| self.modules.get(&path))
            })
            .filter(|module| self.paths.get(module).is_some_and(Option::is_some))
    }

    pub fn path(&self, module: &Module) -> Option<&Path> {
        let path = self.paths.get(module)?.as_deref()?;
        if matches!(module.origin, ModuleOrigin::External(_))
            && !self
                .processing
                .as_ref()
                .is_some_and(|sources| sources.matches_file(path))
        {
            return None;
        }
        Some(path)
    }
}

fn validate_descriptor(root: &Path, kind: ProjectType) -> Result<(), SourceError> {
    const MAX_ROOT: u64 = 64 * 1024 * 1024;
    let path = root
        .join("Configuration.xml")
        .canonicalize()
        .map_err(|_| SourceError::Project)?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(SourceError::Project);
    }
    let mut text = String::new();
    let file = File::open(path).map_err(|_| SourceError::Project)?;
    file.take(MAX_ROOT + 1)
        .read_to_string(&mut text)
        .map_err(|_| SourceError::Project)?;
    if text.len() as u64 > MAX_ROOT
        || designer_xml::project_type(&text).ok().flatten() != Some(kind)
    {
        return Err(SourceError::Project);
    }
    Ok(())
}

// These are property identifiers of the platform's metadata classes. Object UUIDs come
// from the selected dump; the same property identifier is reused for every object of a class.
fn property_id(kind: &str, path: &Path) -> Option<&'static str> {
    Some(match path.file_stem()?.to_str()? {
        "Module" if matches!(kind, "common-module" | "web-service" | "http-service") => {
            "d5963243-262e-4398-b4d7-fb16d06484f6"
        }
        "Module" => "32e087ab-1491-49b6-aba7-43571b41ac2b",
        "CommandModule" => "078a6af8-d22c-4248-9c33-7e90075a3d2c",
        "ObjectModule" => "a637f77f-3840-441d-a1c3-699c8c5cb7e0",
        "ManagerModule" => "d1b64a2c-8078-4982-8190-8f81aefda192",
        "RecordSetModule" => "9f36fd70-4bf4-47f6-b235-935f73aab43f",
        "ValueManagerModule" => "3e58c91f-9aaa-4f42-8999-4baf33907b75",
        "ManagedApplicationModule" => "d22e852a-cf8a-4f77-8ccb-3548e7792bea",
        "OrdinaryApplicationModule" => "a78d9ce3-4e0c-48d5-9863-ae7342eedf94",
        "SessionModule" => "9b7bbbae-9771-46f2-9e4d-2489e0ffc702",
        "ExternalConnectionModule" => "a4a9c1e2-1e54-4c7f-af06-4ca341198fac",
        _ => return None,
    })
}
