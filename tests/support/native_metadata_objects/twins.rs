use std::{collections::BTreeMap, fs, path::PathBuf};

use eska_project_core::project::metadata_model::MetadataKind;
use serde_json::{Value, json};

use super::{
    Stand, strings,
    verify::{self, Object},
};

pub fn run() {
    let input = PathBuf::from(
        std::env::var_os("ESKA_NATIVE_METADATA_SOURCE")
            .expect("select a read-only source directory containing Configuration.xml"),
    );
    let stand = Stand::from_source(&input);
    let objects = verify::select(&stand.source);
    assert!(
        !objects.is_empty(),
        "selected source contains no applicable metadata"
    );
    let mut identities = std::collections::BTreeSet::new();
    let mut report = Vec::new();
    for object in &objects {
        let before: BTreeMap<_, _> = object
            .files
            .iter()
            .map(|path| (path, fs::read(stand.source.join(path)).unwrap()))
            .collect();
        let overrides = overrides(object.kind);
        let plan = copy(&stand, object, &overrides);
        for (path, bytes) in before {
            assert_eq!(
                fs::read(stand.source.join(path)).unwrap(),
                bytes,
                "copy modified original {}",
                path.display()
            );
        }
        verify::twins(&stand.source, object, &plan, &overrides);
        for identity in plan["result"]["prepared"]["identities"]
            .as_object()
            .unwrap()
            .values()
        {
            assert!(
                identities.insert(identity.as_str().unwrap().to_owned()),
                "cross-object UUID collision"
            );
        }
        report.push(json!({"kind":object.kind.as_str(),"files":object.files.len(),"identities":plan["result"]["prepared"]["identities"].as_object().unwrap().len(),"copy":"passed"}));
    }
    let has_registrar = create_registrar(&stand, &objects);
    let has_process = create_task_process(&stand, &objects);
    let export = roundtrip(&stand, "twins", true);
    for object in &objects {
        verify::exported(&stand.source, &export, object);
    }
    if has_process {
        stand.mutate(
            "delete-task-process",
            &strings(&[
                "delete",
                "--object",
                "business-process:NativeTwinProcess",
                "--yes",
            ]),
        );
    }
    if has_registrar {
        stand.mutate(
            "delete-registrar",
            &strings(&[
                "delete",
                "--object",
                "document:NativeTwinRegistrar",
                "--yes",
            ]),
        );
    }
    for object in objects.iter().rev() {
        delete(&stand, object);
    }
    roundtrip(&stand, "deleted", false);
    fs::write(
        stand.root.0.join("logs/matrix.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Native metadata matrix: {} families, {} unique fresh identities",
        objects.len(),
        identities.len()
    );
    eprintln!(
        "Native mode: {}",
        std::env::var("ESKA_NATIVE_METADATA_MODE").unwrap_or_else(|_| "full".to_owned())
    );
}

fn roundtrip(stand: &Stand, label: &str, create: bool) -> PathBuf {
    match std::env::var("ESKA_NATIVE_METADATA_MODE").as_deref() {
        Ok("source") => stand.source_roundtrip(label, create),
        Ok("full") | Err(_) => stand.roundtrip(label, create),
        Ok(value) => panic!("unknown native acceptance mode {value}; use full or source"),
    }
}

fn overrides(kind: MetadataKind) -> BTreeMap<String, String> {
    match kind {
        MetadataKind::HTTPService => {
            BTreeMap::from([("RootURL".to_owned(), "eska-native-twin".to_owned())])
        }
        MetadataKind::WebService => BTreeMap::from([(
            "DescriptorFileName".to_owned(),
            "eska-native-twin.1cws".to_owned(),
        )]),
        _ => BTreeMap::new(),
    }
}

fn copy(stand: &Stand, object: &Object, overrides: &BTreeMap<String, String>) -> Value {
    let mut args = strings(&[
        "copy",
        "--source",
        &object.id(),
        "--name",
        &object.twin_name(),
    ]);
    for (key, value) in overrides {
        args.extend(["--set".to_owned(), format!("{key}={value}")]);
    }
    stand.mutate(&format!("copy-{}", object.kind.as_str()), &args)
}

fn delete(stand: &Stand, object: &Object) {
    let id = format!("{}:{}", object.kind.as_str(), object.twin_name());
    stand.mutate(
        &format!("delete-{}", object.kind.as_str()),
        &strings(&["delete", "--object", &id, "--yes"]),
    );
    assert!(!stand.source.join(object.twin_descriptor()).exists());
    assert!(
        !stand
            .source
            .join(object.twin_descriptor().with_extension(""))
            .exists()
    );
    for path in &object.files {
        assert!(
            stand.source.join(path).exists(),
            "delete lost source {}",
            path.display()
        );
    }
}

fn create_registrar(stand: &Stand, objects: &[Object]) -> bool {
    let registers: Vec<_> = objects
        .iter()
        .filter(|object| {
            matches!(
                object.kind,
                MetadataKind::AccountingRegister
                    | MetadataKind::AccumulationRegister
                    | MetadataKind::CalculationRegister
            )
        })
        .map(|object| format!("{}.{}", object.kind.designer_tag(), object.twin_name()))
        .collect();
    if registers.is_empty() {
        return false;
    }
    stand.mutate(
        "create-registrar",
        &strings(&[
            "create",
            "--kind",
            "document",
            "--name",
            "NativeTwinRegistrar",
        ]),
    );
    references(stand, "document:NativeTwinRegistrar", &registers);
    true
}

fn create_task_process(stand: &Stand, objects: &[Object]) -> bool {
    let Some(task) = objects
        .iter()
        .find(|object| object.kind == MetadataKind::Task)
    else {
        return false;
    };
    // The platform exposes Task.BusinessProcess/RoutePoint only when a process uses it.
    stand.mutate(
        "create-task-process",
        &strings(&[
            "create",
            "--kind",
            "business-process",
            "--name",
            "NativeTwinProcess",
            "--set",
            &format!("Task=Task.{}", task.twin_name()),
        ]),
    );
    true
}

pub(super) fn references(stand: &Stand, object: &str, values: &[String]) {
    stand.property(
        object,
        "RegisterRecords",
        &json!({"kind":"references","values":values}),
    );
}
