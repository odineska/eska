//! Platform fields are visible without XML overrides and never become mutation targets.

use super::*;
use crate::backend::project::metadata_model::MetadataValue;

pub(super) mod accounting;

pub(super) fn descriptor(kind: &str, name: &str, properties: &str, children: &str) -> String {
    format!(
        "<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses' xmlns:r='http://v8.1c.ru/8.3/xcf/readable'><{kind} uuid='00000000-0000-0000-0000-000000000001'><Properties><Name>{name}</Name>{properties}</Properties><ChildObjects>{children}</ChildObjects></{kind}></MetaDataObject>"
    )
}

pub(super) fn source(root: &Path, properties: &str) {
    fs::create_dir_all(root.join("src/Catalogs")).unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype='configuration'\n").unwrap();
    fs::write(
        root.join("src/Configuration.xml"),
        descriptor(
            "Configuration",
            "Example",
            "<CompatibilityMode>DontUse</CompatibilityMode>",
            "<Catalog>Goods</Catalog>",
        ),
    )
    .unwrap();
    fs::write(
        root.join("src/Catalogs/Goods.xml"),
        descriptor("Catalog", "Goods", properties, ""),
    )
    .unwrap();
}

fn attributes(
    project: &mut crate::backend::project::metadata_workspace::ProjectSession,
    owner: &ObjectId,
) -> Vec<NodeId> {
    let group = NodeId::Collection {
        owner: owner.clone(),
        kind: CollectionKind::StandardAttributes,
    };
    assert!(
        project
            .children(&NodeId::Object(owner.clone()), TreeOptions::default())
            .unwrap()
            .iter()
            .any(|node| node.id == group)
    );
    project
        .children(&group, TreeOptions::default())
        .unwrap()
        .into_iter()
        .map(|node| node.id.clone())
        .collect()
}

#[test]
fn standard_attributes_are_implicit_read_only_and_owner_qualified() {
    let directory = TestDir::new();
    source(
        &directory.0,
        "<CodeLength>9</CodeLength><CodeType>Number</CodeType><DescriptionLength>50</DescriptionLength><Hierarchical>true</Hierarchical><HierarchyType>HierarchyFoldersAndItems</HierarchyType><Owners><r:Item>Catalog.Companies</r:Item></Owners>",
    );
    let before = bytes(&directory.0);
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Catalog, "Goods", None);
    let nodes = attributes(project, &owner);
    let names: Vec<_> = nodes
        .iter()
        .map(|id| match id {
            NodeId::StandardAttribute { name, .. } => name.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        [
            "Ref",
            "DeletionMark",
            "Predefined",
            "PredefinedDataName",
            "Code",
            "Description",
            "Parent",
            "IsFolder",
            "Owner"
        ]
    );
    for node in nodes {
        assert_eq!(project.source(&node).unwrap().len(), 0);
        assert_eq!(
            project
                .children(&node, TreeOptions::default())
                .unwrap()
                .len(),
            0
        );
        assert!(project.node(&node).unwrap().metadata_kind.is_none());
        let properties = project.standard_attribute_properties(&node).unwrap();
        assert_eq!(properties[0].key.name, "Name");
        let value_type = properties
            .iter()
            .find(|property| property.key.name == "Type")
            .unwrap();
        let MetadataValue::Record(fields) = &value_type.value else {
            panic!("type record expected")
        };
        if matches!(&node, NodeId::StandardAttribute { name, .. } if name == "Code") {
            assert!(
                matches!(&fields[0].value, MetadataValue::QualifiedText {key,..} if key.name == "decimal")
            );
            assert!(
                matches!(&fields[1].value, MetadataValue::Record(qualifiers) if qualifiers[0].value == MetadataValue::Text("9".into()))
            );
        }
    }
    let disabled = NodeId::StandardAttribute {
        owner,
        name: "Date".into(),
    };
    assert!(matches!(
        project.standard_attribute_properties(&disabled),
        Err(WorkspaceError::UnknownNode(_))
    ));
    assert_eq!(bytes(&directory.0), before);
}

