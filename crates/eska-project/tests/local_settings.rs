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

fn call(path: &Path, locale: &str, action: &str, input: Option<&Value>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_eska-project"))
        .current_dir(path)
        .args([
            "--lang",
            locale,
            "settings-local",
            action,
            "--format",
            "json",
        ])
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

fn result(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_refuses_to_save_exposed_or_tracked_settings_and_localizes_errors() {
    let directory = TestDir::new();
    let root = &directory.0;
    fs::write(
        root.join("eska.toml"),
        "[project]\ntype = 'configuration'\n",
    )
    .unwrap();
    fs::create_dir(root.join("src")).unwrap();
    git(root, &["init", "--quiet"]);
    let input = json!({"revision": null, "settings": {"prod": {
        "infobase": {"kind":"file", "path":"Пример базы"},
        "runner":{"kind":"local","version":"8.3.27.2342"},
        "debug":{"url":"http://127.0.0.1:1550"},
        "authentication":{"user":"demo","password_env":"ESKA_DEMO_SECRET"}
    }}});
    let english = call(root, "en-US", "save", Some(&input));
    let russian = call(root, "ru-RU", "save", Some(&input));
    assert!(!english.status.success() && !russian.status.success());
    assert_eq!(result(&english), result(&russian));
    assert_eq!(result(&english)["error"]["code"], "settings_exposed");
    assert!(String::from_utf8_lossy(&english.stderr).contains("Exclude"));
    assert!(String::from_utf8_lossy(&russian.stderr).contains("Исключите"));
    assert!(!root.join(".eska/settings_local.toml").exists());

    fs::write(root.join(".gitignore"), ".eska/\n").unwrap();
    let saved = call(root, "ru-RU", "save", Some(&input));
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    assert_eq!(saved.stderr, [] as [u8; 0]);
    let current = result(&saved);
    assert_eq!(current["schema_version"], 1);
    assert_eq!(current["git_protection"], "ignored");
    assert_eq!(
        current["settings"]["prod"]["infobase"]["path"],
        "Пример базы"
    );
    assert_eq!(current, result(&call(root, "en-US", "show", None)));
    assert_eq!(current, result(&call(root, "ru-RU", "validate", None)));

    git(root, &["add", "--force", "--", ".eska/settings_local.toml"]);
    let tracked = call(root, "en-US", "show", None);
    assert!(tracked.status.success());
    assert_eq!(result(&tracked)["git_protection"], "tracked");
    assert!(String::from_utf8_lossy(&tracked.stderr).contains("Git tracks"));
    let update = json!({"revision": current["revision"], "settings": {}});
    let rejected = call(root, "en-US", "save", Some(&update));
    assert!(!rejected.status.success());
    assert_eq!(result(&rejected)["error"]["code"], "settings_tracked");
}
