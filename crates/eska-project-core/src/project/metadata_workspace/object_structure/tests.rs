use super::*;
use crate::project::{
    metadata_model::{MetadataObject, ProjectScope},
    metadata_objects::xml,
    metadata_workspace::MetadataWorkspace,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

struct Stand(PathBuf);
impl Stand {
    fn new() -> Self {
        let base = PathBuf::from(
            std::env::var_os("ESKA_TEST_ROOT")
                .expect("ESKA_TEST_ROOT must point to sibling eska-playground"),
        );
        let path = base.join(format!(
            "object-structure-{}",
            crate::project::metadata_create::allocate_uuid().unwrap()
        ));
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(path.join("eska.toml"), "[project]\ntype='configuration'\n").unwrap();
        let stand = Self(path);
        stand.write(
            "Configuration.xml",
            &descriptor(
                "Configuration",
                "Demo",
                1,
                "",
                "<Document>Origin</Document><HTTPService>Api</HTTPService>",
            ),
        );
        stand.write("Documents/Origin.xml", &descriptor("Document", "Origin", 2,
            "<Comment>keep Origin verbatim</Comment><DefaultObjectForm>Document.Origin.Form.Main</DefaultObjectForm>",
            "<Attribute uuid=\"00000000-0000-4000-8000-000000000003\"><Properties><Name>Self</Name><Type><v8:Type>cfg:DocumentRef.Origin</v8:Type></Type></Properties></Attribute><Form>Main</Form>"));
        let path = stand.source().join("Documents/Origin.xml");
        let input = fs::read_to_string(&path).unwrap();
        let internal = "<InternalInfo><xr:GeneratedType name=\"DocumentRef.Origin\" category=\"Ref\"><xr:TypeId>00000000-0000-4000-8000-000000000006</xr:TypeId><xr:ValueId>00000000-0000-4000-8000-000000000007</xr:ValueId></xr:GeneratedType><xr:GeneratedType name=\"DocumentObject.Origin\" category=\"Object\"><xr:TypeId>00000000-0000-4000-8000-000000000008</xr:TypeId><xr:ValueId>00000000-0000-4000-8000-000000000009</xr:ValueId></xr:GeneratedType></InternalInfo>";
        fs::write(
            path,
            input.replacen("<Properties>", &format!("{internal}<Properties>"), 1),
        )
        .unwrap();
        stand.write(
            "Documents/Origin/Forms/Main.xml",
            &descriptor("Form", "Main", 4, "<FormType>Managed</FormType>", ""),
        );
        stand.write("Documents/Origin/Forms/Main/Ext/Form.xml", "<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\" xmlns:core=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"http://v8.1c.ru/8.1/data/enterprise/current-config\" version=\"2.20\"><AutoCommandBar name=\"FormCommandBar\" id=\"-1\"/><Attributes><Attribute name=\"Object\" id=\"1\"><Type><core:Type>cfg:DocumentObject.Origin</core:Type></Type></Attribute></Attributes></Form>");
        stand.write(
            "Documents/Origin/Ext/ObjectModule.bsl",
            "Procedure Run()\r\n    Result = Documents.Origin.CreateDocument();\nEndProcedure\r\n",
        );
        stand.write(
            "HTTPServices/Api.xml",
            &descriptor("HTTPService", "Api", 5, "<RootURL>api</RootURL>", ""),
        );
        stand
    }
    fn source(&self) -> PathBuf {
        self.0.join("src")
    }
    fn write(&self, path: &str, value: &str) {
        let path = self.source().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
    fn workspace(&self) -> MetadataWorkspace {
        MetadataWorkspace::open(&self.0, &[], false).unwrap()
    }
}
impl Drop for Stand {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn descriptor(kind: &str, name: &str, id: u32, properties: &str, children: &str) -> String {
    format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<MetaDataObject xmlns=\"{}\" xmlns:xr=\"{}\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"http://v8.1c.ru/8.1/data/enterprise/current-config\" version=\"2.20\"><{kind} uuid=\"00000000-0000-4000-8000-{id:012}\"><Properties><Name>{name}</Name>{properties}</Properties><ChildObjects>{children}</ChildObjects></{kind}></MetaDataObject>\r\n",
        xml::MD,
        xml::XR
    )
}
fn id(kind: MetadataKind, name: &str, parent: Option<&ObjectId>) -> ObjectId {
    MetadataObject::new(kind, name.to_owned(), "test".to_owned(), parent.cloned())
        .unwrap()
        .id()
        .clone()
}
fn copy_request(source: ObjectId, name: &str) -> ObjectRequest {
    ObjectRequest::Copy {
        source,
        name: name.to_owned(),
        parent: None,
        property_values: BTreeMap::new(),
        property_references: BTreeMap::new(),
        property_types: BTreeMap::new(),
    }
}

#[test]
fn deep_copy_preserves_original_resources_and_allocates_owned_identity() {
    let stand = Stand::new();
    let resources = stand.source().join("Documents/Origin/Ext/UnknownResource");
    fs::create_dir_all(resources.join("empty")).unwrap();
    fs::write(resources.join("data.bin"), [0, 255, 17, 10]).unwrap();
    let original = fs::read(stand.source().join("Documents/Origin.xml")).unwrap();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let plan = project
        .preview_object(&copy_request(
            id(MetadataKind::Document, "Origin", None),
            "Twin",
        ))
        .unwrap();
    assert!(!stand.source().join("Documents/Twin.xml").exists());
    assert_eq!(plan.prepared.identities.len(), 7);
    let saved: PreparedObject =
        serde_json::from_value(serde_json::to_value(&plan.prepared).unwrap()).unwrap();
    assert_eq!(
        project.preview_prepared_object(&saved).unwrap().files,
        plan.files
    );
    let result = project.apply_object(&saved).unwrap();
    assert_eq!(result.object_id.unwrap().as_str(), "document:Twin");
    assert_eq!(
        fs::read(stand.source().join("Documents/Origin.xml")).unwrap(),
        original
    );
    let twin = fs::read_to_string(stand.source().join("Documents/Twin.xml")).unwrap();
    assert!(twin.starts_with('\u{feff}'));
    assert!(twin.contains("Document.Twin.Form.Main"));
    assert!(twin.contains("cfg:DocumentRef.Twin"));
    assert!(twin.contains("keep Origin verbatim"));
    assert!(
        fs::read_to_string(stand.source().join("Documents/Twin/Ext/ObjectModule.bsl"))
            .unwrap()
            .contains("Documents.Twin.CreateDocument()")
    );
    assert_eq!(
        fs::read(
            stand
                .source()
                .join("Documents/Twin/Ext/UnknownResource/data.bin")
        )
        .unwrap(),
        [0, 255, 17, 10]
    );
    assert!(
        stand
            .source()
            .join("Documents/Twin/Ext/UnknownResource/empty")
            .is_dir()
    );
    assert!(
        fs::read_to_string(
            stand
                .source()
                .join("Documents/Twin/Forms/Main/Ext/Form.xml")
        )
        .unwrap()
        .contains("cfg:DocumentObject.Twin")
    );
    assert!(matches!(
        project.preview_object(&copy_request(
            id(MetadataKind::Document, "Origin", None),
            "twin"
        )),
        Err(ObjectError::Collision(_))
    ));
}

#[test]
fn saved_plan_rejects_changed_source_and_orphan_destination_tree() {
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let request = copy_request(id(MetadataKind::Document, "Origin", None), "Twin");
    let plan = project.preview_object(&request).unwrap();
    stand.write("Documents/Origin/Ext/ObjectModule.bsl", "// changed\n");
    assert!(matches!(
        project.apply_object(&plan.prepared),
        Err(ObjectError::Edit(EditError::Conflict))
    ));
    assert!(!stand.source().join("Documents/Twin.xml").exists());
    fs::create_dir_all(stand.source().join("Documents/Twin")).unwrap();
    assert!(matches!(
        project.preview_object(&request),
        Err(ObjectError::Collision(_))
    ));
}

#[test]
fn incoming_references_block_delete_until_explicit_force() {
    let stand = Stand::new();
    fs::write(stand.source().join("Opaque.bin"), [0, 255, 17]).unwrap();
    stand.write(
        "Documents/Reference.xml",
        &descriptor(
            "Document",
            "Reference",
            6,
            "<DefaultObjectForm>Document.Origin.Form.Main</DefaultObjectForm>",
            "",
        ),
    );
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let object = id(MetadataKind::Document, "Origin", None);
    let plan = project
        .preview_object(&ObjectRequest::Delete {
            object: object.clone(),
            force: false,
        })
        .unwrap();
    assert!(plan.references.iter().any(|reference| reference.path
        == Path::new("Documents/Reference.xml")
        && reference.confirmed));
    assert!(matches!(
        project.apply_object(&plan.prepared),
        Err(ObjectError::ReviewRequired(_))
    ));
    let plan = project
        .preview_object(&ObjectRequest::Delete {
            object,
            force: true,
        })
        .unwrap();
    project.apply_object(&plan.prepared).unwrap();
    assert!(!stand.source().join("Documents/Origin.xml").exists());
    assert!(!stand.source().join("Documents/Origin").exists());
    assert!(stand.source().join("Documents/Reference.xml").exists());
    assert!(
        !fs::read_to_string(stand.source().join("Configuration.xml"))
            .unwrap()
            .contains("<Document>Origin</Document>")
    );
}

#[test]
fn inline_copy_keeps_siblings_and_renames_only_selected_declaration() {
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = id(MetadataKind::Document, "Origin", None);
    let plan = project
        .preview_object(&copy_request(
            id(MetadataKind::Attribute, "Self", Some(&owner)),
            "Other",
        ))
        .unwrap();
    project.apply_object(&plan.prepared).unwrap();
    let input = fs::read_to_string(stand.source().join("Documents/Origin.xml")).unwrap();
    assert_eq!(input.matches("<Name>Self</Name>").count(), 1);
    assert_eq!(input.matches("<Name>Other</Name>").count(), 1);
    assert_eq!(input.matches("cfg:DocumentRef.Origin").count(), 2);
}

#[test]
fn blank_form_and_catalog_use_native_identity_slots_and_shared_schema() {
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    assert!(
        project
            .object_schema(None)
            .unwrap()
            .kinds
            .contains(&MetadataKind::DocumentNumerator)
    );
    for (kind, name) in [
        (MetadataKind::Catalog, "NewCatalog"),
        (MetadataKind::CommonForm, "NewForm"),
    ] {
        let plan = project
            .preview_object(&ObjectRequest::Create {
                kind,
                name: name.to_owned(),
                parent: None,
                template: None,
                property_values: BTreeMap::new(),
                property_references: BTreeMap::new(),
                property_types: BTreeMap::new(),
            })
            .unwrap();
        assert_eq!(
            plan.prepared.identities.len(),
            if kind == MetadataKind::Catalog { 11 } else { 1 }
        );
        project.apply_object(&plan.prepared).unwrap();
    }
    assert!(
        stand
            .source()
            .join("CommonForms/NewForm/Ext/Form.xml")
            .is_file()
    );
}

#[test]
fn copied_http_service_requires_explicit_unique_property_override() {
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let source = id(MetadataKind::HTTPService, "Api", None);
    assert!(
        matches!(project.preview_object(&copy_request(source.clone(), "Second")), Err(ObjectError::PropertyConflict { property, .. }) if property == "RootURL")
    );
    let request = ObjectRequest::Copy {
        source,
        name: "Second".to_owned(),
        parent: None,
        property_values: BTreeMap::from([("RootURL".to_owned(), "second".to_owned())]),
        property_references: BTreeMap::new(),
        property_types: BTreeMap::new(),
    };
    let plan = project.preview_object(&request).unwrap();
    project.apply_object(&plan.prepared).unwrap();
    assert!(
        fs::read_to_string(stand.source().join("HTTPServices/Second.xml"))
            .unwrap()
            .contains("<RootURL>second</RootURL>")
    );
}

#[test]
fn reference_lists_edit_known_domains_and_insert_missing_optional_container() {
    use crate::project::metadata_edit::{PropertyChange, ScalarSchema};
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let created = project
        .preview_object(&ObjectRequest::Create {
            kind: MetadataKind::InformationRegister,
            name: "Records".to_owned(),
            parent: None,
            template: None,
            property_values: BTreeMap::new(),
            property_references: BTreeMap::new(),
            property_types: BTreeMap::new(),
        })
        .unwrap();
    project.apply_object(&created.prepared).unwrap();
    let object = id(MetadataKind::Document, "Origin", None);
    let editing = project.property_editing(&object).unwrap();
    let field = editing
        .properties
        .fields
        .iter()
        .find(|field| field.path[0].key.name == "RegisterRecords")
        .unwrap();
    assert!(matches!(&field.schema, ScalarSchema::References { values, .. } if values.is_empty()));
    let path = field.path.clone();
    let change = PropertyChange::References {
        values: vec!["InformationRegister.Records".to_owned()],
    };
    project
        .update_property(&object, &editing.properties.snapshot, None, &path, &change)
        .unwrap();
    let editing = project.property_editing(&object).unwrap();
    assert!(editing.properties.fields.iter().any(|field| matches!(&field.schema, ScalarSchema::References { values, .. } if values == &["InformationRegister.Records"])));
    let before = fs::read(stand.source().join("Documents/Origin.xml")).unwrap();
    let invalid = PropertyChange::References {
        values: vec!["HTTPService.Api".to_owned()],
    };
    assert!(
        project
            .update_property(&object, &editing.properties.snapshot, None, &path, &invalid)
            .is_err()
    );
    assert_eq!(
        fs::read(stand.source().join("Documents/Origin.xml")).unwrap(),
        before
    );
    project
        .update_property(&object, &editing.properties.snapshot, None, &path, &change)
        .unwrap();
    assert_eq!(
        fs::read(stand.source().join("Documents/Origin.xml")).unwrap(),
        before
    );
}

#[test]
fn atomic_creation_uses_reference_writer_and_requires_native_semantic_fields() {
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let catalog = project
        .preview_object(&ObjectRequest::Create {
            kind: MetadataKind::Catalog,
            name: "Products".to_owned(),
            parent: None,
            template: None,
            property_values: BTreeMap::new(),
            property_references: BTreeMap::new(),
            property_types: BTreeMap::new(),
        })
        .unwrap();
    project.apply_object(&catalog.prepared).unwrap();
    let mut request = ObjectRequest::Create {
        kind: MetadataKind::FunctionalOptionsParameter,
        name: "Filter".to_owned(),
        parent: None,
        template: None,
        property_values: BTreeMap::new(),
        property_references: BTreeMap::new(),
        property_types: BTreeMap::new(),
    };
    assert!(matches!(
        project.preview_object(&request),
        Err(ObjectError::Invalid {
            field: "Use",
            reason: "required_property"
        })
    ));
    if let ObjectRequest::Create {
        property_references,
        ..
    } = &mut request
    {
        property_references.insert("Use".to_owned(), vec!["Catalog.Products".to_owned()]);
    }
    let plan = project.preview_object(&request).unwrap();
    project.apply_object(&plan.prepared).unwrap();
    let output = fs::read_to_string(
        stand
            .source()
            .join("FunctionalOptionsParameters/Filter.xml"),
    )
    .unwrap();
    assert!(output.contains("Catalog.Products"));
    assert!(output.contains("MDObjectRef"));
}

#[test]
fn atomic_creation_uses_type_membership_and_checks_generated_targets() {
    use crate::project::metadata_model::PropertyKey;
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let mut request = ObjectRequest::Create {
        kind: MetadataKind::EventSubscription,
        name: "BeforeWrite".to_owned(),
        parent: None,
        template: None,
        property_values: BTreeMap::from([
            ("Event".to_owned(), "BeforeWrite".to_owned()),
            (
                "Handler".to_owned(),
                "CommonModule.Handlers.BeforeWrite".to_owned(),
            ),
        ]),
        property_references: BTreeMap::new(),
        property_types: BTreeMap::from([(
            "Source".to_owned(),
            vec![PropertyKey {
                namespace: Some(crate::project::metadata_edit::types::CFG.to_owned()),
                name: "DocumentObject.Missing".to_owned(),
            }],
        )]),
    };
    assert!(project.preview_object(&request).is_err());
    if let ObjectRequest::Create { property_types, .. } = &mut request {
        property_types.get_mut("Source").unwrap()[0].name = "DocumentObject.Origin".to_owned();
    }
    let plan = project.preview_object(&request).unwrap();
    assert!(
        plan.inputs
            .iter()
            .any(|path| path == Path::new("Documents/Origin.xml"))
    );
    project.apply_object(&plan.prepared).unwrap();
    let object = id(MetadataKind::EventSubscription, "BeforeWrite", None);
    let editing = project.property_editing(&object).unwrap();
    let field = editing
        .properties
        .fields
        .iter()
        .find(|field| field.path.len() == 1 && field.path[0].key.name == "Source")
        .unwrap();
    let choices = project.property_type_choices(&object, &field.path).unwrap();
    assert!(
        choices
            .iter()
            .any(|choice| choice.key.name == "DocumentObject.Origin")
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.key.name == "CatalogObject")
    );
    assert!(!choices.iter().any(|choice| choice.key.name == "string"));
}

#[test]
fn copy_protects_read_dependencies_and_rechecks_them_under_batch_lease() {
    let stand = Stand::new();
    let mut workspace = stand.workspace();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let request = copy_request(id(MetadataKind::Document, "Origin", None), "Twin");
    let plan = project.preview_object(&request).unwrap();
    let module = stand.source().join("Documents/Origin/Ext/ObjectModule.bsl");
    let active = SourceLease::acquire(&stand.0, std::slice::from_ref(&module)).unwrap();
    assert!(matches!(
        project.preview_object(&request),
        Err(ObjectError::Edit(EditError::Busy))
    ));
    assert!(matches!(
        project.apply_object(&plan.prepared),
        Err(ObjectError::Edit(EditError::Busy))
    ));
    assert!(!stand.source().join("Documents/Twin.xml").exists());
    drop(active);
    let paths: Vec<_> = plan
        .inputs
        .iter()
        .map(|path| stand.source().join(path))
        .collect();
    let lease = SourceLease::acquire_batch(&stand.0, &paths).unwrap();
    plan.input_checks.verify(&stand.source()).unwrap();
    let added = stand.source().join("Documents/Origin/Ext/Added.bin");
    fs::write(&added, [0, 255]).unwrap();
    assert!(matches!(
        plan.input_checks.verify(&stand.source()),
        Err(ObjectError::Edit(EditError::Conflict))
    ));
    fs::remove_file(added).unwrap();
    plan.input_checks.verify(&stand.source()).unwrap();
    fs::write(&module, "// external editor wrote after preflight\n").unwrap();
    assert!(matches!(
        plan.input_checks.verify(&stand.source()),
        Err(ObjectError::Edit(EditError::Conflict))
    ));
    drop(lease);
    assert!(matches!(
        project.apply_object(&plan.prepared),
        Err(ObjectError::Edit(EditError::Conflict))
    ));
    assert!(!stand.source().join("Documents/Twin.xml").exists());
}
