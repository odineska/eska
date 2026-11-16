//! A fresh configuration uses the same identity allocation, plans and journal as its objects.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    fs,
    path::{Path, PathBuf},
};

use super::{
    ObjectError, ObjectPlan, ObjectRequest, ObjectResult, ObjectSchema, PreparedObject, invalid,
    inventory::{Inputs, output_snapshot},
    templates, xml,
};
use crate::project::{
    Project, ProjectType,
    metadata_edit::{EditError, escape_xml},
    metadata_model::{MetadataKind, MetadataObject},
    metadata_rename::{transaction::Guard, validate_name},
    metadata_workspace::RefreshReport,
    metadata_write::{DirectoryChange, FileChange},
};

// Platform classes are shared identities; only their contained objects are allocated per configuration.
const CLASSES: [&str; 7] = [
    "9cd510cd-abfc-11d4-9434-004095e12fc7",
    "9fcd25a0-4822-11d4-9414-008048da11f9",
    "e3687481-0a87-462c-a166-9f34594f9bba",
    "9de14907-ec23-4a07-96f0-85521cb6b53b",
    "51f2d5d8-ea4d-4064-8892-82951750031e",
    "e68182ea-4237-4383-967f-90c1e3370bc7",
    "fb282519-d103-4dd3-bc12-cb271d631dfc",
];

