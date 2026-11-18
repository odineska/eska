//! Drive the public CLI before asking the real platform to load its generated XML.

#[path = "metadata_creation_external.rs"]
mod external;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use roxmltree::{Document, Node};
use serde_json::{Value, json};

use super::{MD, XR, copy_tree, leaves, native, option, property, support::TestDir};

struct Created {
    uuid: String,
    source: PathBuf,
    table: bool,
}

fn source_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(root).expect("read owned source directory") {
        let entry = entry.expect("source entry");
        if entry.file_type().expect("source type").is_dir() {
            files.extend(source_files(&entry.path()));
        } else {
            files.insert(
                entry.path(),
                fs::read(entry.path()).expect("read source bytes"),
            );
        }
    }
    files
}

fn executable(environment: &str, component: &str) -> PathBuf {
    std::env::var_os(environment).map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "target/debug/{component}{}",
                std::env::consts::EXE_SUFFIX
            ))
        },
        PathBuf::from,
    )
}

fn cli(root: &Path, language: &str, arguments: &[&str], success: bool) -> Value {
    let output = Command::new(executable("ESKA_METADATA_CLI", "eska-project"))
        .current_dir(root)
        .args(["--lang", language, "metadata"])
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("build eska-project or select ESKA_METADATA_CLI");
    assert_eq!(
        output.status.code(),
        Some(i32::from(!success)),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("one machine JSON response");
    assert_eq!(value["schema_version"], 1);
    value
}

fn create(root: &Path, label: &str, arguments: &[&str]) -> Created {
    let before = source_files(&root.join("src"));
    let mut preview = vec!["add"];
    preview.extend_from_slice(arguments);
    preview.extend(["--dry-run", "--no-input", "--format", "json"]);
    let plan = cli(root, "ru-RU", &preview, true);
    assert_eq!(
        before,
        source_files(&root.join("src")),
        "preview must be read-only"
    );
    let source = root.join("src").join(
        plan["result"]["source"]["value"]
            .as_str()
            .expect("relative source path"),
    );
    let mut expected = before[&source].clone();
    let mut changes = plan["result"]["changes"]
        .as_array()
        .expect("plan changes")
        .clone();
    changes.sort_by_key(|change| std::cmp::Reverse(change["range"]["start"].as_u64().unwrap()));
    for change in changes {
        let start = usize::try_from(change["range"]["start"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(change["range"]["end"].as_u64().unwrap()).unwrap();
        expected.splice(start..end, change["replacement"].as_str().unwrap().bytes());
    }
    let file = root.join(format!("{}-plan.json", label.replace([':', '/'], "_")));
    fs::write(&file, serde_json::to_vec(&plan).unwrap()).expect("save exact preview");
    let file = file.to_str().expect("UTF-8 test path");
    let replay = cli(
        root,
        "en-US",
        &[
            "add",
            "--plan",
            file,
            "--dry-run",
            "--no-input",
            "--format",
            "json",
        ],
        true,
    );
    assert_eq!(plan, replay, "saved plan retains all allocated identities");
    cli(
        root,
        "en-US",
        &["add", "--plan", file, "--no-input", "--format", "json"],
        true,
    );
    assert_eq!(
        fs::read(&source).unwrap(),
        expected,
        "write matches reviewed bytes"
    );
    for (path, bytes) in before {
        if path != source {
            assert_eq!(fs::read(path).unwrap(), bytes, "unrelated file changed");
        }
    }
    Created {
        uuid: plan["result"]["prepared"]["identity"]["uuid"]
            .as_str()
            .unwrap()
            .to_owned(),
        source: source.strip_prefix(root.join("src")).unwrap().to_owned(),
        table: plan["result"]["prepared"]["request"]["kind"] == "table-part",
    }
}

fn composite(root: &Path, owner: &str, label: &str, reference: bool) -> Created {
    let schema = cli(root, "en-US", &["create-schema", "--owner", owner], true);
    let mut types = vec![
        json!({"kind":"string","length":50,"fixed":false}),
        json!({"kind":"number","digits":10,"fractionDigits":3,"nonNegative":false}),
        json!({"kind":"date","fractions":"dateTime"}),
        json!({"kind":"boolean"}),
    ];
    if reference {
        types.push(json!({"kind":"reference","object":"catalog:NativeGoods"}));
    }
    let request = json!({"schemaVersion":1,"request":{
        "owner":owner,"snapshot":schema["result"]["snapshot"],"kind":"attribute",
        "name":"NativeComposite","comment":"Native CLI <&> roundtrip",
        "synonyms":[{"language":"ru","content":"Составной реквизит"}],
        "types":types
    }});
    let input = root.join(format!("{}-request.json", label.replace([':', '/'], "_")));
    fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
    create(root, label, &["--input", input.to_str().unwrap()])
}

fn scalar_cases() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("StringUnlimited", &["--type", "string", "--length", "0"]),
        ("StringMax", &["--type", "string", "--length", "1024"]),
        (
            "StringFixedMin",
            &["--type", "string", "--length", "1", "--fixed-length"],
        ),
        (
            "StringFixedMax",
            &["--type", "string", "--length", "100", "--fixed-length"],
        ),
        (
            "NumberDefault",
            &[
                "--type",
                "number",
                "--digits",
                "0",
                "--fraction-digits",
                "0",
            ],
        ),
        (
            "NumberMin",
            &[
                "--type",
                "number",
                "--digits",
                "1",
                "--fraction-digits",
                "0",
            ],
        ),
        (
            "NumberMax",
            &[
                "--type",
                "number",
                "--digits",
                "38",
                "--fraction-digits",
                "38",
            ],
        ),
        (
            "NumberPositive",
            &[
                "--type",
                "number",
                "--digits",
                "15",
                "--fraction-digits",
                "2",
                "--nonnegative",
            ],
        ),
        ("DateOnly", &["--type", "date", "--date-fractions", "date"]),
        ("TimeOnly", &["--type", "date", "--date-fractions", "time"]),
        (
            "DateTime",
            &["--type", "date", "--date-fractions", "date-time"],
        ),
        ("Boolean", &["--type", "boolean"]),
        (
            "Reference",
            &["--type", "reference", "--reference", "catalog:NativeGoods"],
        ),
    ]
}

fn create_matrix(root: &Path) -> Vec<Created> {
    let mut created = Vec::new();
    let cases = scalar_cases();
    for owner in ["catalog:NativeGoods", "document:PostingProbe"] {
        for (name, flags) in &cases {
            let name = format!("Native{name}");
            let mut arguments = vec!["attribute", "--owner", owner, "--name", &name];
            arguments.extend_from_slice(flags);
            created.push(create(root, &format!("{owner}-{name}"), &arguments));
        }
        created.push(composite(root, owner, &format!("{owner}-composite"), true));
        created.push(create(
            root,
            &format!("{owner}-table"),
            &[
                "table-part",
                "--owner",
                owner,
                "--name",
                "NativeLines",
                "--synonym",
                "ru=Строки",
            ],
        ));
        let table = format!("{owner}/tabular-section:NativeLines");
        for index in [1, 6, 8, 12] {
            let (name, flags) = cases[index];
            let name = format!("Native{name}");
            let mut arguments = vec!["attribute", "--owner", &table, "--name", &name];
            arguments.extend_from_slice(flags);
            created.push(create(root, &format!("{owner}-row-{name}"), &arguments));
        }
        created.push(composite(
            root,
            &table,
            &format!("{owner}-row-composite"),
            true,
        ));
        for name in ["ThisObject", "ЭтотОбъект"] {
            created.push(create(
                root,
                &format!("{owner}-row-{name}"),
                &[
                    "attribute",
                    "--owner",
                    &table,
                    "--name",
                    name,
                    "--type",
                    "boolean",
                ],
            ));
        }
    }
    created
}

fn seed(root: &Path) {
    let source = root.join("src");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/eska-debug/tests/fixtures/posting/source"),
        &source,
    );
    let catalog = include_str!("../fixtures/metadata-creation-native/Catalog.xml");
    let parsed = Document::parse(catalog).unwrap();
    let children = parsed
        .root_element()
        .first_element_child()
        .unwrap()
        .children()
        .find(|node| node.has_tag_name((MD, "ChildObjects")))
        .unwrap();
    let mut input = catalog.to_owned();
    input.replace_range(children.range(), "<ChildObjects/>");
    fs::create_dir_all(source.join("Catalogs")).unwrap();
    fs::write(
        source.join("Catalogs/NativeGoods.xml"),
        format!("\u{feff}{}", input.replace('\n', "\r\n")),
    )
    .unwrap();
    let document = source.join("Documents/PostingProbe.xml");
    fs::write(
        &document,
        fs::read_to_string(&document)
            .unwrap()
            .replace("<ChildObjects/>", ""),
    )
    .unwrap();
    let configuration = source.join("Configuration.xml");
    fs::write(
        &configuration,
        fs::read_to_string(&configuration).unwrap().replace(
            "<Document>PostingProbe</Document>",
            "<Document>PostingProbe</Document><Catalog>NativeGoods</Catalog>",
        ),
    )
    .unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype='configuration'\n").unwrap();
}

