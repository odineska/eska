use std::{collections::BTreeSet, fs, path::PathBuf};

use eska_project_core::project::metadata_model::MetadataKind;
use roxmltree::{Document, Node};
use serde_json::{Value, json};

use super::{MD, Stand, strings, twins, verify};

const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const CFG: &str = "http://v8.1c.ru/8.1/data/enterprise/current-config";

pub fn run() {
    let input = PathBuf::from(
        std::env::var_os("ESKA_NATIVE_METADATA_SOURCE")
            .expect("select a native source containing an event subscription and a scheduled job"),
    );
    let stand = Stand::from_source(&input);
    let prototypes = prototypes(&stand);
    let dependencies = [
        MetadataKind::Catalog,
        MetadataKind::Constant,
        MetadataKind::Document,
        MetadataKind::Task,
        MetadataKind::ChartOfAccounts,
        MetadataKind::ChartOfCalculationTypes,
    ];
    let kinds: Vec<_> = dependencies
        .iter()
        .chain(
            MetadataKind::ALL
                .iter()
                .filter(|kind| kind.collection_folder().is_some() && !dependencies.contains(kind)),
        )
        .copied()
        .collect();
    let identity_count = create_roots(&stand, &kinds, &prototypes);
    bind_registers(&stand);
    stand.mutate(
        "event-named",
        &strings(&[
            "create",
            "--kind",
            "event-subscription",
            "--name",
            "NativeNamedEvent",
            "--type",
            "Source=cfg:DocumentObject.NativeBlankDocument",
            "--set",
            "Event=BeforeWrite",
            "--set",
            &format!("Handler={}", prototypes["Handler"].as_str().unwrap()),
        ]),
    );
    let export = stand.roundtrip("blank", true);
    verify_export(&stand, &export, &kinds, &prototypes);
    stand.mutate(
        "delete-event-named",
        &strings(&[
            "delete",
            "--object",
            "event-subscription:NativeNamedEvent",
            "--yes",
        ]),
    );
    twins::references(&stand, &id(MetadataKind::Document), &[]);
    for kind in kinds.iter().rev() {
        stand.mutate(
            &format!("delete-blank-{}", kind.as_str()),
            &strings(&["delete", "--object", &id(*kind), "--yes"]),
        );
    }
    stand.roundtrip("blank-deleted", false);
    eprintln!(
        "Native blank matrix: {} families, {} fresh root identities",
        kinds.len(),
        identity_count
    );
}

fn create_roots(stand: &Stand, kinds: &[MetadataKind], prototypes: &Value) -> usize {
    let mut identities = BTreeSet::new();
    for kind in kinds {
        let request = request(*kind, prototypes);
        let path = stand.root.0.join("blank-request.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"schemaVersion":1,"request":request})).unwrap(),
        )
        .unwrap();
        let plan = stand.mutate(
            &format!("blank-{}", kind.as_str()),
            &strings(&["create", "--input", path.to_str().unwrap()]),
        );
        for identity in plan["result"]["prepared"]["identities"]
            .as_object()
            .unwrap()
            .values()
        {
            assert!(identities.insert(identity.as_str().unwrap().to_owned()));
        }
    }
    identities.len()
}

fn bind_registers(stand: &Stand) {
    for kind in [
        MetadataKind::InformationRegister,
        MetadataKind::AccumulationRegister,
        MetadataKind::AccountingRegister,
        MetadataKind::CalculationRegister,
    ] {
        stand.mutate(
            &format!("resource-{}", kind.as_str()),
            &strings(&[
                "create",
                "--kind",
                "resource",
                "--name",
                "Amount",
                "--parent",
                &id(kind),
                "--type",
                "Type=xs:decimal",
            ]),
        );
    }
    let registers = [
        MetadataKind::AccountingRegister,
        MetadataKind::AccumulationRegister,
        MetadataKind::CalculationRegister,
    ]
    .map(reference);
    twins::references(stand, &id(MetadataKind::Document), &registers);
}

