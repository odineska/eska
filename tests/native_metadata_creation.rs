//! Explicit real-platform checks for the Designer XML used by metadata creation.

#[path = "support/metadata_creation_cli.rs"]
mod cli;
#[path = "support/mod.rs"]
mod support;

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    path::Path,
    process::Command,
};

use roxmltree::{Document, Node};
use support::TestDir;

const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
const XR: &str = "http://v8.1c.ru/8.3/xcf/readable";

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create owned fixture directory");
    for entry in fs::read_dir(source).expect("read committed fixture") {
        let entry = entry.expect("read fixture entry");
        let destination = destination.join(entry.file_name());
        if entry.file_type().expect("fixture entry type").is_dir() {
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy committed fixture");
        }
    }
}

fn native(tool: &OsStr, root: &Path, label: &str, arguments: &[OsString]) {
    let result = Command::new(tool)
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("execute explicitly selected ibcmd");
    fs::write(root.join(format!("{label}.stdout")), &result.stdout).expect("write native stdout");
    fs::write(root.join(format!("{label}.stderr")), &result.stderr).expect("write native stderr");
    assert!(
        result.status.success(),
        "{label}: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn option(name: &str, path: &Path) -> OsString {
    let mut option = OsString::from(name);
    option.push("=");
    option.push(path);
    option
}

fn property<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.has_tag_name((MD, "Properties")))?
        .children()
        .find(|child| child.has_tag_name((MD, name)))?
        .text()
}

fn leaves(node: Node<'_, '_>) -> Vec<(String, String)> {
    let mut values: Vec<_> = node
        .descendants()
        .filter(|child| child.is_element() && !child.children().any(|entry| entry.is_element()))
        .map(|child| {
            (
                child.tag_name().name().to_owned(),
                child.text().unwrap_or_default().trim().to_owned(),
            )
        })
        .collect();
    values.sort();
    values
}

fn assert_roundtrip(before: &str, after: &str, catalog: bool) {
    let input = Document::parse(before).expect("parse committed input fixture");
    let output = Document::parse(after).expect("parse native export");
    let exported: BTreeMap<_, _> = output
        .descendants()
        .filter_map(|node| node.attribute("uuid").map(|uuid| (uuid, node)))
        .collect();
    let mut attributes = 0;
    let mut tables = 0;
    for node in input.descendants().filter(Node::is_element) {
        let Some(uuid) = node.attribute("uuid") else {
            continue;
        };
        let actual = *exported.get(uuid).expect("native export retains identity");
        assert_eq!(property(node, "Name"), property(actual, "Name"));
        if node.has_tag_name((MD, "Attribute")) {
            attributes += 1;
            let data_type = |node: Node<'_, '_>| {
                leaves(
                    node.descendants()
                        .find(|child| child.has_tag_name((MD, "Type")))
                        .expect("attribute type"),
                )
            };
            assert_eq!(data_type(node), data_type(actual));
            assert_eq!(property(actual, "FillChecking"), Some("DontCheck"));
            assert_eq!(property(actual, "Indexing"), Some("DontIndex"));
            assert_eq!(property(actual, "FullTextSearch"), Some("Use"));
        }
        if node.has_tag_name((MD, "TabularSection")) {
            tables += 1;
            assert_eq!(property(actual, "FillChecking"), Some("DontCheck"));
            assert_eq!(property(actual, "LineNumberLength"), Some("9"));
            assert_eq!(property(actual, "Use"), catalog.then_some("ForItem"));
            let generated = |node: Node<'_, '_>| {
                node.descendants()
                    .filter(|child| child.has_tag_name((XR, "GeneratedType")))
                    .map(|child| {
                        (
                            child
                                .attribute("name")
                                .expect("generated type name")
                                .to_owned(),
                            child
                                .attribute("category")
                                .expect("generated category")
                                .to_owned(),
                            leaves(child),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(generated(node), generated(actual));
        }
    }
    assert_eq!(attributes, 13);
    assert_eq!(tables, 2);
}

#[test]
#[ignore = "requires ESKA_NATIVE_IBCMD and an installed licensed 1C 8.3.27 platform"]
fn metadata_children_survive_native_import_apply_and_export() {
    assert!(
        std::env::var_os("ESKA_TEST_ROOT").is_some(),
        "set ESKA_TEST_ROOT to the repository's sibling eska-playground"
    );
    let tool = std::env::var_os("ESKA_NATIVE_IBCMD").expect("select an installed ibcmd");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = TestDir::new();
    let source = root.0.join("src");
    copy_tree(
        &repo.join("crates/eska-debug/tests/fixtures/posting/source"),
        &source,
    );
    fs::create_dir_all(source.join("Catalogs")).expect("create fixture catalog directory");
    let catalog = include_str!("fixtures/metadata-creation-native/Catalog.xml");
    let document = include_str!("fixtures/metadata-creation-native/Document.xml");
    fs::write(source.join("Catalogs/NativeGoods.xml"), catalog).expect("write catalog fixture");
    fs::write(source.join("Documents/PostingProbe.xml"), document).expect("write document fixture");
    let configuration = source.join("Configuration.xml");
    let text = fs::read_to_string(&configuration).expect("read seed configuration");
    fs::write(
        configuration,
        text.replace(
            "<Document>PostingProbe</Document>",
            "<Document>PostingProbe</Document><Catalog>NativeGoods</Catalog>",
        ),
    )
    .expect("declare synthetic catalog");
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
            source.into_os_string(),
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
    let exported = root.0.join("export");
    native(
        &tool,
        &root.0,
        "export",
        &[
            "infobase".into(),
            "config".into(),
            "export".into(),
            data,
            exported.clone().into_os_string(),
        ],
    );
    for (input, path, catalog) in [
        (catalog, "Catalogs/NativeGoods.xml", true),
        (document, "Documents/PostingProbe.xml", false),
    ] {
        assert_roundtrip(
            input,
            &fs::read_to_string(exported.join(path)).expect("native descriptor"),
            catalog,
        );
    }
}
