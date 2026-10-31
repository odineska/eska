//! Change exactly one declared configuration link while retaining TOML policy and comments.

use super::{ConfigurationReference, Edit, Error, KnownLibrary, Project, ProjectType, Workspace};
use toml_edit::{DocumentMut, Item, Table};

pub(super) fn prepare(
    project: &Project,
    workspace: Option<&Workspace>,
    library: KnownLibrary,
) -> Result<Edit, Error> {
    let kind = project.configuration().project_type();
    if !matches!(kind, ProjectType::Processing | ProjectType::Report) {
        return Err(Error::UnsupportedType(kind));
    }
    let owner = workspace
        .map(|workspace| crate::application::owner(project, workspace))
        .transpose()
        .map_err(|error| Error::Context(Box::new(error)))?
        .flatten();
    let root = workspace.map_or_else(|| project.root(), Workspace::root);
    let path = root.join("eska.toml");
    if workspace.is_some() && owner.is_none() {
        return Err(Error::Conflict {
            path,
            field: "workspace.members".into(),
        });
    }
    let declaration = owner.map_or_else(
        || {
            project
                .configuration()
                .analysis_settings()
                .options()
                .configuration()
        },
        |(_, application)| application.configuration_reference(),
    );
    if owner.is_some_and(|(_, application)| application.configuration().is_some())
        || declaration.is_some_and(|declaration| {
            declaration != &ConfigurationReference::Library(library.name().into())
        })
    {
        return Err(Error::Conflict {
            path,
            field: owner.map_or_else(
                || "analysis.configuration".into(),
                |(name, _)| format!("workspace.members.{name}.configuration"),
            ),
        });
    }
    let mut edit = Edit::read(&path)?;
    if edit.before.is_none() {
        return Err(Error::Changed(path));
    }
    if declaration.is_some() {
        return Ok(edit);
    }
    let mut document = edit.document()?;
    if let Some((name, _)) = owner {
        let workspace = table(&mut document["workspace"])?;
        let members = table(&mut workspace["members"])?;
        let application = table(&mut members[name.as_str()])?;
        let mut link = toml_edit::InlineTable::new();
        link.insert("library", library.name().into());
        application["configuration"] = toml_edit::value(link);
    } else {
        let analysis = table(&mut document["analysis"])?;
        let configuration = table(&mut analysis["configuration"])?;
        configuration["library"] = toml_edit::value(library.name());
    }
    validate_document(&document, workspace)?;
    edit.set(document.to_string().into_bytes());
    Ok(edit)
}

fn validate_document(document: &DocumentMut, workspace: Option<&Workspace>) -> Result<(), Error> {
    let text = document.to_string();
    if let Some(workspace) = workspace {
        let config = crate::config::WorkspaceConfig::from_toml(&text)
            .map_err(|error| Error::Workspace(Box::new(error)))?;
        config
            .resolve_analysis(workspace.members())
            .map_err(|error| Error::Workspace(Box::new(error)))?;
    } else {
        crate::config::ProjectConfig::from_toml(&text)
            .map_err(|error| Error::Project(Box::new(error)))?;
    }
    Ok(())
}

pub(super) fn table(item: &mut Item) -> Result<&mut Table, Error> {
    if item.is_none() {
        *item = Item::Table(Table::new());
    }
    if let Some(inline) = item.as_inline_table() {
        let comment = inline
            .decor()
            .suffix()
            .and_then(toml_edit::RawString::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let mut expanded = inline.clone().into_table();
        if !comment.is_empty() {
            expanded.decor_mut().set_prefix(format!("\n{comment}\n"));
        }
        *item = Item::Table(expanded);
    }
    item.as_table_mut().ok_or(Error::Download)
}