fn verify_export(
    stand: &Stand,
    export: &std::path::Path,
    kinds: &[MetadataKind],
    prototypes: &Value,
) {
    for kind in kinds {
        let descriptor =
            PathBuf::from(kind.collection_folder().unwrap()).join(format!("{}.xml", name(*kind)));
        let mut files = vec![descriptor.clone()];
        verify::collect_files(&stand.source, &descriptor.with_extension(""), &mut files);
        for file in files {
            verify::native_file(
                &fs::read(stand.source.join(&file)).unwrap(),
                &fs::read(export.join(&file)).unwrap(),
                &file,
            );
        }
    }
    for (file, expected) in [
        (
            "NativeBlankEventSubscription.xml",
            prototypes["Event"].as_str().unwrap(),
        ),
        ("NativeNamedEvent.xml", "BeforeWrite"),
    ] {
        let xml = fs::read_to_string(export.join("EventSubscriptions").join(file)).unwrap();
        let document = Document::parse(&xml).unwrap();
        assert!(
            document
                .descendants()
                .any(|node| { node.has_tag_name((MD, "Event")) && node.text() == Some(expected) })
        );
    }
}

fn request(kind: MetadataKind, prototypes: &Value) -> Value {
    use MetadataKind as K;
    let mut values = json!({});
    let mut references = json!({});
    let mut types = json!({});
    match kind {
        K::CommonCommand => values["Group"] = json!("FormCommandBarImportant"),
        K::Constant => {
            types["Type"] =
                json!([{"namespace":"http://www.w3.org/2001/XMLSchema","name":"boolean"}]);
        }
        K::BusinessProcess => values["Task"] = json!(reference(K::Task)),
        K::FunctionalOption => values["Location"] = json!(reference(K::Constant)),
        K::FunctionalOptionsParameter => references["Use"] = json!([reference(K::Catalog)]),
        K::DocumentJournal => references["RegisteredDocuments"] = json!([reference(K::Document)]),
        K::Sequence => references["Documents"] = json!([reference(K::Document)]),
        K::AccountingRegister => values["ChartOfAccounts"] = json!(reference(K::ChartOfAccounts)),
        K::CalculationRegister => {
            values["ChartOfCalculationTypes"] = json!(reference(K::ChartOfCalculationTypes));
        }
        K::EventSubscription => {
            values["Event"] = prototypes["Event"].clone();
            values["Handler"] = prototypes["Handler"].clone();
            types["Source"] = prototypes["Source"].clone();
        }
        K::ScheduledJob => values["MethodName"] = prototypes["MethodName"].clone(),
        _ => {}
    }
    json!({"operation":"create","kind":kind,"name":name(kind),"propertyValues":values,"propertyReferences":references,"propertyTypes":types})
}

fn prototypes(stand: &Stand) -> Value {
    let mut result = json!({});
    for (folder, properties) in [
        ("EventSubscriptions", ["Event", "Handler"].as_slice()),
        ("ScheduledJobs", ["MethodName"].as_slice()),
    ] {
        let mut paths: Vec<_> = fs::read_dir(stand.source.join(folder))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "xml"))
            .collect();
        paths.sort();
        let xml = fs::read_to_string(paths.first().expect("native prototype exists")).unwrap();
        let document = Document::parse(&xml).unwrap();
        for property in properties {
            let value = document
                .descendants()
                .find(|node| node.has_tag_name((MD, *property)))
                .and_then(|node| node.text())
                .expect("native prototype property exists");
            result[*property] = json!(value);
        }
        if folder == "EventSubscriptions" {
            let source = document
                .descendants()
                .find(|node| node.has_tag_name((MD, "Source")))
                .unwrap();
            let keys: Vec<_> = source
                .children()
                .filter(Node::is_element)
                .filter(|node| {
                    node.tag_name().namespace() == Some(CORE)
                        && matches!(node.tag_name().name(), "Type" | "TypeSet")
                })
                .map(|node| {
                    let (prefix, name) = node.text().unwrap().split_once(':').unwrap();
                    let namespace = node.lookup_namespace_uri(Some(prefix)).unwrap();
                    assert_eq!(namespace, CFG);
                    json!({"namespace":namespace,"name":name})
                })
                .collect();
            assert_ne!(keys.as_slice(), &[] as &[Value]);
            result["Source"] = json!(keys);
        }
    }
    result
}

fn name(kind: MetadataKind) -> String {
    format!("NativeBlank{}", kind.designer_tag())
}

fn id(kind: MetadataKind) -> String {
    format!("{}:{}", kind.as_str(), name(kind))
}

fn reference(kind: MetadataKind) -> String {
    format!("{}.{}", kind.designer_tag(), name(kind))
}
