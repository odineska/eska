//! Managed-form writes reuse the same owner resolution and source protections as metadata edits.

use super::{
    MetadataKind, MetadataWorkspace, ObjectId, ProjectScope, TestDir, bytes, fixture, fs, object,
};
use crate::backend::project::{
    forms::FORM_NAMESPACE,
    metadata_edit::{EditError, snapshot},
    metadata_form_edit::{
        FormEditError, FormEditRequest, FormElementSpec, FormOperation, FormTarget,
    },
    metadata_model::{MetadataProperty, MetadataValue, PropertyKey},
    source_lock::SourceLease,
};

const PATH: &str = "Catalogs/Контрагенты/Forms/ФормаЭлемента/Ext/Form.xml";

fn form_id() -> ObjectId {
    object(
        MetadataKind::Form,
        "ФормаЭлемента",
        Some(object(MetadataKind::Catalog, "Контрагенты", None)),
    )
}

fn add_request(snapshot: String) -> FormEditRequest {
    FormEditRequest {
        form: form_id(),
        snapshot,
        operations: vec![FormOperation::AddElement {
            parent: FormTarget::Root {},
            before: None,
            element: FormElementSpec {
                kind: "UsualGroup".into(),
                name: "Group".into(),
                properties: vec![MetadataProperty {
                    key: PropertyKey {
                        namespace: Some(FORM_NAMESPACE.into()),
                        name: "United".into(),
                    },
                    qualifiers: Vec::new(),
                    value: MetadataValue::Text("true".into()),
                }],
                children: vec![FormElementSpec {
                    kind: "InputField".into(),
                    name: "Value".into(),
                    properties: Vec::new(),
                    children: Vec::new(),
                    auxiliary: Vec::new(),
                }],
                auxiliary: Vec::new(),
            },
        }],
    }
}

#[test]
fn form_preview_is_read_only_and_apply_keeps_payload_identity_and_other_files() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let before = bytes(&directory.0);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let schema = project.form_edit_schema(&form_id()).unwrap();
    assert!(schema.writable);
    let request = add_request(schema.snapshot);
    let plan = project.preview_form_edit(&request).unwrap();
    let repeated = project.preview_prepared_form_edit(&plan.prepared).unwrap();
    assert_eq!(plan.output(), repeated.output());
    assert_eq!(plan.assigned_ids, repeated.assigned_ids);
    assert_eq!(bytes(&directory.0), before);
    let expected = plan.output().to_owned();
    let result = project.apply_form_edit(&plan.prepared).unwrap();
    assert_eq!(
        fs::read_to_string(directory.0.join("src").join(PATH)).unwrap(),
        expected
    );
    assert_eq!(result.snapshot, snapshot(&expected));
    assert!(result.refresh.is_some());
    for (path, original) in &before {
        if path != &std::path::Path::new("src").join(PATH) {
            assert_eq!(
                fs::read(directory.0.join(path)).unwrap(),
                *original,
                "{}",
                path.display()
            );
        }
    }
    assert!(matches!(
        project.apply_form_edit(&plan.prepared),
        Err(FormEditError::Edit(EditError::Conflict))
    ));
    let schema = project.form_edit_schema(&form_id()).unwrap();
    assert_eq!(schema.nodes.len(), 3);
}

#[test]
fn form_apply_rechecks_source_lease_source_bytes_and_prepared_hash() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let path = directory.0.join("src").join(PATH);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let request = add_request(project.form_edit_schema(&form_id()).unwrap().snapshot);
    let plan = project.preview_form_edit(&request).unwrap();
    let lease = SourceLease::acquire(&directory.0, std::slice::from_ref(&path)).unwrap();
    assert!(!project.form_edit_schema(&form_id()).unwrap().writable);
    assert!(matches!(
        project.apply_form_edit(&plan.prepared),
        Err(FormEditError::Edit(EditError::ReadOnly))
    ));
    drop(lease);
    let mut tampered = plan.prepared.clone();
    tampered.expected_output_snapshot = "other".into();
    assert!(matches!(
        project.apply_form_edit(&tampered),
        Err(FormEditError::Edit(EditError::Conflict))
    ));
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{original}\n")).unwrap();
    assert!(matches!(
        project.apply_form_edit(&plan.prepared),
        Err(FormEditError::Edit(EditError::Conflict))
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), format!("{original}\n"));
}

