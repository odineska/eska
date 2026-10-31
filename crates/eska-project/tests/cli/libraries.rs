//! Named references share the direct-source contract while keeping global inputs read-only.

use super::*;

fn manifest(root: &Path, kind: &str, reference: &str) {
    export(&root.join("src"), kind);
    let name = root.file_name().unwrap().to_str().unwrap();
    fs::write(root.join("eska.toml"), format!(
        "[project]\nname={name:?}\ntype={kind:?}\nsource='src'\n[build]\nplatform_version='8.3.27.2325'\n{reference}"
    )).unwrap();
}

fn catalog(path: &Path, source: &Path, other: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = source.to_str().unwrap();
    fs::write(path, format!(
        "[build]\nrunner='host'\n[libraries.ssl_3_1]\nsource={source:?}\n[libraries.other]\nsource={other:?}\n"
    )).unwrap();
}

fn named(root: &Path, config: &Path, language: &str, args: &[&str]) -> Output {
    command(root, language)
        .env("ESKA_CONFIG_DIR", config.parent().unwrap())
        .args(args)
        .output()
        .unwrap()
}

fn status(root: &Path, config: &Path) -> Value {
    let output = named(
        root,
        config,
        "en",
        &["analyzer-config", "status", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "missing");
    value
}

#[test]
fn selected_library_is_shared_by_report_processing_and_extension_without_writes() {
    let fixture = TestDir::new();
    let library = fixture.0.join("БСП с пробелами/src");
    let original = export(&library, "configuration");
    let config = fixture.0.join("settings/config.toml");
    catalog(
        &config,
        Path::new("../БСП с пробелами/src"),
        "unselected/missing",
    );
    for kind in ["report", "processing", "extension"] {
        let root = fixture.0.join(kind);
        manifest(&root, kind, "[analysis.configuration]\nlibrary='ssl_3_1'\n");
        let before = fs::read(root.join("eska.toml")).unwrap();
        let english = named(&root, &config, "en", &["context", "--format", "json"]);
        let russian = named(&root, &config, "ru", &["context", "--format", "json"]);
        let value = success(&english);
        assert_eq!(english.stdout, russian.stdout);
        assert!(russian.status.success(), "{russian:?}");
        let sources = value["context"]["sources"].as_array().unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0]["role"], "configuration");
        assert_eq!(sources[0]["member"], Value::Null);
        assert_eq!(sources[0]["library"]["name"], "ssl_3_1");
        assert_eq!(
            Path::new(sources[0]["source"]["value"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            library.canonicalize().unwrap(),
        );
        assert_eq!(sources[1]["project_type"], kind);
        assert_eq!(fs::read(root.join("eska.toml")).unwrap(), before);
        assert!(!root.join("bsl-analyzer.toml").exists());
        assert!(!root.join(".eska").exists());
        for (name, bytes) in &original {
            assert_eq!(fs::read(library.join(name)).unwrap(), *bytes);
        }
    }
}

#[test]
fn fingerprints_follow_only_the_selected_entry_and_config_location() {
    let fixture = TestDir::new();
    let first = fixture.0.join("first/src");
    let second = fixture.0.join("second/src");
    export(&first, "configuration");
    export(&second, "configuration");
    let config = fixture.0.join("settings/config.toml");
    let root = fixture.0.join("report");
    manifest(
        &root,
        "report",
        "[analysis.configuration]\nlibrary='ssl_3_1'\n",
    );
    let original = fs::read(root.join("eska.toml")).unwrap();
    catalog(&config, &first, "unselected/first");
    let initial = status(&root, &config);
    catalog(&config, &first, "unselected/second");
    assert_eq!(status(&root, &config), initial);
    catalog(&config, &second, "unselected/second");
    let changed = status(&root, &config);
    assert_ne!(
        changed["ownership"]["input_fingerprint"],
        initial["ownership"]["input_fingerprint"]
    );
    assert_ne!(
        changed["ownership"]["body_sha256"],
        initial["ownership"]["body_sha256"]
    );
    let another_config = fixture.0.join("another machine/config.toml");
    catalog(&another_config, &first, "unused");
    let another = status(&root, &another_config);
    assert_eq!(
        another["ownership"]["body_sha256"],
        initial["ownership"]["body_sha256"]
    );
    assert_ne!(
        another["ownership"]["input_fingerprint"],
        initial["ownership"]["input_fingerprint"]
    );
    assert_eq!(fs::read(root.join("eska.toml")).unwrap(), original);
}

#[test]
fn catalog_errors_keep_library_name_config_path_and_locale_independent_codes() {
    let fixture = TestDir::new();
    let root = fixture.0.join("report");
    manifest(
        &root,
        "report",
        "[analysis.configuration]\nlibrary='ssl_3_1'\n",
    );
    let config = fixture.0.join("settings/config.toml");
    fs::create_dir(config.parent().unwrap()).unwrap();
    let bad_xml = fixture.0.join("bad XML");
    fs::create_dir(&bad_xml).unwrap();
    fs::write(bad_xml.join("Configuration.xml"), "<invalid>").unwrap();
    for (contents, expected, cause) in [
        (None, "library_configuration", "unknown_library"),
        (
            Some("[libraries.ssl_3_1]\nsource=''\n".into()),
            "library_configuration",
            "empty_library_source",
        ),
        (
            Some("broken TOML [".into()),
            "library_configuration",
            "invalid_toml",
        ),
        (
            Some("[libraries.ssl_3_1]\nsource='../missing'\n".into()),
            "library_source",
            "",
        ),
        (
            Some(format!("[libraries.ssl_3_1]\nsource={bad_xml:?}\n")),
            "library_source",
            "",
        ),
    ] {
        if let Some(contents) = contents {
            fs::write(&config, contents).unwrap();
        } else if config.exists() {
            fs::remove_file(&config).unwrap();
        }
        let mut previous = None;
        for language in ["ru", "en"] {
            let output = named(&root, &config, language, &["context", "--format", "json"]);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["error"]["code"], expected);
            assert_eq!(value["error"]["data"]["library"], "ssl_3_1");
            assert!(
                value["error"]["data"]["configuration"]["value"]
                    .as_str()
                    .unwrap()
                    .ends_with("config.toml")
            );
            if !cause.is_empty() {
                assert_eq!(value["error"]["data"]["cause"], cause);
            }
            let text = String::from_utf8_lossy(&output.stderr);
            assert!(
                text.contains("ssl_3_1") && text.contains("config.toml"),
                "{text}"
            );
            assert!(!output.stderr.contains(&0x1b));
            if let Some(previous) = previous {
                assert_eq!(value, previous);
            }
            previous = Some(value);
        }
    }
}