fn verify_created(root: &Path, created: &[Created], external_file: Option<&str>) {
    for case in created {
        let input = fs::read_to_string(root.join("src").join(&case.source)).unwrap();
        let exported_path = external_file.map_or_else(|| case.source.clone(), PathBuf::from);
        let exported = fs::read_to_string(root.join("export").join(exported_path)).unwrap();
        let before = Document::parse(&input).unwrap();
        let after = Document::parse(&exported).unwrap();
        let find = |node: Node<'_, '_>| node.attribute("uuid") == Some(case.uuid.as_str());
        let before = before.descendants().find(|node| find(*node)).unwrap();
        let after = after
            .descendants()
            .find(|node| find(*node))
            .expect("native export retains created UUID");
        assert_eq!(property(before, "Name"), property(after, "Name"));
        assert_eq!(property(before, "Comment"), property(after, "Comment"));
        let synonyms = |node: Node<'_, '_>| {
            node.children()
                .find(|child| child.has_tag_name((MD, "Properties")))
                .unwrap()
                .children()
                .find(|child| child.has_tag_name((MD, "Synonym")))
                .map_or_default(leaves)
        };
        assert_eq!(synonyms(before), synonyms(after));
        if case.table {
            let identities = |node: Node<'_, '_>| {
                node.descendants()
                    .filter(|child| child.has_tag_name((XR, "GeneratedType")))
                    .map(|child| {
                        (
                            child.attribute("name").unwrap().to_owned(),
                            child.attribute("category").unwrap().to_owned(),
                            leaves(child),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(identities(before), identities(after));
        } else {
            let types = |node: Node<'_, '_>| {
                leaves(
                    node.descendants()
                        .find(|child| child.has_tag_name((MD, "Type")))
                        .unwrap(),
                )
            };
            assert_eq!(
                types(before),
                types(after),
                "native platform changed type for {}",
                case.uuid
            );
        }
    }
}

#[test]
#[ignore = "requires built eska-project, ESKA_NATIVE_IBCMD and licensed 1C 8.3.27"]
fn cli_created_metadata_survives_native_import_apply_export() {
    assert!(
        std::env::var_os("ESKA_TEST_ROOT").is_some(),
        "set ESKA_TEST_ROOT to sibling eska-playground"
    );
    let tool = std::env::var_os("ESKA_NATIVE_IBCMD").expect("select installed ibcmd");
    let root = TestDir::new();
    seed(&root.0);
    let created = create_matrix(&root.0);
    assert_eq!(created.len(), 44);
    let data = option("--data", &root.0.join("data"));
    native(
        &tool,
        &root.0,
        "create",
        &["infobase".into(), "create".into(), data.clone()],
    );
    native(
        &tool,
        &root.0,
        "import",
        &[
            "infobase".into(),
            "config".into(),
            "import".into(),
            data.clone(),
            root.0.join("src").into_os_string(),
        ],
    );
    native(
        &tool,
        &root.0,
        "apply",
        &[
            "infobase".into(),
            "config".into(),
            "apply".into(),
            data.clone(),
            "--force".into(),
        ],
    );
    native(
        &tool,
        &root.0,
        "export",
        &[
            "infobase".into(),
            "config".into(),
            "export".into(),
            data,
            root.0.join("export").into_os_string(),
        ],
    );
    verify_created(&root.0, &created, None);
}
