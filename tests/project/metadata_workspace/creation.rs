//! Creation is a typed source operation, independent of CLI prompts and IDE state.

use super::*;
use crate::backend::project::{
    metadata_create::{
        CreateRequest, CreateType, CreationError, CreationKind, DateFractions, PreparedCreate,
    },
    metadata_edit::{EditError, PropertyChange, snapshot},
    metadata_model::LocalizedText,
};

const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
const CORE: &str = "http://v8.1c.ru/8.1/data/core";

fn request(
    project: &mut crate::backend::project::metadata_workspace::ProjectSession,
    owner: &ObjectId,
    name: &str,
) -> CreateRequest {
    CreateRequest {
        owner: owner.clone(),
        snapshot: project.creation_schema(owner).unwrap().snapshot,
        kind: CreationKind::Attribute,
        name: name.to_owned(),
        synonyms: vec![LocalizedText {
            language: "ru".to_owned(),
            content: "Новое поле & данные".to_owned(),
        }],
        comment: "A < B & C".to_owned(),
        types: vec![CreateType::String {
            length: 200,
            fixed: false,
        }],
    }
}

#[test]
fn creation_preview_is_read_only_and_apply_publishes_exact_allocated_identity() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let source = directory.0.join("src");
    let before = bytes(&source);
    let before_project = bytes(&directory.0);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Document, "Продажа", None);
    let input = request(project, &owner, "Дополнение");
    let plan = project.preview_create(&input).unwrap();
    let second = project.preview_create(&input).unwrap();
    assert_ne!(plan.prepared.identity.uuid, second.prepared.identity.uuid);
    assert_eq!(bytes(&source), before);
    assert_eq!(bytes(&directory.0), before_project);
    assert_eq!(
        plan.object_id.as_str(),
        "document:Продажа/attribute:Дополнение"
    );
    assert_eq!(plan.replacements().len(), 1);
    assert_eq!(
        plan.prepared.expected_output_snapshot,
        snapshot(plan.output())
    );
    let saved: PreparedCreate =
        serde_json::from_value(serde_json::to_value(&plan.prepared).unwrap()).unwrap();
    let replay = project.preview_prepared_create(&saved).unwrap();
    assert_eq!(plan.output(), replay.output());
    let result = project.apply_create(&saved).unwrap();
    assert_eq!(
        fs::read_to_string(source.join(&plan.path)).unwrap(),
        plan.output()
    );
    assert_eq!(result.uuid, saved.identity.uuid);
    assert_eq!(
        project.object(&result.object_id).unwrap().uuid.as_deref(),
        Some(result.uuid.as_str())
    );
    let after = bytes(&source);
    let changed: Vec<_> = before
        .iter()
        .filter(|(path, content)| after.get(*path) != Some(content))
        .map(|(path, _)| path)
        .collect();
    assert_eq!(changed, [&plan.path]);
    assert_eq!(after.len(), before.len());
    let fresh = request(project, &owner, "дополнение");
    assert!(matches!(
        project.preview_create(&fresh),
        Err(CreationError::Collision(_))
    ));
    assert_eq!(bytes(&source), after);
}

#[test]
fn table_part_allocates_distinct_native_types_and_accepts_a_nested_composite_field() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Catalog, "Контрагенты", None);
    let mut input = request(project, &owner, "Строки");
    input.kind = CreationKind::TablePart;
    input.types.clear();
    let plan = project.preview_create(&input).unwrap();
    let ids = &plan.prepared.identity;
    let table = ids.table.as_ref().unwrap();
    let row = ids.row.as_ref().unwrap();
    let unique: BTreeSet<_> = [
        &ids.uuid,
        &table.type_id,
        &table.value_id,
        &row.type_id,
        &row.value_id,
    ]
    .into_iter()
    .collect();
    assert_eq!(unique.len(), 5);
    assert!(
        plan.output()
            .contains("CatalogTabularSection.Контрагенты.Строки")
    );
    assert!(
        plan.output()
            .contains("CatalogTabularSectionRow.Контрагенты.Строки")
    );
    let result = project.apply_create(&plan.prepared).unwrap();
    let mut field = request(project, &result.object_id, "Значение");
    field.types = vec![
        CreateType::Reference { object: owner },
        CreateType::Number {
            digits: 15,
            fraction_digits: 2,
            non_negative: true,
        },
        CreateType::Boolean {},
        CreateType::Date {
            fractions: DateFractions::Date,
        },
        CreateType::String {
            length: 0,
            fixed: false,
        },
    ];
    let plan = project.preview_create(&field).unwrap();
    project.apply_create(&plan.prepared).unwrap();
    let xml = roxmltree::Document::parse(plan.output()).unwrap();
    let field = xml
        .descendants()
        .find(|node| node.attribute("uuid") == Some(plan.prepared.identity.uuid.as_str()))
        .unwrap();
    let types = field
        .descendants()
        .filter(|node| node.has_tag_name((CORE, "Type")))
        .count();
    assert_eq!(types, 5);
    assert!(
        plan.output()
            .contains("<v8:AllowedSign>Nonnegative</v8:AllowedSign>")
    );
}

