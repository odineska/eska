use super::*;
use crate::{
    application::ActivityProjection,
    project::{
        activity::{RootSummary, State, path_key},
        discovery::{DiscoveryContext, discover_context},
    },
};
use std::collections::BTreeSet;

fn name(value: &str) -> ProjectName {
    ProjectName::parse(value.into()).unwrap()
}
fn disable(state: &mut State, workspace: &crate::project::Workspace, member: &str) {
    let project = workspace.member(&name(member)).unwrap();
    state.disabled.insert(
        path_key(project.root()),
        RootSummary {
            id: serde_json::from_str("\"configuration:Демо\"").unwrap(),
            name: "Демо".into(),
        },
    );
}

#[test]
fn activity_keeps_only_actual_dependencies_and_scopes_shared_consumers_by_application() {
    let fixture = TestDir::new();
    for (member, kind) in [
        ("base", "configuration"),
        ("api", "extension"),
        ("consumer", "extension"),
        ("spare", "extension"),
        ("report", "report"),
        ("other", "processing"),
    ] {
        project(&fixture.0.join(member), kind, Some(member), None);
    }
    workspace(
        &fixture.0,
        &["base", "api", "consumer", "spare", "report", "other"],
        "[workspace.members.one]\nconfiguration='base'\nextensions=[{path='api'},{path='consumer',depends_on=['api']},{path='spare'}]\nexternal_objects=['report']\n[workspace.members.two]\nconfiguration='base'\nextensions=[{path='api'}]\nexternal_objects=['other']\n",
    );
    let DiscoveryContext::Workspace { workspace, .. } = discover_context(&fixture.0).unwrap()
    else {
        panic!("workspace")
    };
    let mut state = State::default();
    for member in ["base", "api", "spare", "report"] {
        disable(&mut state, &workspace, member);
    }
    let projection = ActivityProjection::for_workspace(&workspace, &state).unwrap();
    assert_eq!(
        projection.applications[&name("one")],
        BTreeSet::from([name("base"), name("api"), name("consumer")])
    );
    assert_eq!(
        projection.applications[&name("two")],
        BTreeSet::from([name("base"), name("api"), name("other")])
    );
    assert_eq!(projection.projects[&name("base")].required_by.len(), 2);
    assert!(projection.projects[&name("spare")].required_by.is_empty());
    assert!(projection.projects[&name("report")].required_by.is_empty());
    assert!(
        projection.projects[&name("base")]
            .required_by
            .iter()
            .any(
                |consumer| consumer.application == name("two") && consumer.project == name("other")
            )
    );
    for member in ["consumer", "other"] {
        disable(&mut state, &workspace, member);
    }
    let empty = ActivityProjection::for_workspace(&workspace, &state).unwrap();
    assert!(empty.applications.values().all(BTreeSet::is_empty));
    state
        .disabled
        .remove(&path_key(workspace.member(&name("report")).unwrap().root()));
    let report = ActivityProjection::for_workspace(&workspace, &state).unwrap();
    assert_eq!(
        report.applications[&name("one")],
        BTreeSet::from([
            name("base"),
            name("api"),
            name("consumer"),
            name("spare"),
            name("report")
        ])
    );
    assert!(report.applications[&name("two")].is_empty());
}

#[test]
fn activity_projection_reads_no_xml_and_never_changes_cli_application_composition() {
    let fixture = TestDir::new();
    let first = project(&fixture.0.join("one"), "processing", Some("one"), None);
    let second = project(&fixture.0.join("two"), "report", Some("two"), None);
    workspace(
        &fixture.0,
        &["one", "two"],
        "[workspace.members.app]\nexternal_objects=['one','two']\n",
    );
    let before = resolve(&fixture.0, Some("app")).unwrap();
    assert_eq!(before.sources().len(), 2);
    let DiscoveryContext::Workspace { workspace, .. } = discover_context(&fixture.0).unwrap()
    else {
        panic!("workspace")
    };
    let mut state = State::default();
    disable(&mut state, &workspace, "one");
    let selected = ActivityProjection::for_workspace(&workspace, &state).unwrap();
    assert_eq!(
        selected.applications[&name("app")],
        BTreeSet::from([name("two")])
    );
    assert_eq!(
        resolve(&fixture.0, Some("app")).unwrap().sources(),
        before.sources()
    );
    fs::remove_file(first).unwrap();
    let context = crate::application::resolve_active(&fixture.0, Some("app"), &state)
        .unwrap()
        .unwrap();
    assert_eq!(context.sources().len(), 1);
    assert_eq!(context.sources()[0].member(), Some(&name("two")));
    assert_eq!(
        context.excluded_sources(),
        &[workspace.member(&name("one")).unwrap().project().source()]
    );
    disable(&mut state, &workspace, "two");
    assert!(
        crate::application::resolve_active(&fixture.0, Some("app"), &state)
            .unwrap()
            .is_none()
    );
    fs::remove_file(second).unwrap();
    assert_eq!(
        {
            state
                .disabled
                .remove(&path_key(workspace.member(&name("two")).unwrap().root()));
            ActivityProjection::for_workspace(&workspace, &state).unwrap()
        },
        selected
    );
}
