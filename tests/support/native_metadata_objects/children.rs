use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use eska_project_core::project::metadata_model::MetadataKind as K;
use roxmltree::{Document, Node};
use serde_json::{Value, json};

use super::{MD, Stand, strings, verify};

const KINDS: &[K] = &[
    K::Form,
    K::Template,
    K::Command,
    K::Attribute,
    K::TabularSection,
    K::AddressingAttribute,
    K::Dimension,
    K::Resource,
    K::EnumValue,
    K::AccountingFlag,
    K::ExtDimensionAccountingFlag,
    K::Recalculation,
    K::Column,
    K::URLTemplate,
    K::Method,
    K::Operation,
    K::Parameter,
    K::IntegrationServiceChannel,
    K::Subsystem,
];

pub fn run() {
    let source = PathBuf::from(
        std::env::var_os("ESKA_NATIVE_METADATA_SOURCE")
            .expect("select the complete public native fixture"),
    );
    let stand = Stand::from_source(&source);
    let mut parents = roots(&stand);
    let chart = &parents[&K::ChartOfCalculationTypes];
    let dependence = stand.property(
        chart,
        "DependenceOnCalculationTypes",
        &json!({"kind":"text","value":"OnRegistrationPeriod"}),
    );
    let bases = stand.property(
        chart,
        "BaseCalculationTypes",
        &json!({"kind":"references","values":[reference(chart,K::ChartOfCalculationTypes)]}),
    );
    stand.mutate(
        "nested-container",
        &strings(&[
            "create",
            "--kind",
            "subsystem",
            "--name",
            "NativeNestedContainer",
        ]),
    );
    parents.insert(K::Subsystem, "subsystem:NativeNestedContainer".to_owned());
    let attribute = stand.mutate(
        "journal-attribute",
        &strings(&[
            "create",
            "--kind",
            "attribute",
            "--name",
            "NativeJournalValue",
            "--parent",
            &parents[&K::Document],
            "--type",
            "Type=xs:boolean",
        ]),
    );
    let mut plans = vec![attribute.clone()];
    let samples = create_children(&stand, &mut parents, &mut plans);
    let twins = copy(&stand, &samples, &mut plans);
    let export = stand.roundtrip("nested", true);
    verify_plans(&stand, &export, &plans);
    for ((kind, _, id), twin) in samples.iter().zip(&twins).rev() {
        for (suffix, object) in [("twin", twin), ("source", id)] {
            stand.mutate(
                &format!("delete-{}-{suffix}", kind.as_str()),
                &strings(&["delete", "--object", object, "--yes"]),
            );
        }
    }
    stand.mutate(
        "delete-journal-attribute",
        &strings(&[
            "delete",
            "--object",
            attribute["result"]["objectId"].as_str().unwrap(),
            "--yes",
        ]),
    );
    stand.mutate(
        "delete-nested-container",
        &strings(&[
            "delete",
            "--object",
            "subsystem:NativeNestedContainer",
            "--yes",
        ]),
    );
    let chart = &parents[&K::ChartOfCalculationTypes];
    stand.property(
        chart,
        "BaseCalculationTypes",
        &json!({"kind":"references","values":bases["schema"]["values"]}),
    );
    stand.property(
        chart,
        "DependenceOnCalculationTypes",
        &json!({"kind":"text","value":dependence["value"]}),
    );
    stand.roundtrip("nested-deleted", false);
    eprintln!("Native nested matrix: 18 child kinds + nested Subsystem, create/copy/delete passed");
}

fn create_children(
    stand: &Stand,
    parents: &mut BTreeMap<K, String>,
    plans: &mut Vec<Value>,
) -> Vec<(K, String, String)> {
    let mut samples = Vec::new();
    for kind in KINDS {
        let name = format!("NativeChild{}", kind.designer_tag());
        let parent = parents[&parent_kind(*kind)].clone();
        let request = request(*kind, &name, &parent, parents);
        let input = stand.root.0.join("nested-input.json");
        fs::write(
            &input,
            serde_json::to_vec(&json!({"schemaVersion":1,"request":request})).unwrap(),
        )
        .unwrap();
        let plan = stand.mutate(
            &format!("create-{}", kind.as_str()),
            &strings(&["create", "--input", input.to_str().unwrap()]),
        );
        let id = plan["result"]["objectId"].as_str().unwrap().to_owned();
        parents.insert(*kind, id.clone());
        samples.push((*kind, name, id));
        plans.push(plan);
    }
    samples
}

