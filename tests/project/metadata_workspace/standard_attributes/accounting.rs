//! Only the selected accounting register and its explicitly linked metadata are read.

use super::*;
use crate::backend::project::{configurator::TreeLabel, metadata_workspace::CacheLimits};

fn chart(count: u32) -> String {
    descriptor(
        "ChartOfAccounts",
        "Main",
        &format!(
            "<MaxExtDimensionCount>{count}</MaxExtDimensionCount><ExtDimensionTypes>ChartOfCharacteristicTypes.Dimensions</ExtDimensionTypes>"
        ),
        "",
    )
}

fn dimension_types(value: &str) -> String {
    descriptor(
        "ChartOfCharacteristicTypes",
        "Dimensions",
        &format!(
            "<Type xmlns:v8='http://v8.1c.ru/8.1/data/core' xmlns:xs='http://www.w3.org/2001/XMLSchema'><v8:Type>xs:{value}</v8:Type></Type>"
        ),
        "",
    )
}

pub(in crate::project::metadata_workspace) fn fixture(root: &Path, count: u32) -> ObjectId {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype='configuration'\n").unwrap();
    fs::write(root.join("src/Configuration.xml"), descriptor("Configuration", "Example", "<CompatibilityMode>DontUse</CompatibilityMode>", "<AccountingRegister>Entries</AccountingRegister><ChartOfAccounts>Main</ChartOfAccounts><ChartOfCharacteristicTypes>Dimensions</ChartOfCharacteristicTypes><Document>Unrelated</Document>")).unwrap();
    for (path, xml) in [
        (
            "AccountingRegisters/Entries.xml",
            descriptor(
                "AccountingRegister",
                "Entries",
                "<ChartOfAccounts>ChartOfAccounts.Main</ChartOfAccounts><Correspondence>true</Correspondence><PeriodAdjustmentLength>1</PeriodAdjustmentLength>",
                "",
            ),
        ),
        ("ChartsOfAccounts/Main.xml", chart(count)),
        (
            "ChartsOfCharacteristicTypes/Dimensions.xml",
            dimension_types("string"),
        ),
        (
            "Documents/Unrelated.xml",
            "Broken unrelated XML must never be read".into(),
        ),
    ] {
        let path = root.join("src").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, xml).unwrap();
    }
    object(MetadataKind::AccountingRegister, "Entries", None)
}

fn standard(owner: &ObjectId, name: &str) -> NodeId {
    NodeId::StandardAttribute {
        owner: owner.clone(),
        name: name.into(),
    }
}

fn type_name(properties: &[crate::backend::project::metadata_model::MetadataProperty]) -> &str {
    let MetadataValue::Record(fields) = &properties
        .iter()
        .find(|field| field.key.name == "Type")
        .unwrap()
        .value
    else {
        panic!("type record")
    };
    let MetadataValue::QualifiedText { key, .. } = &fields[0].value else {
        panic!("qualified type")
    };
    &key.name
}