#[test]
fn creation_rejects_stale_or_modified_prepared_plans_without_writing() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let source = directory.0.join("src");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Document, "Продажа", None);
    let input = request(project, &owner, "Новый");
    let plan = project.preview_create(&input).unwrap();
    let before = bytes(&source);
    for index in 0..4 {
        let mut changed = plan.prepared.clone();
        match index {
            0 => changed.request.name.push('А'),
            1 => changed.request.types = vec![CreateType::Boolean {}],
            2 => changed.identity.uuid = "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_owned(),
            _ => changed.expected_output_snapshot.clear(),
        }
        assert!(project.apply_create(&changed).is_err());
        assert_eq!(bytes(&source), before);
    }
    let path = source.join(&plan.path);
    fs::write(
        &path,
        format!("{}\n<!-- user edit -->", fs::read_to_string(&path).unwrap()),
    )
    .unwrap();
    let modified = bytes(&source);
    assert!(matches!(
        project.apply_create(&plan.prepared),
        Err(CreationError::Edit(EditError::Conflict))
    ));
    assert_eq!(bytes(&source), modified);
}

#[test]
fn reserved_names_cross_kind_collisions_and_normalized_qualifiers_are_rejected() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Catalog, "Контрагенты", None);
    let before = bytes(&directory.0.join("src"));
    for name in [
        "Code",
        "Код",
        "Description",
        "Наименование",
        "Parent",
        "Родитель",
        "Owner",
        "Владелец",
        "ThisObject",
        "ЭтотОбъект",
        "Неправильное.Имя",
    ] {
        let input = request(project, &owner, name);
        assert!(project.preview_create(&input).is_err(), "{name}");
    }
    for value in [
        CreateType::String {
            length: 1025,
            fixed: false,
        },
        CreateType::String {
            length: 101,
            fixed: true,
        },
        CreateType::String {
            length: 0,
            fixed: true,
        },
        CreateType::Number {
            digits: 39,
            fraction_digits: 0,
            non_negative: false,
        },
        CreateType::Number {
            digits: 15,
            fraction_digits: 16,
            non_negative: false,
        },
    ] {
        let mut input = request(project, &owner, "Поле");
        input.types = vec![value];
        assert!(matches!(
            project.preview_create(&input),
            Err(CreationError::Invalid { field: "types", .. })
        ));
    }
    let mut input = request(project, &owner, "ЧислоБезРазрядности");
    input.types = vec![CreateType::Number {
        digits: 0,
        fraction_digits: 0,
        non_negative: false,
    }];
    assert!(project.preview_create(&input).is_ok());
    let document = object(MetadataKind::Document, "Продажа", None);
    let input = request(project, &document, "Товары");
    assert!(matches!(
        project.preview_create(&input),
        Err(CreationError::Collision(_))
    ));
    assert_eq!(bytes(&directory.0.join("src")), before);
}

