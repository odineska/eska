use super::*;

#[test]
fn add_help_is_localized_without_discovering_a_project_or_machine_configuration() {
    let fixture = TestDir::new();
    for language in ["ru", "en"] {
        let output = command(&fixture.0, language)
            .args(["add", "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stderr, [] as [u8; 0]);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(if language == "ru" {
            "Использование:"
        } else {
            "Usage:"
        }));
        assert!(text.contains(&format!("eska-project{} add", std::env::consts::EXE_SUFFIX)));
        assert!(text.contains("~/.eska/lib"));
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

fn setup(root: &Path, config: &Path, kind: &str) {
    export(&root.join("src"), kind);
    fs::write(
        root.join("eska.toml"),
        format!(
            "# preserve policy\n[project]\ntype='{kind}'\n[build]\nplatform_version='8.3.27.2325'\n"
        ),
    )
    .unwrap();
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    if !config.exists() {
        export(&config.parent().unwrap().join("library"), "configuration");
        fs::write(config,"# keep machine policy\n[build]\nrunner='host'\n[libraries.ssl_3_1]\nsource='library'\n").unwrap();
    }
}

#[test]
fn add_reuses_registered_sources_offline_for_reports_and_processors_in_both_languages() {
    let fixture = TestDir::new();
    let config = fixture.0.join("machine/config.toml");
    for kind in ["report", "processing"] {
        let mut documents = Vec::new();
        for language in ["ru", "en"] {
            let root = fixture.0.join(format!("{kind}-{language}"));
            setup(&root, &config, kind);
            let before = fs::read(&config).unwrap();
            let output = command(&root, language)
                .env("ESKA_CONFIG_DIR", config.parent().unwrap())
                .env("ESKA_VCS_BINARY", fixture.0.join("absent-vcs"))
                .args(["add", "ssl_3_1", "--format", "json"])
                .output()
                .unwrap();
            let document = success(&output);
            assert_eq!(document["kind"], "library-add");
            assert_eq!(document["installed"], false);
            assert_eq!(document["project_changed"], true);
            assert_eq!(document["catalog_changed"], false);
            assert_eq!(fs::read(&config).unwrap(), before);
            let manifest = root.join("eska.toml");
            let bytes = fs::read(&manifest).unwrap();
            let text = String::from_utf8(bytes.clone()).unwrap();
            assert!(text.contains("# preserve policy"));
            assert!(text.contains("ssl_3_1"));
            let repeated = success(
                &command(&root, language)
                    .env("ESKA_CONFIG_DIR", config.parent().unwrap())
                    .env("ESKA_VCS_BINARY", fixture.0.join("absent-vcs"))
                    .args(["add", "ssl_3_1", "--format", "json"])
                    .output()
                    .unwrap(),
            );
            assert_eq!(repeated["project_changed"], false);
            assert_eq!(fs::read(&manifest).unwrap(), bytes);
            assert!(!root.join("src/Configuration.xml").exists());
            documents.push(document);
        }
        assert_eq!(documents[0], documents[1]);
    }
}

#[test]
fn add_rejects_unknown_names_types_and_conflicting_references_without_vcs() {
    let fixture = TestDir::new();
    let config = fixture.0.join("machine/config.toml");
    for (label, kind, extra, library, expected) in [
        ("unknown", "report", "", "unknown", "library_unknown"),
        (
            "type",
            "configuration",
            "",
            "ssl_3_1",
            "library_project_type",
        ),
        (
            "source",
            "processing",
            "[analysis.configuration]\nsource='../old'\n",
            "ssl_3_1",
            "library_conflict",
        ),
        (
            "other",
            "report",
            "[analysis.configuration]\nlibrary='ssl_3_2'\n",
            "ssl_3_1",
            "library_conflict",
        ),
    ] {
        let root = fixture.0.join(label);
        setup(&root, &config, kind);
        let manifest = root.join("eska.toml");
        fs::write(
            &manifest,
            format!("{}{extra}", fs::read_to_string(&manifest).unwrap()),
        )
        .unwrap();
        let before = fs::read(&manifest).unwrap();
        let mut errors = Vec::new();
        for language in ["ru", "en"] {
            let output = command(&root, language)
                .env("ESKA_CONFIG_DIR", config.parent().unwrap())
                .env("ESKA_VCS_BINARY", fixture.0.join("absent-vcs"))
                .args(["add", library, "--format", "json"])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            let error: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(error["error"]["code"], expected);
            assert_ne!(output.stderr, [] as [u8; 0]);
            errors.push(error);
        }
        assert_eq!(errors[0], errors[1]);
        assert_eq!(fs::read(manifest).unwrap(), before);
        assert!(!root.join(".eska").exists());
    }
}