#[test]
fn accounting_standard_attributes_are_lazy_typed_and_bounded_to_declared_dependencies() {
    let directory = TestDir::new();
    let owner = fixture(&directory.0, 3);
    let before = bytes(&directory.0);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let initial = project.cache_stats().parses;
    project
        .children(&NodeId::Object(owner.clone()), TreeOptions::default())
        .unwrap();
    assert_eq!(
        project.cache_stats().parses,
        initial + 1,
        "Expanding the register must not read its assigned chart yet"
    );
    let nodes = attributes(project, &owner);
    assert_eq!(project.cache_stats().parses, initial + 3);
    assert_eq!(nodes.len(), 12);
    assert!(!nodes.contains(&standard(&owner, "RecordType")));
    assert!(!nodes.contains(&standard(&owner, "AccountDr")));
    assert!(!nodes.contains(&standard(&owner, "AccountCr")));
    assert!(nodes.contains(&standard(&owner, "ExtDimension3")));
    assert!(!nodes.contains(&standard(&owner, "ExtDimension4")));
    let value = standard(&owner, "ExtDimension2");
    assert!(
        matches!(&project.node(&value).unwrap().label, TreeLabel::IndexedKey { key, index: 2 } if key.ends_with("-ExtDimension"))
    );
    assert_eq!(
        type_name(&project.standard_attribute_properties(&value).unwrap()),
        "string"
    );
    assert_eq!(
        type_name(
            &project
                .standard_attribute_properties(&standard(&owner, "ExtDimensionType2"))
                .unwrap()
        ),
        "ChartOfCharacteristicTypesRef.Dimensions"
    );
    assert_eq!(project.cache_stats().parses, initial + 3);
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn accounting_standard_dependencies_refresh_without_a_scan_or_retained_descriptor_requirement() {
    let directory = TestDir::new();
    let owner = fixture(&directory.0, 2);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    project.set_cache_limits(CacheLimits {
        descriptors: 1,
        source_bytes: 1024 * 1024,
    });
    assert!(attributes(project, &owner).contains(&standard(&owner, "ExtDimension2")));
    fs::write(directory.0.join("src/ChartsOfAccounts/Main.xml"), chart(1)).unwrap();
    let changed = project
        .changed_paths(&[PathBuf::from("ChartsOfAccounts/Main.xml")])
        .unwrap();
    assert!(changed.affected.contains(&owner));
    let names = attributes(project, &owner);
    assert!(!names.contains(&standard(&owner, "ExtDimension2")));
    let value = standard(&owner, "ExtDimension1");
    assert!(names.contains(&value));
    fs::write(
        directory
            .0
            .join("src/ChartsOfCharacteristicTypes/Dimensions.xml"),
        dimension_types("decimal"),
    )
    .unwrap();
    assert!(matches!(
        project.standard_attribute_properties(&value),
        Err(WorkspaceError::SourceChanged(_))
    ));
    let changed = project
        .changed_paths(&[PathBuf::from("ChartsOfCharacteristicTypes/Dimensions.xml")])
        .unwrap();
    assert!(changed.affected.contains(&owner));
    attributes(project, &owner);
    assert_eq!(
        type_name(&project.standard_attribute_properties(&value).unwrap()),
        "decimal"
    );
}

#[test]
fn accounting_standard_dimension_limit_errors_recover_after_target_correction() {
    let directory = TestDir::new();
    let owner = fixture(&directory.0, 51);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    project
        .children(&NodeId::Object(owner.clone()), TreeOptions::default())
        .unwrap();
    let group = NodeId::Collection {
        owner: owner.clone(),
        kind: CollectionKind::StandardAttributes,
    };
    assert!(matches!(
        project.children(&group, TreeOptions::default()),
        Err(WorkspaceError::Tree(_))
    ));
    assert_eq!(project.node(&group).unwrap().state, ChildrenState::Error);
    fs::write(directory.0.join("src/ChartsOfAccounts/Main.xml"), chart(1)).unwrap();
    let changed = project
        .changed_paths(&[PathBuf::from("ChartsOfAccounts/Main.xml")])
        .unwrap();
    assert!(changed.affected.contains(&owner));
    assert!(attributes(project, &owner).contains(&standard(&owner, "ExtDimension1")));
}

#[test]
fn removing_a_declared_accounting_dependency_cannot_graft_its_stale_standard_fields() {
    let directory = TestDir::new();
    let owner = fixture(&directory.0, 2);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let dimension = standard(&owner, "ExtDimension2");
    assert!(attributes(project, &owner).contains(&dimension));
    let path = directory.0.join("src/Configuration.xml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        original.replace("<ChartOfAccounts>Main</ChartOfAccounts>", ""),
    )
    .unwrap();
    let changed = project
        .changed_paths(&[PathBuf::from("Configuration.xml")])
        .unwrap();
    assert!(changed.affected.contains(&owner));
    assert!(project.node(&dimension).is_err());
    project
        .children(&NodeId::Object(owner.clone()), TreeOptions::default())
        .unwrap();
    let group = NodeId::Collection {
        owner: owner.clone(),
        kind: CollectionKind::StandardAttributes,
    };
    assert!(matches!(
        project.children(&group, TreeOptions::default()),
        Err(WorkspaceError::UnknownObject(_))
    ));
    fs::write(&path, original).unwrap();
    project
        .changed_paths(&[PathBuf::from("Configuration.xml")])
        .unwrap();
    assert!(attributes(project, &owner).contains(&dimension));
}