#[test]
fn standard_attribute_overrides_and_availability_follow_refresh() {
    let directory = TestDir::new();
    source(
        &directory.0,
        "<CodeLength>9</CodeLength><StandardAttributes><r:StandardAttribute name='Code'><r:Synonym><r:item/></r:Synonym><r:Comment>Saved code comment</r:Comment></r:StandardAttribute></StandardAttributes>",
    );
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    let owner = object(MetadataKind::Catalog, "Goods", None);
    attributes(project, &owner);
    let code = NodeId::StandardAttribute {
        owner: owner.clone(),
        name: "Code".into(),
    };
    let properties = project.standard_attribute_properties(&code).unwrap();
    assert!(properties.iter().any(|field| field.key.name == "Comment"
        && field.value == MetadataValue::Text("Saved code comment".into())));
    fs::write(directory.0.join("src/Catalogs/Goods.xml"), descriptor("Catalog", "Goods", "<CodeLength>0</CodeLength><DescriptionLength>0</DescriptionLength><Hierarchical>false</Hierarchical>", "")).unwrap();
    project.refresh(&NodeId::Object(owner.clone())).unwrap();
    let nodes = attributes(project, &owner);
    assert!(nodes.contains(&code));
    let fields = project.standard_attribute_properties(&code).unwrap();
    assert!(!fields.iter().any(|field| field.key.name == "Comment"));
    let value_type = fields
        .iter()
        .find(|field| field.key.name == "Type")
        .unwrap();
    assert!(
        matches!(&value_type.value, MetadataValue::Record(fields) if matches!(&fields[1].value, MetadataValue::Record(values) if values[0].value == MetadataValue::Text("0".into())))
    );
}

#[test]
fn standard_attribute_settings_are_isolated_between_projects() {
    let first = TestDir::new();
    let second = TestDir::new();
    source(&first.0, "<CodeLength>9</CodeLength>");
    source(&second.0, "<CodeLength>0</CodeLength>");
    let mut left = MetadataWorkspace::open(&first.0, &[], false).unwrap();
    let mut right = MetadataWorkspace::open(&second.0, &[], false).unwrap();
    let owner = object(MetadataKind::Catalog, "Goods", None);
    let code = NodeId::StandardAttribute {
        owner: owner.clone(),
        name: "Code".into(),
    };
    assert!(
        attributes(left.project_mut(&ProjectScope::Standalone).unwrap(), &owner).contains(&code)
    );
    assert!(
        attributes(
            right.project_mut(&ProjectScope::Standalone).unwrap(),
            &owner
        )
        .contains(&code)
    );
    let left = left
        .project_mut(&ProjectScope::Standalone)
        .unwrap()
        .standard_attribute_properties(&code)
        .unwrap();
    let right = right
        .project_mut(&ProjectScope::Standalone)
        .unwrap()
        .standard_attribute_properties(&code)
        .unwrap();
    assert_ne!(
        left, right,
        "The same virtual identity retains project-local type qualifiers"
    );
}

#[test]
fn standard_attribute_compatibility_survives_cache_eviction_and_reacts_to_root_changes() {
    use crate::backend::project::metadata_workspace::CacheLimits;
    let directory = TestDir::new();
    source(&directory.0, "<CodeLength>9</CodeLength>");
    let mut workspace = MetadataWorkspace::open(&directory.0, &[], false).unwrap();
    let project = workspace.project_mut(&ProjectScope::Standalone).unwrap();
    project.set_cache_limits(CacheLimits {
        descriptors: 1,
        source_bytes: 1024 * 1024,
    });
    let owner = object(MetadataKind::Catalog, "Goods", None);
    let predefined_name = NodeId::StandardAttribute {
        owner: owner.clone(),
        name: "PredefinedDataName".into(),
    };
    assert!(attributes(project, &owner).contains(&predefined_name));
    let parses = project.cache_stats().parses;
    project
        .standard_attribute_properties(&predefined_name)
        .unwrap();
    project
        .standard_attribute_properties(&predefined_name)
        .unwrap();
    assert_eq!(
        project.cache_stats().parses,
        parses,
        "Reading an implicit field must not reload the root or evict its owner"
    );
    fs::write(
        directory.0.join("src/Configuration.xml"),
        descriptor(
            "Configuration",
            "Example",
            "<CompatibilityMode>Version8_3_2</CompatibilityMode>",
            "<Catalog>Goods</Catalog>",
        ),
    )
    .unwrap();
    project
        .changed_paths(&[PathBuf::from("Configuration.xml")])
        .unwrap();
    assert!(!attributes(project, &owner).contains(&predefined_name));
    assert!(matches!(
        project.standard_attribute_properties(&predefined_name),
        Err(WorkspaceError::UnknownNode(_))
    ));
}