#[test]
fn reference_targets_and_root_declarations_are_reread_before_publication() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let source = directory.0.join("src");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Document, "Продажа", None);
    let target = object(MetadataKind::Catalog, "Контрагенты", None);
    let mut input = request(project, &owner, "СсылкаНаОбъект");
    input.types = vec![CreateType::Reference { object: target }];
    let plan = project.preview_create(&input).unwrap();
    let path = source.join("Catalogs/Контрагенты.xml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        original.replace("<Name>Контрагенты</Name>", "<Name>Другой</Name>"),
    )
    .unwrap();
    let before = bytes(&source);
    assert!(project.apply_create(&plan.prepared).is_err());
    assert_eq!(bytes(&source), before);
    fs::write(&path, original).unwrap();
    let root = source.join("Configuration.xml");
    let input = fs::read_to_string(&root)
        .unwrap()
        .replace("<Catalog>Контрагенты</Catalog>", "");
    fs::write(&root, input).unwrap();
    let before = bytes(&source);
    assert!(project.apply_create(&plan.prepared).is_err());
    assert_eq!(bytes(&source), before);
}

#[test]
fn structural_creation_invalidates_previous_scalar_history_without_deleting_new_metadata() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let source = directory.0.join("src");
    let path = source.join("Documents/Продажа.xml");
    let input = fs::read_to_string(&path).unwrap().replacen(
        "<Name>Продажа</Name>",
        "<Name>Продажа</Name><Comment>before</Comment>",
        1,
    );
    fs::write(&path, input).unwrap();
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Document, "Продажа", None);
    let state = project.property_editing(&owner).unwrap();
    let field = state
        .properties
        .fields
        .iter()
        .find(|field| field.path[0].key.name == "Comment")
        .unwrap();
    project
        .update_property(
            &owner,
            &state.properties.snapshot,
            None,
            &field.path,
            &PropertyChange::Text {
                value: "after".to_owned(),
            },
        )
        .unwrap();
    assert!(project.property_editing(&owner).unwrap().undo.is_some());
    let input = request(project, &owner, "Поле");
    let plan = project.preview_create(&input).unwrap();
    project.apply_create(&plan.prepared).unwrap();
    let current = project.property_editing(&owner).unwrap();
    assert!(current.undo.is_none());
    let before = bytes(&source);
    assert!(
        project
            .undo_property(&owner, &current.properties.snapshot, None, true)
            .is_err()
    );
    assert_eq!(bytes(&source), before);
}

#[test]
fn saved_creation_input_is_strict_including_localized_text_and_identity() {
    let value = serde_json::json!({
        "owner":"document:Probe", "snapshot":"hash", "kind":"attribute", "name":"Field",
        "synonyms":[{"language":"ru", "content":"Текст", "xml":"<escape/>"}],
        "types":[{"kind":"boolean"}]
    });
    assert!(serde_json::from_value::<CreateRequest>(value).is_err());
    let value = serde_json::json!({"kind":"boolean", "length":10});
    assert!(serde_json::from_value::<CreateType>(value).is_err());
}

#[test]
fn namespace_shadowing_and_empty_containers_preserve_existing_bytes_and_type_identity() {
    for ending in ["\n", "\r\n", "\r"] {
        for children in [
            "<m:ChildObjects xmlns='urn:foreign' xmlns:v8='urn:shadow' xmlns:xs='urn:shadow'/>",
            "<m:ChildObjects xmlns='urn:foreign' xmlns:v8='urn:shadow' xmlns:xs='urn:shadow'></m:ChildObjects>",
            "",
        ] {
            let directory = TestDir::new();
            fixture(&directory.0, "configuration");
            let path = directory.0.join("src/Documents/Продажа.xml");
            let original = format!("\u{feff}<?xml version='1.0' encoding='UTF-8'?>\n<m:MetaDataObject xmlns:m='{MD}' xmlns:v8='{CORE}' xmlns:xs='http://www.w3.org/2001/XMLSchema' version='2.20'>\n  <m:Document uuid='11111111-1111-1111-1111-111111111111'>\n    <m:Properties><m:Name>Продажа</m:Name><foreign xmlns='urn:user' flag='keep'> unchanged </foreign></m:Properties>\n    {children}\n  </m:Document>\n</m:MetaDataObject>").replace('\n', ending);
            fs::write(&path, &original).unwrap();
            let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
            let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
            let owner = object(MetadataKind::Document, "Продажа", None);
            let input = request(project, &owner, "НовоеПоле");
            let plan = project.preview_create(&input).unwrap();
            let mut reproduced = original;
            for change in plan.replacements().iter().rev() {
                reproduced.replace_range(change.range.clone(), &change.text);
            }
            assert_eq!(reproduced, plan.output());
            assert!(plan.output().starts_with('\u{feff}'));
            assert!(
                plan.output()
                    .contains("<foreign xmlns='urn:user' flag='keep'> unchanged </foreign>")
            );
            let parsed = roxmltree::Document::parse(plan.output()).unwrap();
            let created = parsed
                .descendants()
                .find(|node| node.attribute("uuid") == Some(plan.prepared.identity.uuid.as_str()))
                .unwrap();
            assert!(created.has_tag_name((MD, "Attribute")));
            let value_type = created
                .descendants()
                .find(|node| node.has_tag_name((CORE, "Type")))
                .unwrap();
            let (prefix, local) = value_type.text().unwrap().split_once(':').unwrap();
            assert_eq!(local, "string");
            assert_eq!(
                value_type.lookup_namespace_uri(Some(prefix)),
                Some("http://www.w3.org/2001/XMLSchema")
            );
            project.apply_create(&plan.prepared).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), plan.output());
        }
    }
}