fn roots(stand: &Stand) -> BTreeMap<K, String> {
    let xml = fs::read_to_string(stand.source.join("Configuration.xml")).unwrap();
    let document = Document::parse(&xml).unwrap();
    document
        .descendants()
        .find(|node| node.has_tag_name((MD, "ChildObjects")))
        .unwrap()
        .children()
        .filter(Node::is_element)
        .map(|node| {
            let kind = K::from_xml_tag(node.tag_name().name()).unwrap();
            (kind, format!("{}:{}", kind.as_str(), node.text().unwrap()))
        })
        .collect()
}

fn parent_kind(kind: K) -> K {
    match kind {
        K::Form | K::Template | K::Command | K::Attribute | K::TabularSection => K::Catalog,
        K::AddressingAttribute => K::Task,
        K::Dimension | K::Resource => K::InformationRegister,
        K::EnumValue => K::Enum,
        K::AccountingFlag | K::ExtDimensionAccountingFlag => K::ChartOfAccounts,
        K::Recalculation => K::CalculationRegister,
        K::Column => K::DocumentJournal,
        K::URLTemplate => K::HTTPService,
        K::Method => K::URLTemplate,
        K::Operation => K::WebService,
        K::Parameter => K::Operation,
        K::IntegrationServiceChannel => K::IntegrationService,
        K::Subsystem => K::Subsystem,
        _ => panic!("not a nested native case: {kind:?}"),
    }
}

fn request(kind: K, name: &str, parent: &str, parents: &BTreeMap<K, String>) -> Value {
    let mut values = json!({});
    let mut types = json!({});
    let mut references = json!({});
    match kind {
        K::Command => values["Group"] = json!("FormCommandBarImportant"),
        K::URLTemplate => values["Template"] = json!("/native"),
        K::Method => {
            values["HTTPMethod"] = json!("GET");
            values["Handler"] = json!("NativeHandler");
        }
        K::Operation => values["ProcedureName"] = json!("NativeOperation"),
        K::Column => {
            references["References"] = json!([format!(
                "{}.Attribute.NativeJournalValue",
                reference(&parents[&K::Document], K::Document)
            )]);
        }
        K::Attribute
        | K::AddressingAttribute
        | K::Dimension
        | K::Resource
        | K::AccountingFlag
        | K::ExtDimensionAccountingFlag => {
            types["Type"] = json!([{"namespace":"http://www.w3.org/2001/XMLSchema","name":if kind==K::Resource {"decimal"} else {"boolean"}}]);
        }
        _ => {}
    }
    json!({"operation":"create","kind":kind,"parent":parent,"name":name,"propertyValues":values,"propertyTypes":types,"propertyReferences":references})
}

fn copy(stand: &Stand, samples: &[(K, String, String)], plans: &mut Vec<Value>) -> Vec<String> {
    let mut result = Vec::new();
    for (kind, name, id) in samples.iter().rev() {
        let mut args = strings(&["copy", "--source", id, "--name", &format!("{name}Twin")]);
        match kind {
            K::URLTemplate => args.extend(strings(&["--set", "Template=/native-twin"])),
            K::Method => args.extend(strings(&["--set", "HTTPMethod=POST"])),
            _ => {}
        }
        let plan = stand.mutate(&format!("copy-{}", kind.as_str()), &args);
        result.push(plan["result"]["objectId"].as_str().unwrap().to_owned());
        plans.push(plan);
    }
    result.reverse();
    result
}

fn verify_plans(stand: &Stand, export: &std::path::Path, plans: &[Value]) {
    let paths: BTreeSet<_> = plans
        .iter()
        .flat_map(|plan| plan["result"]["files"].as_array().unwrap())
        .map(|file| PathBuf::from(file["path"]["value"].as_str().unwrap()))
        .collect();
    let mut all_ids = BTreeSet::new();
    for path in paths {
        let expected = fs::read(stand.source.join(&path)).unwrap();
        let observed = fs::read(export.join(&path)).unwrap();
        verify::native_file(&expected, &observed, &path);
        all_ids.extend(verify::identities(&observed));
    }
    let mut fresh = BTreeSet::new();
    for plan in plans {
        for value in plan["result"]["prepared"]["identities"]
            .as_object()
            .unwrap()
            .values()
        {
            let value = value.as_str().unwrap().to_owned();
            assert!(all_ids.contains(&value));
            assert!(fresh.insert(value), "cross-case UUID collision");
        }
    }
}

fn reference(id: &str, kind: K) -> String {
    format!("{}.{}", kind.designer_tag(), id.split_once(':').unwrap().1)
}
