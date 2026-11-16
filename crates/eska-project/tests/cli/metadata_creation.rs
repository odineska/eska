//! Creation works through the standalone CLI without an IDE, platform or executable on PATH.

use std::{
    fs,
    io::Write,
    process::{Output, Stdio},
};

use serde_json::{Value, json};

use super::{TestDir, command};

fn fixture() -> TestDir {
    let directory = TestDir::new();
    fs::write(
        directory.0.join("eska.toml"),
        "[project]\ntype='configuration'\n",
    )
    .unwrap();
    let source = directory.0.join("src");
    for name in ["Ext", "Documents", "Catalogs", "Languages"] {
        fs::create_dir_all(source.join(name)).unwrap();
    }
    fs::write(source.join("Ext/ParentConfigurations.bin"), "{6,0,0,0,0,0}").unwrap();
    fs::write(source.join("Configuration.xml"), descriptor("Configuration", "Demo", 1,
        "<DefaultLanguage>Language.Русский</DefaultLanguage>",
        "<Document>Sale</Document><Catalog>Goods</Catalog><Language>Русский</Language><Language>English</Language>")).unwrap();
    fs::write(
        source.join("Documents/Sale.xml"),
        descriptor(
            "Document",
            "Sale",
            2,
            "<Comment>keep &amp; preserve</Comment><Opaque xmlns='urn:vendor'>untouched</Opaque>",
            "",
        ),
    )
    .unwrap();
    fs::write(
        source.join("Catalogs/Goods.xml"),
        descriptor("Catalog", "Goods", 3, "", ""),
    )
    .unwrap();
    fs::write(
        source.join("Languages/Русский.xml"),
        descriptor(
            "Language",
            "Русский",
            4,
            "<LanguageCode>ru</LanguageCode>",
            "",
        ),
    )
    .unwrap();
    fs::write(
        source.join("Languages/English.xml"),
        descriptor(
            "Language",
            "English",
            5,
            "<LanguageCode>en</LanguageCode>",
            "",
        ),
    )
    .unwrap();
    directory
}

fn descriptor(tag: &str, name: &str, id: u8, properties: &str, children: &str) -> String {
    format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\" version=\"2.20\">\r\n  <{tag} uuid=\"11111111-1111-4111-8111-{id:012}\"><Properties><Name>{name}</Name>{properties}</Properties><ChildObjects>{children}</ChildObjects></{tag}>\r\n</MetaDataObject>\r\n"
    )
}

fn output(root: &TestDir, locale: &str, args: &[&str], input: Option<&Value>) -> Output {
    let mut child = command(&root.0, locale)
        .args(["metadata"])
        .args(args)
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
    } else {
        drop(child.stdin.take());
    }
    child.wait_with_output().unwrap()
}

