#![cfg(unix)]

#[path = "../../../tests/support/mod.rs"]
mod support;

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use support::TestDir;

fn apply(root: &Path, lang: &str, args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_eska-build"))
        .current_dir(root)
        .args(["--lang", lang, "apply", "--format", "json"])
        .args(args)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

fn prepare(root: &Path) -> PathBuf {
    for name in [".eska", "src", "test-base"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join("test-base/1Cv8.1CD"), "").unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype = 'configuration'").unwrap();
    fs::write(root.join("src/Configuration.xml"), "<configuration/>").unwrap();
    let module = root.join("src/Module.bsl");
    fs::write(&module, "A = 1;").unwrap();
    let platform = root.join("ibcmd");
    fs::write(
        &platform,
        r#"#!/bin/sh
case "$1:$2:$3" in
  --version::) printf '8.3.27.2342\n' ;;
  infobase:config:generation-id)
    if test -f generation-unavailable; then exit 3; fi
    if test -f generation-override; then
      cat generation-override
    else
      printf '0123456789012345678901234567890123456789\n'
    fi ;;
  infobase:config:apply)
    if test -f pause-apply; then
      touch apply-ready
      exec sleep 30
    fi
    test ! -f fail-apply ;;
  infobase:config:import)
    if test "$4" = files; then cat > "$(dirname "$0")/import-files"; fi ;;
  *) exit 2 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&platform, fs::Permissions::from_mode(0o700)).unwrap();
    let path = serde_json::to_string(&platform).unwrap();
    fs::write(
        root.join(".eska/settings_local.toml"),
        format!(
            "[test.infobase]\nkind='file'\npath='test-base'\n[test.runner]\nkind='local'\nversion='8.3.27.2342'\nibcmd={path}\n"
        ),
    )
    .unwrap();
    module
}

fn initialize(root: &Path) {
    let (_, initial) = apply(root, "en-US", &["--initialize"]);
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(root, "en-US", &["--initialize", "--confirm", token])
            .0
            .status
            .success()
    );
}

#[test]
fn apply_proves_structural_identity_and_keeps_unrelated_root_xml_out_of_the_plan() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let source = root.join("src");
    let languages = source.join("Languages");
    fs::create_dir(&languages).unwrap();
    let default = "9830bf6c-f182-4be1-9d98-642d2985cbca";
    let new = "1130bf6c-f182-4be1-9d98-642d2985cbca";
    let language = |name, uuid| {
        format!(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Language uuid="{uuid}"><Properties><Name>{name}</Name></Properties></Language></MetaDataObject>"#
        )
    };
    let configuration = |name, children| {
        format!(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Configuration uuid="{default}"><Properties><Name>{name}</Name><DefaultLanguage>Language.Default</DefaultLanguage></Properties><ChildObjects><Language>Default</Language>{children}</ChildObjects></Configuration></MetaDataObject>"#
        )
    };
    let main = source.join("Configuration.xml");
    fs::write(languages.join("Default.xml"), language("Default", default)).unwrap();
    fs::write(&main, configuration("Base", "")).unwrap();
    initialize(root);

    let added = languages.join("Added.xml");
    fs::write(&added, language("Added", default)).unwrap();
    fs::write(&main, configuration("Base", "<Language>Added</Language>")).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");

    fs::write(&added, language("Added", new)).unwrap();
    fs::write(&main, configuration("Other", "<Language>Added</Language>")).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");
    fs::write(&main, configuration("Base", "<Language>Added</Language>")).unwrap();

    for (operation, name, previous, expected) in [
        ("added", "Added", None, "Languages/Added.xml"),
        ("renamed", "Renamed", Some("Added"), "Languages/Renamed.xml"),
        ("removed", "Renamed", None, "Languages/Default.xml"),
    ] {
        if operation == "renamed" {
            fs::remove_file(&added).unwrap();
            fs::write(languages.join("Renamed.xml"), language("Renamed", new)).unwrap();
            fs::write(&main, configuration("Base", "<Language>Renamed</Language>")).unwrap();
        } else if operation == "removed" {
            fs::remove_file(languages.join("Renamed.xml")).unwrap();
            fs::write(&main, configuration("Base", "")).unwrap();
        }
        let (out, preview) = apply(root, "en-US", &[]);
        assert!(out.status.success());
        assert_eq!(preview, apply(root, "ru-RU", &[]).1);
        let plan = &preview["plan"];
        assert_eq!(plan["mode"], "incremental");
        assert_eq!(
            plan["import_files"],
            serde_json::json!(["Configuration.xml", expected])
        );
        let group = &plan["object_changes"][0];
        assert_eq!(group["kind"], "language");
        assert_eq!(group["operation"], operation);
        assert_eq!(group["name"], name);
        assert_eq!(group["previous_name"], serde_json::json!(previous));
        let token = plan["confirmation"].as_str().unwrap();
        assert!(
            apply(root, "ru-RU", &["--confirm", token])
                .0
                .status
                .success()
        );
        assert_eq!(apply(root, "en-US", &[]).1["plan"]["mode"], "unchanged");
    }
}