#[test]
fn external_objects_share_creation_with_native_generated_type_aliases() {
    for (kind, prefix) in [("processing", "DataProcessor"), ("report", "Report")] {
        let directory = TestDir::new();
        fixture(&directory.0, kind);
        let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
        let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
        let NodeId::Object(owner) = project.root().clone() else {
            panic!("root object")
        };
        let mut input = request(project, &owner, "Lines");
        input.kind = CreationKind::TablePart;
        input.types.clear();
        let plan = project.preview_create(&input).unwrap();
        assert!(
            plan.output()
                .contains(&format!("name=\"{prefix}TabularSection."))
        );
        assert!(
            plan.output()
                .contains(&format!("name=\"{prefix}TabularSectionRow."))
        );
        let table = project.apply_create(&plan.prepared).unwrap();
        for name in ["Column", "ThisObject", "ЭтотОбъект"] {
            let input = request(project, &table.object_id, name);
            let plan = project.preview_create(&input).unwrap();
            project.apply_create(&plan.prepared).unwrap();
        }
        for name in ["LineNumber", "НомерСтроки"] {
            let input = request(project, &table.object_id, name);
            assert!(project.preview_create(&input).is_err(), "{name}");
        }
        assert!(
            project
                .creation_schema(&table.object_id)
                .unwrap()
                .references
                .is_empty()
        );
    }
}

#[test]
fn creation_rechecks_readonly_permissions_after_preview_and_rejects_extension_owners() {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Document, "Продажа", None);
    let input = request(project, &owner, "Поле");
    let plan = project.preview_create(&input).unwrap();
    let path = directory.0.join("src").join(&plan.path);
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(!project.creation_schema(&owner).unwrap().writable);
    assert!(matches!(
        project.apply_create(&plan.prepared),
        Err(CreationError::Edit(EditError::ReadOnly))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::set_permissions(&path, original_permissions).unwrap();

    let directory = TestDir::new();
    fixture(&directory.0, "extension");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Catalog, "Контрагенты", None);
    assert!(matches!(
        project.creation_schema(&owner),
        Err(CreationError::UnsupportedOwner(_))
    ));
}

#[test]
fn creation_respects_source_leases_owned_by_an_independent_session() {
    use crate::backend::project::source_lock::SourceLease;
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    let source = directory.0.join("src");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Document, "Продажа", None);
    let input = request(project, &owner, "ЗаблокированныйРеквизит");
    let plan = project.preview_create(&input).unwrap();
    let lease = SourceLease::acquire(&directory.0, &[source.join(&plan.path)]).unwrap();
    let before = bytes(&source);
    assert!(!project.creation_schema(&owner).unwrap().writable);
    assert!(matches!(
        project.apply_create(&plan.prepared),
        Err(CreationError::Edit(EditError::ReadOnly))
    ));
    assert_eq!(bytes(&source), before);
    drop(lease);
    project.apply_create(&plan.prepared).unwrap();
    assert_eq!(
        fs::read_to_string(source.join(&plan.path)).unwrap(),
        plan.output()
    );
}