#[test]
fn projects_without_a_catalog_reference_ignore_an_unavailable_global_config() {
    let fixture = TestDir::new();
    let root = fixture.0.join("report");
    let config = fixture.0.join("settings/config.toml");
    fs::create_dir(config.parent().unwrap()).unwrap();
    fs::write(&config, "invalid TOML").unwrap();
    manifest(&root, "report", "");
    let context = success(&named(
        &root,
        &config,
        "en",
        &["context", "--format", "json"],
    ));
    assert_eq!(context["context"]["limited_context"], true);
    let library = fixture.0.join("base/src");
    export(&library, "configuration");
    manifest(
        &root,
        "report",
        &format!("[analysis.configuration]\nsource={library:?}\n"),
    );
    let context = success(&named(
        &root,
        &config,
        "ru",
        &["context", "--format", "json"],
    ));
    assert_eq!(context["context"]["limited_context"], false);
    assert!(context["context"]["sources"][0].get("library").is_none());
}

#[test]
fn workspace_enrollment_retains_one_named_reference_and_rejects_member_aliases() {
    let fixture = TestDir::new();
    let root = fixture.0.join("workspace");
    let config = fixture.0.join("settings/config.toml");
    let library = fixture.0.join("base/src");
    export(&library, "configuration");
    catalog(&config, &library, "unused");
    manifest(&root.join("report"), "report", "");
    fs::write(root.join("eska.toml"),
        "[workspace.members.tools]\nconfiguration={library='ssl_3_1'}\nexternal_objects=['report']\n").unwrap();
    success(&named(
        &root,
        &config,
        "en",
        &[
            "new",
            "fix",
            "--type",
            "extension",
            "--application",
            "tools",
            "--format",
            "json",
        ],
    ));
    let input = fs::read_to_string(root.join("eska.toml")).unwrap();
    assert_eq!(input.matches("ssl_3_1").count(), 1);
    assert!(!input.contains(library.to_str().unwrap()));
    let child = root.join("src/fix");
    assert!(
        !fs::read_to_string(child.join("eska.toml"))
            .unwrap()
            .contains("ssl_3_1")
    );
    export(&child.join("src"), "extension");
    let context = success(&named(
        &root,
        &config,
        "en",
        &["context", "--format", "json"],
    ));
    assert_eq!(context["context"]["sources"].as_array().unwrap().len(), 3);
    // A second application owns this same configuration as a writable member.
    manifest(&root.join("owned"), "configuration", "");
    fs::write(root.join("eska.toml"), "[workspace.members.tools]\nconfiguration={library='ssl_3_1'}\nexternal_objects=['report']\n[workspace.members.other]\nconfiguration='owned'\n").unwrap();
    catalog(&config, &root.join("owned/src"), "unused");
    let output = named(
        &root,
        &config,
        "en",
        &["context", "--application", "tools", "--format", "json"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["error"]["code"], "library_source",
        "{value:?}; {output:?}"
    );
    assert_eq!(
        value["error"]["data"]["cause"]["reason"],
        "application_settings"
    );
}
