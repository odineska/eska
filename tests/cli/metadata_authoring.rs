//! Agent workflows use only public CLI inputs, discovery and exact saved plans.

use crate::support::TestDir;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

fn fixture() -> TestDir {
    let root = TestDir::new();
    fs::create_dir_all(root.0.join("src/Ext")).unwrap();
    fs::write(
        root.0.join("eska.toml"),
        "[project]\ntype='configuration'\n",
    )
    .unwrap();
    fs::write(root.0.join("src/Configuration.xml"), "<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses' version='2.20'><Configuration uuid='11111111-1111-4111-8111-111111111111'><Properties><Name>Demo</Name></Properties><ChildObjects/></Configuration></MetaDataObject>").unwrap();
    fs::write(
        root.0.join("src/Ext/ParentConfigurations.bin"),
        "{6,0,0,0,0,0}",
    )
    .unwrap();
    root
}

fn call(
    root: &TestDir,
    locale: &str,
    args: &[&str],
    input: Option<&Value>,
    success: bool,
) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_eska"))
        .current_dir(&root.0)
        .args(["--lang", locale, "metadata"])
        .args(args)
        .args(["--format", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(input).unwrap())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(i32::from(!success)),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["schema_version"], 1);
    result
}

fn field_fixture() -> TestDir {
    let root = fixture();
    call(
        &root,
        "ru-RU",
        &[
            "create",
            "--kind",
            "document",
            "--name",
            "Order",
            "--no-input",
        ],
        None,
        true,
    );
    for (kind, name) in [("attribute", "Value"), ("tabular-section", "Lines")] {
        call(
            &root,
            "ru-RU",
            &[
                "create",
                "--kind",
                kind,
                "--parent",
                "document:Order",
                "--name",
                name,
                "--no-input",
            ],
            None,
            true,
        );
    }
    root
}

#[test]
fn structural_names_reject_standard_fields_and_cross_kind_collisions_without_writes() {
    let root = field_fixture();
    let path = root.0.join("src/Documents/Order.xml");
    let before = fs::read(&path).unwrap();
    let configuration = fs::read(root.0.join("src/Configuration.xml")).unwrap();
    for locale in ["ru-RU", "en-US"] {
        for name in ["Ref", "Ссылка", "Номер"] {
            for kind in ["attribute", "tabular-section"] {
                let error = call(
                    &root,
                    locale,
                    &[
                        "create",
                        "--kind",
                        kind,
                        "--parent",
                        "document:Order",
                        "--name",
                        name,
                        "--no-input",
                    ],
                    None,
                    false,
                );
                assert_eq!(error["error"]["kind"], "creation_invalid_name");
                assert_eq!(error["error"]["details"]["reason"], "reserved_property");
            }
            for source in [
                "document:Order/attribute:Value",
                "document:Order/tabular-section:Lines",
            ] {
                let error = call(
                    &root,
                    locale,
                    &["copy", "--source", source, "--name", name, "--no-input"],
                    None,
                    false,
                );
                assert_eq!(error["error"]["kind"], "creation_invalid_name");
                let error = call(
                    &root,
                    locale,
                    &["rename-preview", "-"],
                    Some(&json!({"schemaVersion":1,"objectId":source,"newName":name})),
                    false,
                );
                assert_eq!(error["error"]["kind"], "rename_invalid_name");
            }
        }
        for (kind, name) in [("attribute", "lines"), ("tabular-section", "VALUE")] {
            let error = call(
                &root,
                locale,
                &[
                    "create",
                    "--kind",
                    kind,
                    "--parent",
                    "document:Order",
                    "--name",
                    name,
                    "--no-input",
                ],
                None,
                false,
            );
            assert_eq!(error["error"]["kind"], "creation_collision");
        }
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            fs::read(root.0.join("src/Configuration.xml")).unwrap(),
            configuration
        );
    }
}