#[test]
fn apply_imports_existing_metadata_properties_and_their_form_resource() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let source = root.join("src");
    let descriptor = "CommonForms/ProbeButtons.xml";
    let form = "CommonForms/ProbeButtons/Ext/Form.xml";
    let original = include_str!(
        "../../../tests/fixtures/forms/native-button-rows/source/CommonForms/ProbeButtons.xml"
    );
    fs::create_dir_all(source.join(form).parent().unwrap()).unwrap();
    fs::write(source.join(descriptor), original).unwrap();
    fs::write(source.join(form), "<Form/>").unwrap();
    initialize(root);
    let edited = original.replace("<Comment/>", "<Comment>Updated</Comment>");
    assert_ne!(edited, original);
    fs::write(source.join(descriptor), &edited).unwrap();
    fs::write(source.join(form), "<Form><Title>Updated</Title></Form>").unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "incremental");
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    let (_, localized) = apply(root, "ru-RU", &[]);
    assert_eq!(preview, localized);
    let list = root.join("import-files");
    assert!(!list.exists());
    assert!(
        apply(root, "en-US", &["--confirm", token])
            .0
            .status
            .success()
    );
    let files = fs::read_to_string(&list).unwrap();
    assert_eq!(
        files.lines().collect::<std::collections::BTreeSet<_>>(),
        [descriptor, form].into_iter().collect()
    );

    let renamed = edited.replace("<Name>ProbeButtons</Name>", "<Name>Another</Name>");
    assert_ne!(renamed, edited);
    fs::write(source.join(descriptor), renamed).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");
    assert!(preview["plan"]["confirmation"].is_null());

    let structure = edited.replace(
        "</CommonForm>",
        "<ChildObjects><Form>NewChild</Form></ChildObjects></CommonForm>",
    );
    assert_ne!(structure, edited);
    fs::write(source.join(descriptor), structure).unwrap();
    let (_, preview) = apply(root, "en-US", &[]);
    assert_eq!(preview["plan"]["mode"], "unsupported");
}

#[test]
fn apply_reviews_and_imports_unchanged_catalog_companions_in_platform_order() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let source = root.join("src");
    let catalog = "Catalogs/ProbeOwner.xml";
    let catalog_form = "Catalogs/ProbeOwner/Forms/ProbeObject.xml";
    let catalog_layout = "Catalogs/ProbeOwner/Forms/ProbeObject/Ext/Form.xml";
    let catalog_original = include_str!(
        "../../../tests/fixtures/forms/native-owner-composition/base/Catalogs/ProbeOwner.xml"
    );
    fs::create_dir_all(source.join(catalog_layout).parent().unwrap()).unwrap();
    fs::write(source.join(catalog), catalog_original).unwrap();
    fs::write(
        source.join(catalog_form),
        include_str!(
            "../../../tests/fixtures/forms/native-owner-composition/base/Catalogs/ProbeOwner/Forms/ProbeObject.xml"
        ),
    )
    .unwrap();
    fs::write(source.join(catalog_layout), "<Form/>").unwrap();
    initialize(root);
    let list = root.join("import-files");
    fs::write(
        source.join(catalog),
        catalog_original.replace("<Comment/>", "<Comment>Catalog property</Comment>"),
    )
    .unwrap();
    let (_, preview) = apply(root, "ru-RU", &[]);
    assert_eq!(preview["plan"]["mode"], "incremental");
    assert_eq!(
        preview["plan"]["changes"],
        serde_json::json!({(catalog):"modified"})
    );
    assert_eq!(
        preview["plan"]["import_files"],
        serde_json::json!([catalog, catalog_form, catalog_layout])
    );
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(root, "en-US", &["--confirm", token])
            .0
            .status
            .success()
    );
    assert_eq!(
        fs::read_to_string(list)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [catalog, catalog_form, catalog_layout]
    );
}