/// Empty scaffolds may contain only the generated, empty Git placeholder.
///
/// # Errors
/// Rejects source aliases, special entries and unreadable paths.
pub fn empty_source(project: &Project) -> Result<bool, ObjectError> {
    let source = project.source();
    if source.canonicalize().map_err(|error| io(source, error))? != source {
        return Err(EditError::UnsafePath.into());
    }
    for entry in fs::read_dir(source).map_err(|error| io(source, error))? {
        let entry = entry.map_err(|error| io(source, error))?;
        let metadata = entry.metadata().map_err(|error| io(&entry.path(), error))?;
        if entry.file_name() != ".gitkeep"
            || !entry
                .file_type()
                .map_err(|error| io(&entry.path(), error))?
                .is_file()
            || metadata.len() != 0
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Discover the first operation before a Designer root descriptor exists.
#[must_use]
pub fn schema() -> ObjectSchema {
    ObjectSchema {
        schema_version: 1,
        kinds: vec![MetadataKind::Configuration],
        module_roles: Vec::new(),
        parent: None,
        operations: vec!["create"],
        prepared_plans: true,
        required_properties: BTreeMap::new(),
        reference_properties: BTreeMap::new(),
        type_properties: BTreeMap::new(),
        unique_properties: BTreeMap::new(),
    }
}

/// Prepare a managed configuration and its default Russian language without touching sources.
///
/// # Errors
/// Requires an empty configuration scaffold and rejects injected paths, identities and unsupported overrides.
pub fn preview(project: &Project, request: &ObjectRequest) -> Result<ObjectPlan, ObjectError> {
    plan(project, request, None)
}

/// Rebuild a saved first-configuration plan using exactly the reviewed identities.
///
/// # Errors
/// Rejects changed scaffold contents or a modified prepared plan.
pub fn preview_prepared(
    project: &Project,
    prepared: &PreparedObject,
) -> Result<ObjectPlan, ObjectError> {
    let plan = plan(project, &prepared.request, Some(&prepared.identities))?;
    if prepared.source_snapshot != plan.prepared.source_snapshot
        || prepared.expected_output_snapshot != plan.prepared.expected_output_snapshot
    {
        return Err(EditError::Conflict.into());
    }
    Ok(plan)
}

/// Publish the first descriptor under the same write lock and recovery journal as later authoring.
///
/// # Errors
/// Rejects occupied destinations and stale plans; interrupted writes use the shared recovery protocol.
pub fn apply(project: &Project, prepared: &PreparedObject) -> Result<ObjectResult, ObjectError> {
    let guard = Guard::acquire(project.root())?;
    guard.check_clear(project.source())?;
    let plan = preview_prepared(project, prepared)?;
    guard.publish_files(project.source(), &plan.files, &plan.directories)?;
    Ok(ObjectResult {
        refresh: RefreshReport {
            generation: 0,
            affected: plan.object_id.iter().cloned().collect(),
        },
        object_id: plan.object_id,
        snapshot: plan.prepared.expected_output_snapshot,
    })
}

fn plan(
    project: &Project,
    request: &ObjectRequest,
    saved: Option<&BTreeMap<String, String>>,
) -> Result<ObjectPlan, ObjectError> {
    if project.configuration().project_type() != ProjectType::Configuration {
        return Err(ObjectError::Unsupported(MetadataKind::Configuration));
    }
    if !empty_source(project)? {
        return Err(invalid("source", "configuration_requires_empty_source"));
    }
    let ObjectRequest::Create {
        kind: MetadataKind::Configuration,
        name,
        parent: None,
        template: None,
        property_values,
        property_references,
        property_types,
    } = request
    else {
        return Err(invalid("request", "configuration_bootstrap_required"));
    };
    validate_name(name).map_err(ObjectError::Name)?;
    if !property_values.is_empty() || !property_references.is_empty() || !property_types.is_empty()
    {
        return Err(invalid(
            "propertyValues",
            "edit_configuration_after_creation",
        ));
    }
    let mut slots = BTreeSet::from(["object".to_owned(), "language".to_owned()]);
    slots.extend(
        CLASSES
            .into_iter()
            .map(|class| format!("contained:{class}")),
    );
    let identities = templates::allocate_slots(slots, saved)?;
    let id = MetadataObject::new(
        MetadataKind::Configuration,
        name.clone(),
        identity(&identities, "object")?.to_owned(),
        None,
    )
    .map_err(|_| invalid("name", "invalid_name"))?
    .id()
    .clone();
    let files = vec![
        FileChange {
            path: "Configuration.xml".into(),
            before: None,
            after: Some(configuration(name, &identities)?.into_bytes()),
        },
        FileChange {
            path: "Languages/Русский.xml".into(),
            before: None,
            after: Some(language(&identities)?.into_bytes()),
        },
        FileChange {
            path: "Ext/ParentConfigurations.bin".into(),
            before: None,
            after: Some(b"{6,0,0,0,0,0}".to_vec()),
        },
    ];
    let directories = ["Ext", "Languages"]
        .map(|path| DirectoryChange {
            path: path.into(),
            before: false,
            after: true,
        })
        .to_vec();
    let mut inputs = Inputs::default();
    inputs.read(project.source(), Path::new(".gitkeep"))?;
    for file in &files {
        inputs.read(project.source(), &file.path)?;
    }
    for directory in &directories {
        inputs.directory(project.source(), &directory.path)?;
    }
    let mut plan = ObjectPlan {
        prepared: PreparedObject {
            request: request.clone(),
            source_snapshot: inputs.snapshot(request)?,
            identities,
            expected_output_snapshot: String::new(),
        },
        object_id: Some(id),
        files,
        directories,
        references: Vec::new(),
        inputs: Vec::new(),
        input_checks: super::inventory::InputChecks::default(),
    };
    plan.prepared.expected_output_snapshot = output_snapshot(&plan);
    Ok(plan)
}

fn identity<'a>(
    identities: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, ObjectError> {
    identities
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| invalid("identities", "missing_slot"))
}

fn configuration(name: &str, identities: &BTreeMap<String, String>) -> Result<String, ObjectError> {
    let name = escape_xml(name)?;
    let uuid = identity(identities, "object")?;
    let mut internal = String::new();
    for class in CLASSES {
        let _ = write!(
            internal,
            "<xr:ContainedObject><xr:ClassId>{class}</xr:ClassId><xr:ObjectId>{}</xr:ObjectId></xr:ContainedObject>",
            identity(identities, &format!("contained:{class}"))?
        );
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MetaDataObject xmlns=\"{}\" xmlns:xr=\"{}\" version=\"2.20\">\n\t<Configuration uuid=\"{uuid}\"><InternalInfo>{internal}</InternalInfo><Properties><Name>{name}</Name><Synonym/><Comment/><DefaultRunMode>ManagedApplication</DefaultRunMode><ScriptVariant>Russian</ScriptVariant><DefaultLanguage>Language.Русский</DefaultLanguage></Properties><ChildObjects><Language>Русский</Language></ChildObjects></Configuration>\n</MetaDataObject>\n",
        xml::MD,
        xml::XR
    ))
}

fn language(identities: &BTreeMap<String, String>) -> Result<String, ObjectError> {
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MetaDataObject xmlns=\"{}\" version=\"2.20\">\n\t<Language uuid=\"{}\"><Properties><Name>Русский</Name><Synonym/><Comment/><LanguageCode>ru</LanguageCode></Properties></Language>\n</MetaDataObject>\n",
        xml::MD,
        identity(identities, "language")?
    ))
}

fn io(path: &Path, source: std::io::Error) -> ObjectError {
    ObjectError::Io {
        path: PathBuf::from(path),
        source,
    }
}
