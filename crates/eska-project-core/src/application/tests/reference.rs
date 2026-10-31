use super::*;
use crate::{
    application::resolve_project_reference,
    project::discovery::{self, DiscoveryContext},
};

#[test]
fn standalone_reference_does_not_require_unrelated_root_or_module_contents() {
    let fixture = TestDir::new();
    let root = fixture.0.join("report");
    let descriptor = project(&root, "report", None, None);
    fs::remove_file(descriptor).unwrap();
    let DiscoveryContext::Standalone(selected) = discovery::discover_context(&root).unwrap() else {
        panic!()
    };
    assert!(
        resolve_project_reference(&selected, None)
            .unwrap()
            .is_none()
    );
    let base = project(&fixture.0.join("base"), "configuration", None, None);
    fs::remove_file(fixture.0.join("base/eska.toml")).unwrap();
    let manifest = root.join("eska.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[analysis.configuration]\nsource='../base/исходники XML'\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    let DiscoveryContext::Standalone(selected) = discovery::discover_context(&root).unwrap() else {
        panic!()
    };
    let before = snapshot(&fixture.0);
    let resolved = resolve_project_reference(&selected, None).unwrap().unwrap();
    assert_eq!(
        resolved.project().source().join(resolved.descriptor()),
        base
    );
    assert!(resolved.is_reference());
    assert_eq!(snapshot(&fixture.0), before);
}

#[test]
fn each_member_uses_its_owner_even_when_workspace_default_or_other_xml_is_invalid() {
    let fixture = TestDir::new();
    for name in ["one", "two"] {
        project(&fixture.0.join(name), "report", Some(name), None);
        project(
            &fixture.0.join(format!("base-{name}")),
            "configuration",
            None,
            None,
        );
    }
    workspace(
        &fixture.0,
        &["one", "two"],
        "[analysis]\ndefault_application='first'\n[workspace.members.first]\nconfiguration={source='base-one/исходники XML'}\nexternal_objects=['one']\n[workspace.members.second]\nconfiguration={source='base-two/исходники XML'}\nexternal_objects=['two']\n",
    );
    fs::remove_file(fixture.0.join("base-one/исходники XML/Configuration.xml")).unwrap();
    let DiscoveryContext::Workspace { workspace, .. } =
        discovery::discover_context(&fixture.0).unwrap()
    else {
        panic!()
    };
    let selected = workspace
        .members()
        .iter()
        .find(|member| member.name().as_str() == "two")
        .unwrap();
    let source = resolve_project_reference(selected.project(), Some(&workspace))
        .unwrap()
        .unwrap();
    assert_eq!(
        source.project().source(),
        fixture.0.join("base-two/исходники XML")
    );
}

#[test]
fn shared_member_requires_an_owning_default_and_preserves_error_coordinates() {
    let fixture = TestDir::new();
    project(&fixture.0.join("shared"), "report", Some("shared"), None);
    for name in ["one", "two"] {
        project(
            &fixture.0.join(format!("base-{name}")),
            "configuration",
            None,
            None,
        );
    }
    let apps = "[workspace.members.first]\nconfiguration={source='base-one/исходники XML'}\nexternal_objects=['shared']\n[workspace.members.second]\nconfiguration={source='base-two/исходники XML'}\nexternal_objects=['shared']\n";
    workspace(&fixture.0, &["shared"], apps);
    let DiscoveryContext::Workspace { workspace, .. } =
        discovery::discover_context(&fixture.0).unwrap()
    else {
        panic!()
    };
    let error =
        resolve_project_reference(workspace.members()[0].project(), Some(&workspace)).unwrap_err();
    assert_eq!(error.path, fixture.0.join("eska.toml"));
    assert_eq!(error.field, "analysis.default_application");
    assert!(matches!(
        *error.source,
        ContextError::AmbiguousApplication { .. }
    ));
    super::workspace(
        &fixture.0,
        &["shared"],
        &format!("[analysis]\ndefault_application='second'\n{apps}"),
    );
    let DiscoveryContext::Workspace { workspace, .. } =
        discovery::discover_context(&fixture.0).unwrap()
    else {
        panic!()
    };
    let source = resolve_project_reference(workspace.members()[0].project(), Some(&workspace))
        .unwrap()
        .unwrap();
    assert_eq!(
        source.project().source(),
        fixture.0.join("base-two/исходники XML")
    );
}

#[test]
fn reference_cannot_alias_an_unselected_writable_workspace_member() {
    let fixture = TestDir::new();
    project(&fixture.0.join("report"), "report", Some("report"), None);
    project(&fixture.0.join("base"), "configuration", Some("base"), None);
    workspace(
        &fixture.0,
        &["report", "base"],
        "[workspace.members.app]\nconfiguration={source='base/исходники XML'}\nexternal_objects=['report']\n",
    );
    let DiscoveryContext::Workspace { workspace, .. } =
        discovery::discover_context(&fixture.0).unwrap()
    else {
        panic!()
    };
    let selected = workspace
        .members()
        .iter()
        .find(|member| member.name().as_str() == "report")
        .unwrap();
    let error = resolve_project_reference(selected.project(), Some(&workspace)).unwrap_err();
    assert_eq!(error.field, "configuration");
    assert_eq!(error.application.unwrap().as_str(), "app");
    assert!(
        matches!(*error.source, ContextError::Settings(error) if matches!(*error, SettingsError::ConfigurationReferenceScope))
    );
}
