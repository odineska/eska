#[path = "../../../tests/support/mod.rs"]
mod support;

use std::{
    fs,
    io::Write as _,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use support::TestDir;

fn call(
    path: &Path,
    language: &str,
    operation: &str,
    file: &str,
    input: Option<&Value>,
    options: &[&str],
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_eska-project"))
        .arg("--project-dir")
        .arg(path)
        .args([
            "--lang",
            language,
            "settings-project",
            operation,
            "--file",
            file,
            "--format",
            "json",
        ])
        .args(options)
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
    child.wait_with_output().unwrap()
}

fn ready(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap();
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["kind"], "project-settings");
    assert_eq!(result["status"], "ready");
    result
}

fn error(output: &Output, code: &str) -> Value {
    assert!(!output.status.success());
    assert_ne!(output.stderr, [] as [u8; 0]);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["kind"], "project-settings");
    assert_eq!(result["status"], "error");
    assert_eq!(result["error"]["code"], code);
    result
}

fn project(root: &Path, manifest: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("eska.toml"), manifest).unwrap();
}

#[test]
fn manifest_preview_preserves_inline_tables_arrays_and_comments_without_writing() {
    let fixture = TestDir::new();
    let root = &fixture.0;
    let manifest = "\
# Header
project = { type = 'configuration' } # Inline
[format.common]
column_limit = 120 # Keep
[[format.overrides]] # Rule
include = ['src/First.bsl'] # Array
[format.overrides.bsl]
column_limit = 100 # Keep override
";
    project(root, manifest);
    let show = |locale| ready(call(root, locale, "show", "manifest", None, &[]));
    let snapshot = show("en-US");
    assert_eq!(snapshot, show("ru-RU"));
    assert_eq!(snapshot["value"]["project"]["type"], "configuration");
    assert_eq!(
        snapshot["format_presets"]["eska"]["bsl"]["column_limit"],
        150
    );
    assert_eq!(snapshot["format_presets"]["1c"]["bsl"]["column_limit"], 120);
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/format-v1.schema.json")).unwrap();
    assert_eq!(snapshot["format_schema"], schema);
    assert!(snapshot.get("format_inherited").is_none());
    let input = json!({
        "revision": snapshot["revision"],
        "edits": [
            {"path": ["project", "name"], "value": "demo"},
            {"path": ["format", "common", "column_limit"], "value": 130},
            {"path": ["format", "bsl", "column_limit"], "value": 140},
            {"path": ["format", "overrides", 0, "include", 1], "value": "src/Second.bsl"},
            {"path": ["format", "overrides", 0, "bsl", "column_limit"], "value": 110},
            {"path": ["format", "overrides", 1, "include", 0], "value": "src/Third.bsl"}
        ]
    });
    let preview = ready(call(
        root,
        "en-US",
        "preview",
        "manifest",
        Some(&input),
        &[],
    ));
    assert_eq!(preview["revision"], snapshot["revision"]);
    assert_eq!(preview["before"], manifest);
    assert_eq!(preview["changed"], true);
    let after = preview["after"].as_str().unwrap();
    assert!(after.contains("type = 'configuration'"));
    assert!(after.contains("# Inline"));
    assert!(after.contains("column_limit = 130 # Keep"));
    assert!(after.contains("[format.bsl]"));
    assert!(after.contains("[[format.overrides]] # Rule"));
    assert!(after.contains("column_limit = 110 # Keep override"));
    assert_eq!(
        preview["value"]["format"]["overrides"][1]["include"],
        json!(["src/Third.bsl"])
    );
    assert_eq!(
        fs::read_to_string(root.join("eska.toml")).unwrap(),
        manifest
    );
    assert!(!root.join(".eska").exists());
    let saved = ready(call(root, "en-US", "save", "manifest", Some(&input), &[]));
    assert_ne!(saved["revision"], snapshot["revision"]);
    assert_eq!(saved["value"], preview["value"]);
    assert_eq!(fs::read_to_string(root.join("eska.toml")).unwrap(), after);
    let stale = call(root, "en-US", "save", "manifest", Some(&input), &[]);
    error(&stale, "settings_changed");
}