fn json_output(
    root: &TestDir,
    locale: &str,
    args: &[&str],
    input: Option<&Value>,
    success: bool,
) -> Value {
    let output = output(root, locale, args, input);
    assert_eq!(
        output.status.success(),
        success,
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    value
}

fn preview(root: &TestDir, locale: &str, name: &str) -> Value {
    json_output(
        root,
        locale,
        &[
            "add",
            "attribute",
            "--owner",
            "document:Sale",
            "--name",
            name,
            "--type",
            "string",
            "--length",
            "200",
            "--synonym",
            "ru=Комментарий менеджера",
            "--comment",
            "A&B <keep>",
            "--dry-run",
            "--no-input",
            "--format",
            "json",
        ],
        None,
        true,
    )
}

#[test]
fn creation_discovery_is_read_only_and_independent_of_locale() {
    let root = fixture();
    let path = root.0.join("src/Documents/Sale.xml");
    let original = fs::read(&path).unwrap();
    for arguments in [
        vec!["create-schema"],
        vec!["create-schema", "--owner", "document:Sale"],
    ] {
        let ru = json_output(&root, "ru", &arguments, None, true);
        let en = json_output(&root, "en", &arguments, None, true);
        assert_eq!(ru, en);
        assert!(en["result"]["capabilities"]["primitiveTypes"].is_array());
    }
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(!root.0.join(".eska/metadata/write.lock").exists());
}

#[test]
fn saved_creation_plan_preserves_reviewed_ids_and_exact_bytes() {
    for locale in ["ru", "en"] {
        let root = fixture();
        let path = root.0.join("src/Documents/Sale.xml");
        let original = fs::read_to_string(&path).unwrap();
        let plan = preview(&root, locale, "ManagerComment");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(plan["result"]["prepared"]["identity"]["uuid"].is_string());
        assert!(plan["result"]["prepared"]["expectedOutputSnapshot"].is_string());
        let checked = json_output(
            &root,
            locale,
            &[
                "add",
                "--plan",
                "-",
                "--dry-run",
                "--no-input",
                "--format",
                "json",
            ],
            Some(&plan),
            true,
        );
        assert_eq!(checked, plan);
        let mut expected = original;
        for change in plan["result"]["changes"].as_array().unwrap().iter().rev() {
            let start = usize::try_from(change["range"]["start"].as_u64().unwrap()).unwrap();
            let end = usize::try_from(change["range"]["end"].as_u64().unwrap()).unwrap();
            expected.replace_range(start..end, change["replacement"].as_str().unwrap());
        }
        let applied = json_output(
            &root,
            locale,
            &["add", "--plan", "-", "--no-input", "--format", "json"],
            Some(&plan),
            true,
        );
        assert_eq!(
            applied["result"]["objectId"],
            "document:Sale/attribute:ManagerComment"
        );
        assert_eq!(
            applied["result"]["uuid"],
            plan["result"]["prepared"]["identity"]["uuid"]
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        let inspected = json_output(
            &root,
            locale,
            &[
                "inspect",
                "--object",
                "document:Sale/attribute:ManagerComment",
            ],
            None,
            true,
        );
        assert_eq!(
            inspected["result"]["object"]["objectId"],
            applied["result"]["objectId"]
        );
        let duplicate = json_output(
            &root,
            locale,
            &[
                "add",
                "attribute",
                "--owner",
                "document:Sale",
                "--name",
                "ManagerComment",
                "--type",
                "boolean",
                "--no-input",
                "--format",
                "json",
            ],
            None,
            false,
        );
        assert_eq!(duplicate["error"]["kind"], "creation_collision");
        assert_eq!(fs::read_to_string(path).unwrap(), expected);
    }
}

#[test]
fn stale_and_tampered_creation_plans_never_write() {
    let root = fixture();
    let path = root.0.join("src/Documents/Sale.xml");
    let original = fs::read_to_string(&path).unwrap();
    let plan = preview(&root, "en", "ManagerComment");
    let mut tampered = plan.clone();
    tampered["result"]["prepared"]["request"]["name"] = json!("ChangedAfterReview");
    let rejected = json_output(
        &root,
        "en",
        &["add", "--plan", "-", "--no-input", "--format", "json"],
        Some(&tampered),
        false,
    );
    assert!(rejected["error"]["kind"].is_string());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let changed = original.replace("keep &amp; preserve", "external edit");
    fs::write(&path, &changed).unwrap();
    let rejected = json_output(
        &root,
        "en",
        &["add", "--plan", "-", "--no-input", "--format", "json"],
        Some(&plan),
        false,
    );
    assert_eq!(rejected["error"]["kind"], "property_conflict");
    assert_eq!(fs::read_to_string(path).unwrap(), changed);
}

#[test]
fn table_part_attributes_and_composite_input_share_the_core() {
    let root = fixture();
    let table = json_output(
        &root,
        "ru",
        &[
            "add",
            "table-part",
            "--owner",
            "document:Sale",
            "--name",
            "Items",
            "--no-input",
            "--format",
            "json",
        ],
        None,
        true,
    );
    assert_eq!(
        table["result"]["objectId"],
        "document:Sale/tabular-section:Items"
    );
    let schema = json_output(
        &root,
        "en",
        &[
            "create-schema",
            "--owner",
            "document:Sale/tabular-section:Items",
        ],
        None,
        true,
    );
    let request = json!({"schemaVersion":1,"request":{
        "owner":"document:Sale/tabular-section:Items","snapshot":schema["result"]["snapshot"],
        "kind":"attribute","name":"Value","types":[{"kind":"string","length":50},{"kind":"reference","object":"catalog:Goods"}],
        "synonyms":[{"language":"ru","content":"Значение"},{"language":"en","content":"Value"}]
    }});
    let result = json_output(
        &root,
        "ru",
        &["add", "--input", "-", "--no-input", "--format", "json"],
        Some(&request),
        true,
    );
    assert_eq!(
        result["result"]["objectId"],
        "document:Sale/tabular-section:Items/attribute:Value"
    );
    let text = fs::read_to_string(root.0.join("src/Documents/Sale.xml")).unwrap();
    assert!(text.contains("CatalogRef.Goods"));
    assert!(text.contains("<v8:lang>en</v8:lang>"));
    let zero = json_output(
        &root,
        "en",
        &[
            "add",
            "attribute",
            "--owner",
            "document:Sale",
            "--name",
            "ZeroPrecision",
            "--type",
            "number",
            "--digits",
            "0",
            "--fraction-digits",
            "0",
            "--no-input",
            "--format",
            "json",
        ],
        None,
        true,
    );
    assert_eq!(
        zero["result"]["objectId"],
        "document:Sale/attribute:ZeroPrecision"
    );
}

#[test]
fn noninteractive_missing_and_unknown_fields_fail_without_prompting() {
    let root = fixture();
    let path = root.0.join("src/Documents/Sale.xml");
    let original = fs::read(&path).unwrap();
    let missing = json_output(
        &root,
        "en",
        &["add", "attribute", "--no-input", "--format", "json"],
        None,
        false,
    );
    assert_eq!(missing["error"]["kind"], "creation_arguments_required");
    assert_eq!(
        missing["error"]["details"]["fields"],
        json!(["owner", "name", "type"])
    );
    let schema = json_output(
        &root,
        "en",
        &["create-schema", "--owner", "document:Sale"],
        None,
        true,
    );
    let mut request = json!({"schemaVersion":1,"request":{
        "owner":"document:Sale","snapshot":schema["result"]["snapshot"],"kind":"attribute","name":"New","types":[{"kind":"boolean"}]
    }});
    request["request"]["xml"] = json!("<arbitrary/>");
    let failed = json_output(
        &root,
        "en",
        &["add", "--input", "-", "--no-input", "--format", "json"],
        Some(&request),
        false,
    );
    assert_eq!(failed["error"]["kind"], "creation_input_invalid");
    request["request"].as_object_mut().unwrap().remove("xml");
    request["request"]["types"][0]["ignored"] = json!(true);
    let failed = json_output(
        &root,
        "en",
        &["add", "--input", "-", "--no-input", "--format", "json"],
        Some(&request),
        false,
    );
    assert_eq!(failed["error"]["kind"], "creation_input_invalid");
    request["request"]["types"][0]
        .as_object_mut()
        .unwrap()
        .remove("ignored");
    request["request"]["synonyms"] = json!([{"language":"ru","content":"New","xml":"ignored?"}]);
    let failed = json_output(
        &root,
        "ru",
        &["add", "--input", "-", "--no-input", "--format", "json"],
        Some(&request),
        false,
    );
    assert_eq!(failed["error"]["kind"], "creation_input_invalid");
    let invalid = json_output(
        &root,
        "en",
        &[
            "add",
            "attribute",
            "--owner",
            "document:Sale",
            "--name",
            "New",
            "--type",
            "boolean",
            "--length",
            "200",
            "--no-input",
            "--format",
            "json",
        ],
        None,
        false,
    );
    assert_eq!(invalid["error"]["kind"], "creation_input_invalid");
    assert_eq!(fs::read(path).unwrap(), original);
    let terminal = json_output(
        &root,
        "en",
        &["add", "--interactive", "--format", "json"],
        None,
        false,
    );
    assert_eq!(terminal["error"]["kind"], "creation_terminal_required");
    for conflict in [
        vec!["add", "--interactive", "--no-input"],
        vec!["add", "--plan", "-", "--name", "Ignored"],
        vec!["add", "--input", "-", "--type", "boolean"],
    ] {
        let rejected = output(&root, "en", &conflict, None);
        assert_eq!(rejected.status.code(), Some(2));
        assert_eq!(rejected.stdout, [] as [u8; 0]);
    }
}

#[test]
fn human_preview_and_help_are_localized_without_changing_legacy_json() {
    let root = fixture();
    for (locale, label) in [("ru", "Строка"), ("en", "String")] {
        let result = output(
            &root,
            locale,
            &[
                "add",
                "attribute",
                "--owner",
                "document:Sale",
                "--name",
                "New",
                "--type",
                "string",
                "--length",
                "200",
                "--dry-run",
                "--no-input",
            ],
            None,
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains(label), "{text}");
        assert!(text.contains("200"));
        assert!(text.replace('\\', "/").contains("Documents/Sale.xml"));
        let help = output(&root, locale, &["add", "--help"], None);
        assert!(
            help.status.success(),
            "{}",
            String::from_utf8_lossy(&help.stderr)
        );
        let help = String::from_utf8(help.stdout).unwrap();
        for argument in ["--no-input", "--plan", "--dry-run", "--interactive"] {
            assert!(help.contains(argument), "{help}");
        }
        let legacy = output(&root, locale, &["inspect", "--format", "human"], None);
        assert_eq!(legacy.status.code(), Some(2));
        assert_eq!(legacy.stdout, [] as [u8; 0]);
        json_output(&root, locale, &["inspect"], None, true);
    }
}
