//! Opt-in acceptance with a real 1C installation and an owned temporary file infobase.
#[path = "../../../tests/support/mod.rs"]
mod support;

use std::{env, fs, path::Path, process::Command};

use eska_project_core::paths::native_path;
use serde_json::Value;
use support::TestDir;

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let output = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &output);
        } else {
            fs::copy(entry.path(), output).unwrap();
        }
    }
}

fn native(root: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = Command::new(env::var_os("ESKA_NATIVE_IBCMD").unwrap())
        .current_dir(native_path(root))
        .args(["infobase"])
        .args(arguments.iter().map(|argument| {
            match *argument {
                "input" | "src" | "verified" => native_path(&root.join(argument))
                    .into_owned()
                    .into_os_string(),
                "--base-dir=src" => {
                    format!("--base-dir={}", native_path(&root.join("src")).display()).into()
                }
                _ => (*argument).into(),
            }
        }))
        .arg(format!(
            "--data={}",
            native_path(&root.join("base")).display()
        ))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

fn apply(root: &Path, language: &str, arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_eska-build"))
        .current_dir(native_path(root))
        .args(["apply", "--format", "json", "--lang", language])
        .args(arguments)
        .output()
        .unwrap();
    let value: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"));
    assert_eq!(
        output.status.success(),
        value["status"] != "error",
        "{output:?}"
    );
    value
}

fn baseline(root: &Path) -> std::path::PathBuf {
    fs::read_dir(root.join(".eska"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("apply-")
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
        })
        .unwrap()
}

#[test]
#[ignore = "requires ESKA_NATIVE_IBCMD, ESKA_NATIVE_VERSION and ESKA_TEST_ROOT"]
fn existing_file_database_reconciles_without_import_and_keeps_real_changes() {
    assert!(env::var_os("ESKA_TEST_ROOT").is_some());
    let directory = TestDir::new();
    let root = directory.0.as_path();
    fs::create_dir(root.join("base")).unwrap();
    fs::create_dir(root.join(".eska")).unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype='configuration'\n").unwrap();
    let binary = serde_json::to_string(&env::var("ESKA_NATIVE_IBCMD").unwrap()).unwrap();
    let version = serde_json::to_string(&env::var("ESKA_NATIVE_VERSION").unwrap()).unwrap();
    fs::write(
        root.join(".eska/settings_local.toml"),
        format!("[test.infobase]\nkind='file'\npath='base/db-data'\n[test.runner]\nkind='local'\nibcmd={binary}\nversion={version}\n"),
    ).unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/forms/native-dendrogram/source"),
        &root.join("input"),
    );
    native(root, &["create"]);
    native(root, &["config", "import", "input"]);
    native(root, &["config", "apply", "--force"]);
    native(root, &["config", "export", "src"]);
    let generation = native(root, &["config", "generation-id"]);
    let first = apply(root, "en-US", &[]);
    assert_eq!(first["plan"]["mode"], "unchanged", "{first}");
    assert_eq!(first["plan"]["changes"], serde_json::json!({}));
    assert_eq!(first, apply(root, "ru-RU", &[]));
    assert_eq!(native(root, &["config", "generation-id"]), generation);

    // A first comparison with an actual edit must adopt the database, not the local source.
    fs::remove_file(baseline(root)).unwrap();
    let language = root.join("src/Languages/Русский.xml");
    let original = fs::read_to_string(&language).unwrap();
    let edited = original.replace("<Comment/>", "<Comment>Приёмка</Comment>");
    assert_ne!(edited, original);
    fs::write(&language, &edited).unwrap();
    let preview = apply(root, "ru-RU", &[]);
    assert_eq!(preview["plan"]["mode"], "incremental", "{preview}");
    assert_eq!(preview["plan"]["changes"].as_object().unwrap().len(), 1);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    fs::write(&language, edited.replace("Приёмка", "Изменённый снимок")).unwrap();
    assert_eq!(
        apply(root, "en-US", &["--confirm", token])["error"]["code"],
        "stale_preview"
    );
    assert_eq!(native(root, &["config", "generation-id"]), generation);
    let preview = apply(root, "en-US", &[]);
    let token = preview["plan"]["confirmation"].as_str().unwrap();
    let applied = apply(root, "ru-RU", &["--confirm", token]);
    assert_eq!(applied["status"], "applied", "{applied}");
    native(root, &["config", "export", "verified"]);
    assert!(
        fs::read_to_string(root.join("verified/Languages/Русский.xml"))
            .unwrap()
            .contains("Изменённый снимок")
    );
    copy_tree(&root.join("verified"), &root.join("src"));
    assert_eq!(apply(root, "en-US", &[])["plan"]["mode"], "unchanged");

    // Never declare an unapplied Designer configuration up-to-date.
    fs::remove_file(baseline(root)).unwrap();
    fs::write(&language, &original).unwrap();
    native(
        root,
        &[
            "config",
            "import",
            "files",
            "--base-dir=src",
            "Languages/Русский.xml",
        ],
    );
    let review = apply(root, "en-US", &[]);
    assert_eq!(review["error"]["code"], "baseline_required", "{review}");
    native(root, &["config", "apply", "--force"]);
    assert_eq!(apply(root, "en-US", &[])["plan"]["mode"], "unchanged");

    let state = baseline(root);
    fs::remove_file(&state).unwrap();
    fs::write(state.with_extension("pending"), "interrupted import").unwrap();
    assert_eq!(
        apply(root, "ru-RU", &[])["error"]["code"],
        "incomplete_apply"
    );
    assert!(!state.exists());
    assert_eq!(
        apply(root, "en-US", &["--profile", "prod"])["status"],
        "error"
    );
}