#[test]
fn invalid_edits_have_locale_independent_errors_and_leave_manifest_untouched() {
    let fixture = TestDir::new();
    let root = &fixture.0;
    let manifest = "[project]\ntype = 'configuration'\n";
    project(root, manifest);
    let current = ready(call(root, "en-US", "show", "manifest", None, &[]));
    let bad_format = json!({
        "revision": current["revision"],
        "edits": [{"path": ["format", "profile"], "value": "missing"}]
    });
    let english = call(root, "en-US", "save", "manifest", Some(&bad_format), &[]);
    let russian = call(root, "ru-RU", "save", "manifest", Some(&bad_format), &[]);
    assert_eq!(
        error(&english, "invalid_manifest"),
        error(&russian, "invalid_manifest")
    );
    assert_ne!(english.stderr, russian.stderr);
    let null = json!({"revision": current["revision"], "edits": [
        {"path": ["project", "name"], "value": null}
    ]});
    error(
        &call(root, "en-US", "preview", "manifest", Some(&null), &[]),
        "invalid_field",
    );
    assert_eq!(
        fs::read_to_string(root.join("eska.toml")).unwrap(),
        manifest
    );
    assert!(!root.join(".eska").exists());
}

#[test]
fn git_policy_files_have_separate_revisions_and_can_be_created_without_git() {
    let fixture = TestDir::new();
    let root = &fixture.0;
    let manifest = "[workspace]\n";
    project(root, manifest);
    for (file, name, contents) in [
        ("ignore", ".gitignore", "# Rule\n*.tmp\n!important.tmp\n"),
        ("attributes", ".gitattributes", "*.xml text eol=lf\n"),
    ] {
        let path = root.join(name);
        let shown = ready(call(root, "en-US", "show", file, None, &[]));
        assert!(shown["revision"].is_null());
        assert_eq!(shown["value"], "");
        let template = shown["git_template"].as_str().unwrap();
        assert!(template.contains(if file == "ignore" {
            ".eska/"
        } else {
            "*.epf filter=lfs"
        }));
        let input = json!({"revision": null, "value": contents});
        let preview = ready(call(root, "en-US", "preview", file, Some(&input), &[]));
        assert_eq!(preview["before"], "");
        assert_eq!(preview["after"], contents);
        assert!(!path.exists());
        let saved = ready(call(root, "en-US", "save", file, Some(&input), &[]));
        assert_eq!(saved["value"], contents);
        assert!(saved["revision"].is_string());
        let no_change = json!({"revision": saved["revision"], "value": contents});
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            saved,
            ready(call(root, "en-US", "save", file, Some(&no_change), &[]))
        );
        assert_eq!(before, fs::metadata(path).unwrap().modified().unwrap());
    }
    assert_eq!(
        fs::read_to_string(root.join("eska.toml")).unwrap(),
        manifest
    );
    assert!(!root.join(".git").exists());
    error(
        &call(&root.join("src"), "en-US", "show", "ignore", None, &[]),
        "project_discovery",
    );
}

#[test]
fn workspace_member_stays_selected_and_candidate_inherits_root_profiles() {
    let fixture = TestDir::new();
    let root = &fixture.0;
    let workspace = "\
[workspace.members.app]
configuration = 'member' # Keep
[format.profiles.shared.common]
column_limit = 110
";
    project(root, workspace);
    let member = root.join("member");
    project(
        &member,
        "[project]\nname = 'demo'\ntype = 'configuration'\n",
    );
    let shown = ready(call(&member, "en-US", "show", "manifest", None, &[]));
    assert_eq!(shown["value"]["project"]["name"], "demo");
    assert_eq!(
        shown["format_inherited"],
        json!({"profiles": {"shared": {"common": {"column_limit": 110}}}})
    );
    let workspace_shown = ready(call(root, "en-US", "show", "manifest", None, &[]));
    assert!(workspace_shown.get("format_inherited").is_none());
    let invalid = json!({"revision": shown["revision"], "edits": [
        {"path": ["project", "type"], "value": "report"}
    ]});
    error(
        &call(&member, "en-US", "preview", "manifest", Some(&invalid), &[]),
        "invalid_manifest",
    );
    let new_policy = json!({"revision": shown["revision"], "edits": [
        {"path": ["format", "profile"], "value": "shared"}
    ]});
    let saved = ready(call(
        &member,
        "en-US",
        "save",
        "manifest",
        Some(&new_policy),
        &[],
    ));
    assert_eq!(saved["value"]["format"]["profile"], "shared");
    assert_eq!(
        fs::read_to_string(root.join("eska.toml")).unwrap(),
        workspace
    );
}

