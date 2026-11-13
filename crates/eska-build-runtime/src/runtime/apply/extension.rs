//! Resolve an explicitly selected workspace extension and verify its native identity.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use eska_project_core::{
    application::project_application,
    project::{
        Project, ProjectName, ProjectType, designer_xml,
        discovery::{self, DiscoveryContext},
    },
};
use serde::Serialize;

use super::{ApplyError, descriptor, structure};

const METADATA: &str = "http://v8.1c.ru/8.3/MDClasses";
const DUMP_INFO: &str = "http://v8.1c.ru/8.3/xcf/dumpinfo";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Identity {
    pub name: String,
    pub uuid: String,
}

pub(super) struct Selection {
    pub base: Project,
    pub identity: Identity,
    pub dependencies: Vec<Identity>,
    pub dependents: Vec<String>,
}

pub(super) fn resolve(
    project: &Project,
    include_dependents: bool,
) -> Result<Selection, ApplyError> {
    let DiscoveryContext::Workspace {
        workspace,
        current_member: Some(selected),
    } = discovery::discover_context(project.root())
        .map_err(|_| ApplyError::ExtensionApplication)?
    else {
        return Err(ApplyError::ExtensionApplication);
    };
    let (_, application) = project_application(project, &workspace)
        .map_err(|_| ApplyError::ExtensionApplication)?
        .filter(|(_, application)| application.extensions().contains(&selected))
        .ok_or(ApplyError::ExtensionApplication)?;
    if application
        .extension_scope(&selected)
        .is_some_and(|scope| !scope.is_default())
    {
        return Err(ApplyError::ExtensionScope);
    }
    let base = application
        .configuration()
        .and_then(|name| {
            workspace
                .members()
                .iter()
                .find(|member| member.name() == name)
        })
        .ok_or(ApplyError::ExtensionApplication)?;
    let mut members = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut uuids = BTreeSet::new();
    for member in workspace
        .members()
        .iter()
        .filter(|member| application.extensions().contains(member.name()))
    {
        let extension =
            identity(member.project().source()).ok_or(ApplyError::ExtensionDescriptor)?;
        if !names.insert(extension.name.to_lowercase()) || !uuids.insert(extension.uuid.clone()) {
            return Err(ApplyError::ExtensionAmbiguous);
        }
        members.insert(member.name(), extension);
    }
    let dependents = if include_dependents {
        members
            .iter()
            .filter(|(member, _)| {
                dependency_closure(member, application.extension_dependencies()).contains(&selected)
            })
            .map(|(_, identity)| identity.name.clone())
            .collect()
    } else {
        Vec::new()
    };
    let identity = members
        .remove(&selected)
        .ok_or(ApplyError::ExtensionApplication)?;
    let dependencies = dependency_closure(&selected, application.extension_dependencies())
        .into_iter()
        .map(|name| {
            if application
                .extension_scope(name)
                .is_some_and(|scope| !scope.is_default())
            {
                return Err(ApplyError::ExtensionScope);
            }
            members.remove(name).ok_or(ApplyError::ExtensionApplication)
        })
        .collect::<Result<_, _>>()?;
    Ok(Selection {
        base: base.project().clone(),
        identity,
        dependencies,
        dependents,
    })
}

pub(super) fn dependency_closure<'a>(
    selected: &'a ProjectName,
    edges: &'a BTreeMap<ProjectName, Vec<ProjectName>>,
) -> BTreeSet<&'a ProjectName> {
    let mut pending = vec![selected];
    let mut required = BTreeSet::new();
    while let Some(member) = pending.pop() {
        for dependency in edges.get(member).into_iter().flatten() {
            if required.insert(dependency) {
                pending.push(dependency);
            }
        }
    }
    required
}

pub(super) fn identity(source: &Path) -> Option<Identity> {
    let text = descriptor::read_xml(&source.join(structure::ROOT))?;
    let document = descriptor::parse_xml(text.trim_start_matches('\u{feff}'))?;
    let root = document.root_element();
    if designer_xml::project_type_from_root(root) != Some(ProjectType::Extension) {
        return None;
    }
    let object = root.first_element_child()?;
    let uuid = object.attribute("uuid")?;
    let properties = only_child(object, METADATA, "Properties")?;
    let name = only_child(properties, METADATA, "Name")?.text()?;
    if !descriptor::valid_name(name) || !descriptor::valid_uuid(uuid) {
        return None;
    }
    Some(Identity {
        name: name.to_owned(),
        uuid: uuid.to_ascii_lowercase(),
    })
}

/// Read the exact root entry from the platform's XML metadata, excluding child metadata.
pub(super) fn installed_uuid(output: &Path, name: &str) -> Option<String> {
    let text = descriptor::read_xml(&output.join("ConfigDumpInfo.xml"))?;
    let document = descriptor::parse_xml(text.trim_start_matches('\u{feff}'))?;
    let root = document.root_element();
    if !root.has_tag_name((DUMP_INFO, "ConfigDumpInfo")) {
        return None;
    }
    let versions = only_child(root, DUMP_INFO, "ConfigVersions")?;
    let root_name = format!("Configuration.{name}");
    let mut entries = versions.children().filter(|node| {
        node.has_tag_name((DUMP_INFO, "Metadata"))
            && node.attribute("name") == Some(root_name.as_str())
    });
    let uuid = entries.next()?.attribute("id")?;
    (entries.next().is_none() && descriptor::valid_uuid(uuid)).then(|| uuid.to_ascii_lowercase())
}

/// Read the fixed field names from `ibcmd config extension list`.
/// Missing fields or diagnostics leave absence unproven.
pub(super) fn listed(output: &[u8], name: &str) -> Option<bool> {
    let name = name.to_lowercase();
    Some(
        inventory(output)?
            .iter()
            .any(|(listed, _)| listed.to_lowercase() == name),
    )
}

pub(super) fn inventory(output: &[u8]) -> Option<Vec<(&str, bool)>> {
    let text = std::str::from_utf8(output).ok()?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let mut entries = Vec::new();
    while let Some(line) = lines.next() {
        let listed = field(line, "name")?.strip_prefix('"')?.strip_suffix('"')?;
        if !descriptor::valid_name(listed) {
            return None;
        }
        for key in [
            "version",
            "active",
            "purpose",
            "safe-mode",
            "security-profile-name",
            "unsafe-action-protection",
            "used-in-distributed-infobase",
        ] {
            field(lines.next()?, key)?;
        }
        let infobase_scope = match field(lines.next()?, "scope")? {
            "infobase" => true,
            "data-separation" => false,
            _ => return None,
        };
        field(lines.next()?, "hash-sum")?;
        entries.push((listed, infobase_scope));
    }
    Some(entries)
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    (key.trim() == name).then(|| value.trim())
}

fn only_child<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    namespace: &str,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'i>> {
    let mut children = node
        .children()
        .filter(|child| child.has_tag_name((namespace, tag)));
    let first = children.next()?;
    children.next().is_none().then_some(first)
}
