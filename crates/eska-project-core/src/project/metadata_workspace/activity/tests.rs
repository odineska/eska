use super::*;
use crate::{
    config::ProjectConfig,
    project::{
        ProjectName, ProjectType, create, discovery::discover_context,
        metadata_workspace::search::IndexState,
    },
    test_support::TestDir,
};
use std::{fs, path::Path};

fn fixture() -> (TestDir, DiscoveryContext) {
    let root = TestDir::new();
    for name in ["one", "two"] {
        let member = root.0.join(name);
        create::create(&member, &ProjectConfig::new(ProjectType::Processing)).unwrap();
        fs::write(
            member.join("eska.toml"),
            format!("[project]\nname='{name}'\ntype='processing'\n"),
        )
        .unwrap();
        fs::write(member.join("src/Root.xml"), format!(
            "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><ExternalDataProcessor uuid=\"{name}\"><Properties><Name>{name}</Name></Properties></ExternalDataProcessor></MetaDataObject>"
        )).unwrap();
    }
    fs::write(
        root.0.join("eska.toml"),
        "[workspace]\n[workspace.members.app]\nexternal_objects=['one','two']\n",
    )
    .unwrap();
    let context = discover_context(&root.0).unwrap();
    (root, context)
}
fn scope(name: &str) -> ProjectScope {
    ProjectScope::Member(ProjectName::parse(name.into()).unwrap())
}

#[test]
fn suspension_releases_the_index_and_restores_fresh_monotonic_sessions() {
    let (_root, context) = fixture();
    let mut opened =
        MetadataWorkspace::open_activity(&context, &[], true, false, &State::default()).unwrap();
    let project = opened.workspace.project_mut(&scope("one")).unwrap();
    project.start_search_index();
    project.index_search_step(64);
    assert_ne!(project.search_progress().state, IndexState::NotStarted);
    let before = project.generation();
    let two = opened
        .workspace
        .project(&scope("two"))
        .unwrap()
        .generation();
    let suspended = opened.workspace.suspend(&scope("one")).unwrap();
    assert_eq!(suspended.generation(), before + 1);
    assert!(matches!(
        opened.workspace.project(&scope("one")),
        Err(WorkspaceError::UnknownProject(_))
    ));
    assert_eq!(
        opened
            .workspace
            .project(&scope("two"))
            .unwrap()
            .generation(),
        two
    );
    let suspended = opened.workspace.prepare_resume(suspended).unwrap().cancel();
    assert!(opened.workspace.project(&scope("one")).is_err());
    opened.workspace.prepare_resume(suspended).unwrap().commit();
    let resumed = opened.workspace.project(&scope("one")).unwrap();
    assert_eq!(resumed.generation(), before + 2);
    assert_eq!(resumed.search_progress().state, IndexState::NotStarted);
}

#[test]
fn startup_uses_disabled_summaries_without_reading_or_recreating_their_xml() {
    let (_root, context) = fixture();
    let mut first =
        MetadataWorkspace::open_activity(&context, &[], true, false, &State::default()).unwrap();
    let suspended = first.workspace.suspend(&scope("one")).unwrap();
    let mut state = State::default();
    state.disabled.insert(
        path_key(suspended.project().root()),
        suspended.summary().clone(),
    );
    fs::remove_dir_all(suspended.project().source()).unwrap();
    let mut opened = MetadataWorkspace::open_activity(&context, &[], true, true, &state).unwrap();
    assert_eq!(opened.projects.len(), 2);
    assert_eq!(opened.workspace.projects().len(), 1);
    let inactive = opened
        .projects
        .into_iter()
        .find_map(|project| {
            if let ActivityProject::Inactive(project) = project {
                Some(project)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(inactive.summary(), suspended.summary());
    assert!(!inactive.project().source().exists());
    let failure = opened.workspace.prepare_resume(inactive).unwrap_err();
    assert_eq!(failure.project.summary(), suspended.summary());
    assert!(opened.workspace.project(&scope("one")).is_err());
}

#[test]
fn explicit_source_restrictions_survive_suspend_cancel_and_resume() {
    let (_root, context) = fixture();
    let mut opened =
        MetadataWorkspace::open_activity(&context, &[], true, false, &State::default()).unwrap();
    let project = opened.workspace.project_mut(&scope("one")).unwrap();
    let owner = Path::new("Keep.bsl");
    fs::write(project.project().source().join(owner), "keep").unwrap();
    let descriptor = project.source.descriptor().to_owned();
    let project_root = project.project().root().to_path_buf();
    let source_root = project.project().source().to_path_buf();
    let restriction = project
        .source_protections
        .acquire(&project_root, &source_root, owner, &descriptor)
        .unwrap();
    assert_eq!(project.source_protections.describe(owner).len(), 1);
    let suspended = opened.workspace.suspend(&scope("one")).unwrap();
    let preview = suspended.protection_session().unwrap();
    assert_eq!(preview.source_protections.describe(owner).len(), 1);
    drop(preview);
    let suspended = opened.workspace.prepare_resume(suspended).unwrap().cancel();
    opened.workspace.prepare_resume(suspended).unwrap().commit();
    let project = opened.workspace.project(&scope("one")).unwrap();
    assert_eq!(project.source_protections.describe(owner).len(), 1);
    drop(restriction);
    assert!(project.source_protections.describe(owner).is_empty());
}