#[test]
fn form_remove_reports_bsl_candidates_and_explicit_force_leaves_module_untouched() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let path = directory.0.join("src").join(PATH);
    fs::write(&path, format!("<Form xmlns='{FORM_NAMESPACE}'><ChildItems><InputField name='Value' id='1'/></ChildItems></Form>")).unwrap();
    let module = path.parent().unwrap().join("Form/Module.bsl");
    let code = "Процедура Выполнить()\n Элементы.Value.Видимость = Истина;\nКонецПроцедуры\n";
    fs::write(&module, code).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let mut request = FormEditRequest {
        form: form_id(),
        snapshot: project.form_edit_schema(&form_id()).unwrap().snapshot,
        operations: vec![FormOperation::Remove {
            target: FormTarget::Element { id: 1 },
            force: false,
        }],
    };
    assert!(matches!(
        project.preview_form_edit(&request),
        Err(FormEditError::References { .. })
    ));
    request.operations = vec![FormOperation::Remove {
        target: FormTarget::Element { id: 1 },
        force: true,
    }];
    let plan = project.preview_form_edit(&request).unwrap();
    assert!(
        plan.inputs
            .iter()
            .any(|path| path.ends_with("Form/Module.bsl"))
    );
    project.apply_form_edit(&plan.prepared).unwrap();
    assert_eq!(fs::read_to_string(module).unwrap(), code);
    assert!(!fs::read_to_string(path).unwrap().contains("InputField"));
}

#[test]
fn form_removal_seals_module_dependencies_and_respects_their_leases() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let path = directory.0.join("src").join(PATH);
    fs::write(&path, format!("<Form xmlns='{FORM_NAMESPACE}'><ChildItems><InputField name='Value' id='1'/></ChildItems></Form>")).unwrap();
    let module = path.parent().unwrap().join("Form/Module.bsl");
    fs::write(&module, "// clean module").unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let request = FormEditRequest {
        form: form_id(),
        snapshot: project.form_edit_schema(&form_id()).unwrap().snapshot,
        operations: vec![FormOperation::Remove {
            target: FormTarget::Element { id: 1 },
            force: false,
        }],
    };
    let plan = project.preview_form_edit(&request).unwrap();
    assert_eq!(plan.inputs.len(), 2);
    let lease = SourceLease::acquire(&directory.0, std::slice::from_ref(&module)).unwrap();
    assert!(matches!(
        project.preview_form_edit(&request),
        Err(FormEditError::Edit(EditError::Busy))
    ));
    assert!(matches!(
        project.apply_form_edit(&plan.prepared),
        Err(FormEditError::Edit(EditError::Busy))
    ));
    drop(lease);
    fs::write(&module, "// changed without any references").unwrap();
    assert!(matches!(
        project.apply_form_edit(&plan.prepared),
        Err(FormEditError::Edit(EditError::Conflict))
    ));
    assert!(fs::read_to_string(path).unwrap().contains("InputField"));
}

#[test]
fn removal_reference_analysis_uses_the_tree_after_prior_operations() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let path = directory.0.join("src").join(PATH);
    fs::write(&path, format!("<Form xmlns='{FORM_NAMESPACE}'><ChildItems><InputField name='Value' id='1'/><UsualGroup name='Group' id='2'/></ChildItems></Form>")).unwrap();
    fs::write(
        path.parent().unwrap().join("Form/Module.bsl"),
        "Процедура Test()\n Элементы.Value.Видимость = Истина;\nКонецПроцедуры\n",
    )
    .unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let request = FormEditRequest {
        form: form_id(),
        snapshot: project.form_edit_schema(&form_id()).unwrap().snapshot,
        operations: vec![
            FormOperation::MoveElement {
                target: FormTarget::Element { id: 1 },
                parent: FormTarget::Element { id: 2 },
                before: None,
            },
            FormOperation::Remove {
                target: FormTarget::Element { id: 2 },
                force: false,
            },
        ],
    };
    assert!(matches!(
        project.preview_form_edit(&request),
        Err(FormEditError::References { .. })
    ));
    assert!(fs::read_to_string(path).unwrap().contains("InputField"));
}