#[test]
fn apply_reports_saved_source_and_database_changes_independently() {
    let root = TestDir::new();
    let root = &root.0;
    let module = prepare(root);

    let (output, no_baseline) = apply(root, "en-US", &[]);
    assert!(!output.status.success());
    assert_eq!(no_baseline["error"]["code"], "baseline_required");
    assert_eq!(
        no_baseline["review"]["inspection"]["status"],
        "unknown_baseline"
    );
    assert_eq!(no_baseline["review"]["inspection"]["sources"], "unknown");
    assert!(no_baseline["review"]["inspection"]["database_generation"].is_string());
    assert!(no_baseline["review"]["changes"].is_null());

    let (_, initial) = apply(root, "en-US", &["--initialize"]);
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    assert!(
        apply(root, "en-US", &["--initialize", "--confirm", token])
            .0
            .status
            .success()
    );
    let (_, unchanged) = apply(root, "en-US", &[]);
    assert_eq!(unchanged["plan"]["inspection"]["status"], "up_to_date");
    assert_eq!(
        unchanged["plan"]["inspection"]["baseline_generation"],
        unchanged["plan"]["inspection"]["database_generation"]
    );
    let generation = root.join("generation-override");
    fs::write(&generation, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n").unwrap();
    let (_, external) = apply(root, "en-US", &[]);
    assert_eq!(external["error"]["code"], "baseline_changed");
    assert_eq!(
        external["review"]["inspection"]["status"],
        "database_changed"
    );
    assert_eq!(external["review"]["changes"], serde_json::json!({}));

    let modified = fs::metadata(&module).unwrap().modified().unwrap();
    fs::write(&module, "A = 2;").unwrap();
    fs::File::options()
        .write(true)
        .open(&module)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let (english, both) = apply(root, "en-US", &[]);
    let (russian, localized) = apply(root, "ru-RU", &[]);
    assert!(!english.status.success() && !russian.status.success());
    assert_ne!(english.stderr, russian.stderr);
    assert_eq!(both, localized);
    assert_eq!(both["error"]["code"], "baseline_changed");
    assert_eq!(both["review"]["inspection"]["status"], "both_changed");
    assert_eq!(both["review"]["inspection"]["sources"], "changed");
    assert_eq!(both["review"]["inspection"]["database"], "changed");
    assert_eq!(both["review"]["changes"]["Module.bsl"], "modified");

    fs::remove_file(generation).unwrap();
    let (_, local) = apply(root, "en-US", &[]);
    assert_eq!(local["plan"]["inspection"]["status"], "local_changes");
    assert_eq!(local["plan"]["inspection"]["database"], "unchanged");
    fs::write(root.join("generation-unavailable"), "").unwrap();
    let (_, unavailable) = apply(root, "en-US", &[]);
    assert_eq!(unavailable["error"]["code"], "generation_failed");
    assert_eq!(
        unavailable["review"]["inspection"]["status"],
        "database_unavailable"
    );
    assert_eq!(unavailable["review"]["inspection"]["sources"], "changed");
    assert_eq!(unavailable["review"]["inspection"]["database"], "unknown");
    assert!(unavailable["review"]["inspection"]["database_generation"].is_null());
    assert_eq!(unavailable["review"]["changes"]["Module.bsl"], "modified");
}

#[test]
fn full_import_confirmation_requires_reviewing_a_new_pending_operation() {
    let root = TestDir::new();
    let root = &root.0;
    prepare(root);
    let (_, preview) = apply(root, "en-US", &["--initialize"]);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    fs::write(root.join("fail-apply"), "").unwrap();
    let (_, failed) = apply(root, "en-US", &["--initialize", "--confirm", token]);
    assert_eq!(failed["error"]["code"], "apply_failed");
    fs::remove_file(root.join("fail-apply")).unwrap();
    let (output, stale) = apply(root, "en-US", &["--initialize", "--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(stale["error"]["code"], "stale_preview");
    let (_, recovery) = apply(root, "en-US", &["--initialize"]);
    assert_eq!(recovery["plan"]["inspection"]["status"], "incomplete");
    assert_ne!(recovery["plan"]["confirmation"], token);
}

#[test]
fn failed_apply_preserves_the_last_snapshot_and_requires_review() {
    let root = TestDir::new();
    let root = &root.0;
    let module = prepare(root);

    let (output, initial) = apply(root, "en-US", &["--initialize"]);
    assert!(output.status.success(), "{output:?}");
    let token = initial["plan"]["confirmation"].as_str().unwrap();
    let (output, _) = apply(root, "ru-RU", &["--initialize", "--confirm", token]);
    assert!(output.status.success(), "{output:?}");
    let phases: Vec<Value> = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(phases[0]["phase"], "import");
    assert_eq!(phases[1]["phase"], "apply");

    let snapshot = fs::read_dir(root.join(".eska"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let before = fs::read(&snapshot).unwrap();
    fs::write(&module, "A = 2;").unwrap();
    let (_, plan) = apply(root, "en-US", &[]);
    assert_eq!(plan["plan"]["mode"], "incremental");
    let token = plan["plan"]["confirmation"].as_str().unwrap();
    fs::write(root.join("fail-apply"), "").unwrap();
    let (output, failure) = apply(root, "en-US", &["--confirm", token]);
    assert!(!output.status.success());
    assert_eq!(failure["error"]["code"], "apply_failed");
    assert_eq!(fs::read(&snapshot).unwrap(), before);

    let (english, error) = apply(root, "en-US", &[]);
    let (russian, localized_error) = apply(root, "ru-RU", &[]);
    assert!(!english.status.success() && !russian.status.success());
    assert_ne!(english.stderr, russian.stderr);
    assert_eq!(error, localized_error);
    assert_eq!(error["error"]["code"], "incomplete_apply");
    assert_eq!(error["review"]["inspection"]["status"], "incomplete");
    assert_eq!(error["review"]["inspection"]["pending_apply"], true);
    assert_eq!(error["review"]["inspection"]["sources"], "changed");

    fs::remove_file(root.join("fail-apply")).unwrap();
    let (_, plan) = apply(root, "ru-RU", &["--initialize"]);
    let token = plan["plan"]["confirmation"].as_str().unwrap();
    let (output, _) = apply(root, "en-US", &["--initialize", "--confirm", token]);
    assert!(output.status.success(), "{output:?}");
    let (_, unchanged) = apply(root, "ru-RU", &[]);
    assert_eq!(unchanged["plan"]["mode"], "unchanged");

    let applied = fs::read(&snapshot).unwrap();
    fs::write(module, "A = 3;").unwrap();
    fs::write(root.join("pause-apply"), "").unwrap();
    let (_, plan) = apply(root, "en-US", &[]);
    let token = plan["plan"]["confirmation"].as_str().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_eska-build"))
        .current_dir(root)
        .args([
            "--lang",
            "en-US",
            "apply",
            "--format",
            "json",
            "--confirm",
            token,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join("apply-ready").is_file() {
        if Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the first apply did not reach the mutation phase");
        }
        thread::sleep(Duration::from_millis(25));
    }
    let (_, concurrent) = apply(root, "en-US", &["--confirm", token]);
    let signalled = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let cancelled = child.wait_with_output().unwrap();
    assert!(signalled.success());
    assert_eq!(concurrent["error"]["code"], "busy");
    assert!(!cancelled.status.success());
    let cancelled: Value = serde_json::from_slice(&cancelled.stdout).unwrap();
    assert_eq!(cancelled["error"]["code"], "cancelled");
    assert_eq!(fs::read(snapshot).unwrap(), applied);
    let (_, pending) = apply(root, "en-US", &[]);
    assert_eq!(pending["error"]["code"], "incomplete_apply");
}