#[test]
fn authoring_discovery_create_copy_and_delete_are_composable_in_both_locales() {
    for locale in ["ru-RU", "en-US"] {
        let root = fixture();
        let schema = call(&root, locale, &["object-schema"], None, true);
        assert_eq!(
            schema,
            call(
                &root,
                if locale == "ru-RU" { "en-US" } else { "ru-RU" },
                &["object-schema"],
                None,
                true
            )
        );
        assert!(
            schema["result"]["kindOptions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind["key"] == "catalog" && kind["value"] == "Catalog")
        );
        assert_eq!(
            schema["result"]["requestSchemas"]["copy"]["properties"]["request"]["properties"]["operation"]
                ["const"],
            "copy"
        );
        let plan = call(
            &root,
            locale,
            &[
                "create",
                "--kind",
                "common-module",
                "--name",
                "Helpers",
                "--set",
                "Server=true",
                "--dry-run",
                "--no-input",
            ],
            None,
            true,
        );
        assert!(!root.0.join("src/CommonModules/Helpers.xml").exists());
        let replay = call(
            &root,
            locale,
            &["create", "--plan", "-", "--dry-run"],
            Some(&plan),
            true,
        );
        assert_eq!(plan, replay);
        let created = call(
            &root,
            locale,
            &["create", "--plan", "-", "--no-input"],
            Some(&plan),
            true,
        );
        assert_eq!(created["result"]["objectId"], "common-module:Helpers");
        let payload = root.0.join("src/CommonModules/Helpers/Ext/Module.bsl");
        fs::create_dir_all(payload.parent().unwrap()).unwrap();
        fs::write(
            &payload,
            "\u{feff}Процедура Пример() Экспорт\r\nКонецПроцедуры\r\n",
        )
        .unwrap();
        assert_copy_and_delete(&root, locale, &payload);
    }
}

fn assert_copy_and_delete(root: &TestDir, locale: &str, payload: &std::path::Path) {
    let copied = call(
        root,
        locale,
        &[
            "copy",
            "--source",
            "common-module:Helpers",
            "--name",
            "HelpersCopy",
            "--no-input",
        ],
        None,
        true,
    );
    assert_eq!(copied["result"]["objectId"], "common-module:HelpersCopy");
    assert_eq!(
        fs::read(payload).unwrap(),
        fs::read(root.0.join("src/CommonModules/HelpersCopy/Ext/Module.bsl")).unwrap()
    );
    let page = call(
        root,
        locale,
        &["list", "--kind", "common-module", "--limit", "1"],
        None,
        true,
    );
    assert_eq!(page["result"]["objects"].as_array().unwrap().len(), 1);
    assert!(page["result"]["nextAfter"].is_string());
    let second = call(
        root,
        locale,
        &[
            "list",
            "--kind",
            "common-module",
            "--after",
            page["result"]["nextAfter"].as_str().unwrap(),
        ],
        None,
        true,
    );
    assert_eq!(second["result"]["objects"].as_array().unwrap().len(), 1);
    let denied = call(
        root,
        locale,
        &[
            "delete",
            "--object",
            "common-module:HelpersCopy",
            "--no-input",
        ],
        None,
        false,
    );
    assert_eq!(denied["error"]["kind"], "object_confirmation_required");
    call(
        root,
        locale,
        &[
            "delete",
            "--object",
            "common-module:HelpersCopy",
            "--yes",
            "--no-input",
        ],
        None,
        true,
    );
    assert!(!root.0.join("src/CommonModules/HelpersCopy.xml").exists());
    assert!(payload.exists());
}

#[test]
fn authoring_saved_plans_reject_changed_inputs_forged_identity_and_wrong_operation() {
    let root = fixture();
    let plan = call(
        &root,
        "en-US",
        &[
            "create",
            "--kind",
            "catalog",
            "--name",
            "Example",
            "--dry-run",
        ],
        None,
        true,
    );
    let wrong = call(&root, "en-US", &["copy", "--plan", "-"], Some(&plan), false);
    assert_eq!(wrong["error"]["kind"], "object_input_invalid");
    let mut forged = plan.clone();
    forged["result"]["prepared"]["identities"]["object"] = json!("../escape");
    call(
        &root,
        "en-US",
        &["create", "--plan", "-"],
        Some(&forged),
        false,
    );
    assert!(!root.0.join("src/Catalogs/Example.xml").exists());
    let path = root.0.join("src/Configuration.xml");
    let changed = format!(
        "{}\n<!-- external change -->",
        fs::read_to_string(&path).unwrap()
    );
    fs::write(&path, &changed).unwrap();
    let result = call(
        &root,
        "en-US",
        &["create", "--plan", "-"],
        Some(&plan),
        false,
    );
    assert_eq!(result["error"]["kind"], "property_conflict");
    assert_eq!(fs::read_to_string(path).unwrap(), changed);
    assert!(!root.0.join("src/Catalogs/Example.xml").exists());
}

#[test]
fn authoring_type_membership_is_discoverable_and_accepts_semantic_inputs() {
    for locale in ["ru-RU", "en-US"] {
        let root = fixture();
        let schema = call(&root, locale, &["object-schema"], None, true);
        assert!(
            schema["result"]["typeProperties"]["defined-type"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "Type")
        );
        let plan = call(
            &root,
            locale,
            &[
                "create",
                "--kind",
                "defined-type",
                "--name",
                "Identifier",
                "--type",
                "Type=xs:string",
                "--type",
                "Type=xs:decimal",
                "--dry-run",
            ],
            None,
            true,
        );
        call(&root, locale, &["create", "--plan", "-"], Some(&plan), true);
        let inspected = call(
            &root,
            locale,
            &["inspect", "--object", "defined-type:Identifier"],
            None,
            true,
        );
        let field = inspected["result"]["editing"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["schema"]["kind"] == "types")
            .unwrap();
        assert_eq!(field["schema"]["keys"].as_array().unwrap().len(), 2);
        let invalid = call(
            &root,
            locale,
            &[
                "create",
                "--kind",
                "defined-type",
                "--name",
                "Invalid",
                "--type",
                "Type=cfg:DocumentObject.Missing",
            ],
            None,
            false,
        );
        assert!(invalid["error"].is_object());
        assert!(!root.0.join("src/DefinedTypes/Invalid.xml").exists());
        let request = json!({"schemaVersion":1,"request":{"operation":"create","kind":"DefinedType","name":"Typo","propertyTypes":{"Type":[{"namespace":"http://www.w3.org/2001/XMLSchema","name":"string","unexpected":true}]}}});
        assert_eq!(
            call(
                &root,
                locale,
                &["create", "--input", "-"],
                Some(&request),
                false
            )["error"]["kind"],
            "object_input_invalid"
        );
        assert!(!root.0.join("src/DefinedTypes/Typo.xml").exists());
    }
}

#[test]
fn authoring_initial_selectors_use_existing_metadata_and_localized_catalogs() {
    let root = fixture();
    for (kind, name) in [
        ("document", "Order"),
        ("task", "Review"),
        ("constant", "FeatureFlag"),
        ("catalog", "Items"),
    ] {
        call(
            &root,
            "en-US",
            &["create", "--kind", kind, "--name", name],
            None,
            true,
        );
    }
    for (kind, property, value) in [
        ("business-process", "Task", "Task.Review"),
        ("functional-option", "Location", "Constant.FeatureFlag"),
        ("functional-option-parameter", "Use", "Catalog.Items"),
        ("document-journal", "RegisteredDocuments", "Document.Order"),
        ("common-command", "Group", "NavigationPanelOrdinary"),
    ] {
        let schema = call(
            &root,
            "en-US",
            &["object-schema", "--kind", kind],
            None,
            true,
        );
        assert_eq!(
            schema,
            call(
                &root,
                "ru-RU",
                &["object-schema", "--kind", kind],
                None,
                true
            )
        );
        let field = schema["result"]["initialFields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["property"] == property)
            .unwrap();
        assert!(
            field["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["value"] == value),
            "{field}"
        );
    }
    let schema = call(
        &root,
        "en-US",
        &["object-schema", "--kind", "event-subscription"],
        None,
        true,
    );
    let source = schema["result"]["initialFields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["property"] == "Source")
        .unwrap();
    assert_eq!(source["kind"], "types");
    assert_eq!(source["multiple"], true);
    assert!(
        source["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|choice| choice["key"]["name"] == "DocumentObject.Order")
    );
    assert!(
        source["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|choice| choice["key"]["name"] == "DocumentObject")
    );
    assert_eq!(
        schema["result"]["uniqueProperties"]["http-service"],
        json!(["RootURL"])
    );
    assert!(schema["result"]["propertyCaptions"]["RootURL"]["ru-RU"].is_string());
}

#[test]
fn authoring_replays_large_display_plans_without_interpreting_display_paths_as_writes() {
    let root = fixture();
    let mut plan = call(
        &root,
        "en-US",
        &[
            "create",
            "--kind",
            "catalog",
            "--name",
            "LargePreview",
            "--dry-run",
        ],
        None,
        true,
    );
    // File summaries can exceed a request's 1 MiB budget while the semantic prepared input is small.
    plan["result"]["files"] = json!((0..7000).map(|index| json!({
        "path":{"encoding":"utf-8","value":format!("NotAnInstruction/{index}/{}","x".repeat(100))},
        "action":"create","after":{"bytes":0,"snapshot":"0".repeat(64)},"before":null,
    })).collect::<Vec<_>>());
    assert!(serde_json::to_vec(&plan).unwrap().len() > 1_048_576);
    let replay = call(
        &root,
        "en-US",
        &["create", "--plan", "-", "--dry-run"],
        Some(&plan),
        true,
    );
    assert_eq!(replay["result"]["prepared"], plan["result"]["prepared"]);
    call(
        &root,
        "en-US",
        &["create", "--plan", "-"],
        Some(&plan),
        true,
    );
    assert!(root.0.join("src/Catalogs/LargePreview.xml").is_file());
    assert!(!root.0.join("src/NotAnInstruction").exists());
}

#[test]
fn authoring_form_operations_use_discovered_snapshot_and_return_allocated_ids() {
    let root = fixture();
    call(
        &root,
        "ru-RU",
        &[
            "create",
            "--kind",
            "common-form",
            "--name",
            "Editor",
            "--no-input",
        ],
        None,
        true,
    );
    let schema = call(
        &root,
        "en-US",
        &["form-schema", "--form", "common-form:Editor"],
        None,
        true,
    );
    let request = json!({"schemaVersion":1,"request":{
        "form":"common-form:Editor","snapshot":schema["result"]["snapshot"],
        "operations":[{"kind":"addElement","parent":{"kind":"root"},"element":{"kind":"UsualGroup","name":"Totals","children":[{"kind":"InputField","name":"Amount"}]}}],
    }});
    let original = fs::read(root.0.join("src/CommonForms/Editor/Ext/Form.xml")).unwrap();
    let plan = call(
        &root,
        "ru-RU",
        &["form-edit", "--input", "-", "--dry-run", "--no-input"],
        Some(&request),
        true,
    );
    assert_eq!(
        fs::read(root.0.join("src/CommonForms/Editor/Ext/Form.xml")).unwrap(),
        original
    );
    assert_ne!(
        plan["result"]["assignedIds"].as_array().unwrap().as_slice(),
        &[] as &[Value]
    );
    let applied = call(
        &root,
        "en-US",
        &["form-edit", "--plan", "-", "--no-input"],
        Some(&plan),
        true,
    );
    assert_eq!(
        applied["result"]["assignedIds"],
        plan["result"]["assignedIds"]
    );
    let fresh = call(
        &root,
        "en-US",
        &["form-schema", "--form", "common-form:Editor"],
        None,
        true,
    );
    assert!(
        fresh["result"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "Amount")
    );
    let repeated = call(
        &root,
        "en-US",
        &["form-edit", "--plan", "-"],
        Some(&plan),
        false,
    );
    assert_eq!(repeated["error"]["kind"], "property_conflict");
    let mut forged = request;
    forged["request"]["snapshot"] = fresh["result"]["snapshot"].clone();
    forged["request"]["operations"][0]["parent"]["id"] = json!(123);
    assert_eq!(
        call(
            &root,
            "en-US",
            &["form-edit", "--input", "-"],
            Some(&forged),
            false
        )["error"]["kind"],
        "object_input_invalid"
    );
}

#[test]
fn authoring_plans_conditional_appearance_independently_of_locale() {
    let root = fixture();
    call(
        &root,
        "ru-RU",
        &["create", "--kind", "common-form", "--name", "Editor"],
        None,
        true,
    );
    let schema = |locale| {
        call(
            &root,
            locale,
            &["form-schema", "--form", "common-form:Editor"],
            None,
            true,
        )
    };
    let original = schema("ru-RU");
    assert_eq!(original, schema("en-US"));
    assert_eq!(
        original["result"]["conditionalAppearance"]["rules"],
        json!([])
    );
    let form = root.0.join("src/CommonForms/Editor/Ext/Form.xml");
    let content = fs::read(&form).unwrap();
    let appearance = json!({"schemaVersion":1,"request":{
        "form":"common-form:Editor","snapshot":original["result"]["snapshot"],
        "operations":[{"kind":"setConditionalAppearance","rules":[{
            "fields":[],"conditions":[],"format":[
                {"parameter":"background","enabled":true,"value":{"kind":"color","value":"#C0FFEE"}}
            ]
        }]}]
    }});
    let preview = |locale| {
        call(
            &root,
            locale,
            &["form-edit", "--input", "-", "--dry-run"],
            Some(&appearance),
            true,
        )
    };
    let reviewed = preview("ru-RU");
    assert_eq!(preview("en-US"), reviewed);
    assert_eq!(fs::read(&form).unwrap(), content);
    call(
        &root,
        "ru-RU",
        &["form-edit", "--plan", "-", "--no-input"],
        Some(&reviewed),
        true,
    );
    assert!(fs::read_to_string(form).unwrap().contains("#C0FFEE"));
    let saved = schema("en-US");
    assert_eq!(saved, schema("ru-RU"));
    assert_eq!(
        saved["result"]["conditionalAppearance"]["rules"][0]["format"][0]["value"]["value"],
        "#C0FFEE"
    );
}

#[test]
fn authoring_can_start_from_an_empty_scaffold_without_handwritten_designer_xml() {
    for locale in ["ru-RU", "en-US"] {
        let parent = TestDir::new();
        let output = Command::new(env!("CARGO_BIN_EXE_eska"))
            .current_dir(&parent.0)
            .args([
                "--lang",
                locale,
                "new",
                "Fresh",
                "--type",
                "configuration",
                "--workflow",
                "trunk",
                "--platform-version",
                "",
                "--no-vcs",
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let root = TestDir(parent.0.join("Fresh"));
        let schema = call(&root, locale, &["object-schema"], None, true);
        assert_eq!(schema["result"]["kinds"], json!(["Configuration"]));
        let plan = call(
            &root,
            locale,
            &[
                "create",
                "--kind",
                "configuration",
                "--name",
                "Orders",
                "--dry-run",
                "--no-input",
            ],
            None,
            true,
        );
        assert_eq!(
            plan["result"]["prepared"]["identities"]
                .as_object()
                .unwrap()
                .len(),
            9
        );
        assert!(!root.0.join("src/Configuration.xml").exists());
        call(
            &root,
            locale,
            &["create", "--plan", "-", "--no-input"],
            Some(&plan),
            true,
        );
        let configuration = call(&root, locale, &["inspect"], None, true);
        assert_eq!(
            configuration["result"]["object"]["objectId"],
            "configuration:Orders"
        );
        call(
            &root,
            locale,
            &[
                "create",
                "--kind",
                "document",
                "--name",
                "Order",
                "--no-input",
            ],
            None,
            true,
        );
        let path = root.0.join("src/Configuration.xml");
        let before = fs::read(&path).unwrap();
        let repeated = call(
            &root,
            locale,
            &["create", "--plan", "-"],
            Some(&plan),
            false,
        );
        assert_eq!(repeated["error"]["kind"], "object_invalid");
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
fn module_authoring_discovers_roles_and_creates_each_only_once_in_both_locales() {
    for locale in ["ru-RU", "en-US"] {
        let root = field_fixture();
        let schema = call(
            &root,
            locale,
            &["object-schema", "--parent", "document:Order"],
            None,
            true,
        );
        let data = &schema["result"];
        assert_eq!(data["moduleRoles"], json!(["object", "manager"]));
        assert!(data["requestSchemas"]["create-module"].is_object());
        let before = fs::read(root.0.join("src/Documents/Order.xml")).unwrap();
        for (role, file) in [
            ("object", "ObjectModule.bsl"),
            ("manager", "ManagerModule.bsl"),
        ] {
            let plan = call(
                &root,
                locale,
                &[
                    "create-module",
                    "--owner",
                    "document:Order",
                    "--role",
                    role,
                    "--dry-run",
                ],
                None,
                true,
            );
            assert_eq!(plan["result"]["operation"], "create-module");
            assert_eq!(plan["result"]["prepared"]["identities"], json!({}));
            assert!(!root.0.join("src/Documents/Order/Ext").join(file).exists());
            let result = call(
                &root,
                locale,
                &["create-module", "--input", "-"],
                Some(&json!({"schemaVersion":1,"request":plan["result"]["prepared"]["request"]})),
                true,
            );
            assert_eq!(result["result"]["objectId"], "document:Order");
            assert_eq!(
                fs::read(root.0.join("src/Documents/Order/Ext").join(file)).unwrap(),
                b""
            );
            let duplicate = call(
                &root,
                locale,
                &["create-module", "--owner", "document:Order", "--role", role],
                None,
                false,
            );
            assert_eq!(duplicate["error"]["kind"], "object_invalid");
        }
        for role in ["module", "record-set", "arbitrary"] {
            let error = call(
                &root,
                locale,
                &["create-module", "--owner", "document:Order", "--role", role],
                None,
                false,
            );
            assert_eq!(
                error["error"]["kind"],
                if role == "arbitrary" {
                    "object_input_invalid"
                } else {
                    "object_invalid"
                }
            );
        }
        assert_eq!(
            fs::read(root.0.join("src/Documents/Order.xml")).unwrap(),
            before
        );
        let final_schema = call(
            &root,
            locale,
            &["object-schema", "--parent", "document:Order"],
            None,
            true,
        );
        assert_eq!(final_schema["result"]["moduleRoles"], json!([]));
    }
}
