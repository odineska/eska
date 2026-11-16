//! EPF/ERF use native DataProcessor/Report table type names despite their external owner tag.

use std::{ffi::OsStr, fs, path::Path};

use roxmltree::Document;

use super::{
    Created, cli, composite, create, executable, native, property, scalar_cases, source_files,
    verify_created,
};
use crate::support::TestDir;

fn seed(root: &Path, kind: &str, source: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    let descriptor = Document::parse(source).unwrap();
    let name = property(
        descriptor.root_element().first_element_child().unwrap(),
        "Name",
    )
    .unwrap();
    fs::create_dir_all(root.join("src").join(name).join("Ext")).unwrap();
    fs::write(root.join("src").join(format!("{name}.xml")), source).unwrap();
    fs::write(
        root.join("eska.toml"),
        format!("[project]\ntype='{kind}'\n[build]\nplatform_version='8.3.27.2342'\n"),
    )
    .unwrap();
}

fn changes(root: &Path) -> Vec<Created> {
    let object = cli(root, "en-US", &["inspect"], true);
    let owner = object["result"]["object"]["objectId"].as_str().unwrap();
    let cases = scalar_cases();
    let mut created = Vec::new();
    for index in [0, 3, 7, 8, 11] {
        let (name, flags) = cases[index];
        let mut arguments = vec!["attribute", "--owner", owner, "--name", name];
        arguments.extend_from_slice(flags);
        created.push(create(root, name, &arguments));
    }
    created.push(composite(root, owner, "composite", false));
    created.push(create(
        root,
        "table",
        &["table-part", "--owner", owner, "--name", "NativeLines"],
    ));
    let table = format!("{owner}/tabular-section:NativeLines");
    created.push(create(
        root,
        "row",
        &[
            "attribute",
            "--owner",
            &table,
            "--name",
            "NativeText",
            "--type",
            "string",
            "--length",
            "200",
        ],
    ));
    created
}

fn build_and_export(root: &Path, kind: &str, extension: &str, tool: &OsStr) {
    let source = source_files(&root.join("src"));
    let build = executable("ESKA_BUILD_CLI", "eska-build");
    native(
        build.as_os_str(),
        root,
        "build",
        &[
            "--lang".into(),
            "en-US".into(),
            "build".into(),
            "--format".into(),
            "json".into(),
            "--ibcmd".into(),
            tool.to_owned(),
        ],
    );
    assert_eq!(
        source,
        source_files(&root.join("src")),
        "build never rewrites source XML"
    );
    native(
        build.as_os_str(),
        root,
        "unpack",
        &[
            "--lang".into(),
            "en-US".into(),
            "unpack".into(),
            root.join("build")
                .join(format!("{kind}.{extension}"))
                .into_os_string(),
            root.join("export").into_os_string(),
            "--format".into(),
            "json".into(),
            "--platform-version".into(),
            "8.3.27.2342".into(),
            "--ibcmd".into(),
            tool.to_owned(),
        ],
    );
}

#[test]
#[ignore = "requires built eska-project/eska-build, ESKA_NATIVE_IBCMD and 1C 8.3.27.2342"]
fn cli_external_metadata_preserves_canonical_generated_types() {
    assert!(
        std::env::var_os("ESKA_TEST_ROOT").is_some(),
        "set ESKA_TEST_ROOT to sibling eska-playground"
    );
    let tool = std::env::var_os("ESKA_NATIVE_IBCMD").expect("select installed ibcmd");
    let root = TestDir::new();
    for (kind, extension, source) in [
        (
            "processing",
            "epf",
            include_str!("../fixtures/metadata-creation-native/ExternalDataProcessor.xml"),
        ),
        (
            "report",
            "erf",
            include_str!("../fixtures/metadata-creation-native/ExternalReport.xml"),
        ),
    ] {
        let project = root.0.join(kind);
        seed(&project, kind, source);
        let created = changes(&project);
        assert_eq!(created.len(), 8);
        build_and_export(&project, kind, extension, &tool);
        verify_created(&project, &created, Some(&format!("{kind}.xml")));
    }
}
