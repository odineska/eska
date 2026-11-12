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
  infobase:config:generation-id) printf '0123456789012345678901234567890123456789\n' ;;
  infobase:config:apply)
    if test -f pause-apply; then
      touch apply-ready
      exec sleep 30
    fi
    test ! -f fail-apply ;;
  infobase:config:import) exit 0 ;;
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
