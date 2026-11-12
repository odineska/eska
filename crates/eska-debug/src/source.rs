//! Resolve platform module identities from the existing Designer XML object model.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use eska_project_core::{
    application::project_application,
    project::{
        Project, ProjectType,
        discovery::{self, DiscoveryContext},
        object_model,
    },
};

use crate::dbgs::Module;

pub struct Sources {
    modules: BTreeMap<PathBuf, Module>,
    paths: BTreeMap<Module, Option<PathBuf>>,
}

pub enum SourceError {
    Project,
    ApplicationChoice,
    DuplicateExtension,
}

impl Sources {
    pub fn load(project: &Project) -> Result<Self, SourceError> {
        let mut sources = Self {
            modules: BTreeMap::new(),
            paths: BTreeMap::new(),
        };
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
        let model = object_model::discover(project).map_err(|_| SourceError::Project)?;
        let extension = if project.configuration().project_type() == ProjectType::Extension {
            let name = model
                .objects()
                .find(|object| object.descriptor_path() == Path::new("Configuration.xml"))
                .map(object_model::LogicalObject::name)
                .filter(|name| !name.is_empty())
                .ok_or(SourceError::Project)?;
            if !extensions.insert(name.to_lowercase()) {
                return Err(SourceError::DuplicateExtension);
            }
            name
        } else {
            ""
        };
        for owner in model.objects().filter(|owner| !owner.uuid().is_empty()) {
            for path in owner.module_paths() {
                let Some(property) = property_id(owner.metadata_type(), path) else {
                    continue;
                };
                let path = project.source().join(path);
                let module = Module {
                    object: owner.uuid().to_ascii_lowercase(),
                    property: property.to_owned(),
                    extension: extension.to_owned(),
                };
                self.modules.insert(path.clone(), module.clone());
                self.paths
                    .entry(module)
                    .and_modify(|mapped| *mapped = None)
                    .or_insert(Some(path));
            }
        }
        Ok(())
    }

    pub fn module(&self, path: &Path) -> Option<&Module> {
        self.modules
            .get(path)
            .filter(|module| self.path(module).is_some())
            .or_else(|| {
                path.canonicalize()
                    .ok()
                    .and_then(|path| self.modules.get(&path))
                    .filter(|module| self.path(module).is_some())
            })
    }

    pub fn path(&self, module: &Module) -> Option<&Path> {
        self.paths.get(module)?.as_deref()
    }
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