#[cfg(unix)]
#[test]
fn symlinked_policy_and_lock_storage_are_rejected() {
    use std::os::unix::fs::symlink;

    let fixture = TestDir::new();
    let root = &fixture.0;
    project(root, "[workspace]\n");
    let other = root.join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("rules"), "untouched").unwrap();
    symlink(other.join("rules"), root.join(".gitignore")).unwrap();
    error(
        &call(root, "en-US", "show", "ignore", None, &[]),
        "settings_unsafe_path",
    );
    symlink(&other, root.join(".eska")).unwrap();
    let input = json!({"revision": null, "value": "*.xml text\n"});
    error(
        &call(root, "en-US", "save", "attributes", Some(&input), &[]),
        "settings_unsafe_path",
    );
    assert_eq!(
        fs::read_to_string(other.join("rules")).unwrap(),
        "untouched"
    );
    assert!(!root.join(".gitattributes").exists());
    assert!(!other.join("source-locks").exists());
}

#[cfg(unix)]
#[test]
fn analyzer_candidate_preflight_and_separate_post_save_conflict() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = TestDir::new();
    let root = &fixture.0;
    let manifest = "[project]\ntype = 'configuration'\n";
    project(root, manifest);
    fs::write(root.join("src/Configuration.xml"), r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.17"><Configuration uuid="11111111-1111-1111-1111-111111111111"><Properties><Name>Demo</Name><CompatibilityMode>Version8_3_20</CompatibilityMode></Properties><ChildObjects/></Configuration></MetaDataObject>"#).unwrap();
    let fake = root.join("analyzer");
    fs::write(&fake, r#"#!/bin/sh
case "$1" in
    --version) echo 'bsl-analyzer 1.2.3' ;;
    contract) echo '{"build_version":"1.2.3","cli":{"commands":[{"name":"check-config","args":[{"name":"config"}]},{"name":"analyze","args":[{"name":"config"}]},{"name":"lsp"}]}}' ;;
    check-config)
        test "$2" = "--config" && test -f "$3" || exit 4
        grep -q 'root = "src"' "$3" || exit 5
        if grep -q 'invalid_fixture = true' "$3"; then echo "fixture policy rejected" >&2; exit 6; fi
        ;;
    *) exit 3 ;;
esac
"#).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let flags = ["--analyzer", fake.to_str().unwrap()];
    let shown = ready(call(root, "en-US", "show", "manifest", None, &[]));
    let invalid = json!({"revision": shown["revision"], "edits": [
        {"path": ["tools", "bsl-analyzer", "invalid_fixture"], "value": true}
    ]});
    let rejected = call(root, "en-US", "save", "manifest", Some(&invalid), &flags);
    assert_eq!(
        error(&rejected, "analyzer_validation")["error"]["data"]["code"],
        "check_config"
    );
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("fixture policy rejected"));
    assert_eq!(
        fs::read_to_string(root.join("eska.toml")).unwrap(),
        manifest
    );
    assert!(!root.join(".eska").exists());
    assert!(!root.join("bsl-analyzer.toml").exists());
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);

    let valid = json!({"revision": shown["revision"], "edits": [
        {"path": ["tools", "bsl-analyzer", "valid_fixture"], "value": true}
    ]});
    let preview = ready(call(
        root,
        "en-US",
        "preview",
        "manifest",
        Some(&valid),
        &flags,
    ));
    assert_eq!(preview["analyzer"]["status"], "checked");
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);

    fs::write(root.join("bsl-analyzer.toml"), "# User-owned config\n").unwrap();
    let saved_output = call(root, "en-US", "save", "manifest", Some(&valid), &flags);
    assert!(saved_output.status.success());
    let saved = ready(saved_output);
    assert_eq!(saved["analyzer"]["error"]["code"], "migration_required");
    assert!(
        fs::read_to_string(root.join("eska.toml"))
            .unwrap()
            .contains("valid_fixture = true")
    );
    assert_eq!(
        fs::read_to_string(root.join("bsl-analyzer.toml")).unwrap(),
        "# User-owned config\n"
    );
}
