//! Registered launcher discovery keeps independent source availability and legacy output.
#[path = "../../../tests/support/mod.rs"]
mod support;

use serde_json::{Value, json};
use std::{fs, process::Command};
use support::TestDir;

fn command(root: &TestDir, lang: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_eska-debug"));
    command
        .args(["--lang", lang, "infobases"])
        .env_remove("ESKA_DISTROBOX")
        .env("ESKA_CONFIG_DIR", root.0.join("config"))
        .env("HOME", &root.0)
        .env("APPDATA", &root.0);
    command
}

#[test]
fn legacy_and_extended_unavailable_sources_in_both_locales() {
    let root = TestDir::new();
    let list = root.0.join("list.v8i");
    fs::write(&list, "[Host]\nConnect=Srvr=\"host\";Ref=\"base\";\n").unwrap();
    for lang in ["ru-RU", "en-US"] {
        let output = command(&root, lang)
            .arg("--list")
            .arg(&list)
            .output()
            .unwrap();
        assert!(output.status.success());
        let legacy: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            legacy,
            json!({"schema_version":1,"kind":"registered-infobases","entries":[{"name":"Host","infobase":{"kind":"cluster","host":"host","name":"base"}}]})
        );
        let output = command(&root, lang)
            .arg("--include-distrobox")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stderr, [] as [u8; 0]);
        let extended: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            extended["sources"],
            json!({"host":"unavailable","distrobox":null})
        );
        assert_eq!(extended["entries"], json!([]));
        let missing = command(&root, lang).output().unwrap();
        assert!(!missing.status.success());
        assert_eq!(missing.stdout, [] as [u8; 0]);
        assert_ne!(missing.stderr, [] as [u8; 0]);
        let help = command(&root, lang).arg("--help").output().unwrap();
        assert!(help.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        assert!(help.contains("--include-distrobox"));
        assert!(help.contains("--distrobox <DISTROBOX>"));
    }
}

#[cfg(unix)]
#[test]
fn container_home_priority_duplicates_and_partial_results() {
    use std::os::unix::fs::PermissionsExt;
    let root = TestDir::new();
    let bin = root.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let mock = bin.join("distrobox");
    fs::write(
        &mock,
        r#"#!/bin/sh
[ "$1" = enter ] && [ "$2" = --name ] && [ "$4" = --no-tty ] && [ "$5" = --no-workdir ] && [ "$6" = -- ] || exit 2
[ "$3" = "$EXPECTED_CONTAINER" ] && [ "$DBX_NON_INTERACTIVE" = 0 ] || exit 3
if read -r answer; then exit 4; fi
shift 6
HOME="$CONTAINER_HOME" exec "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
    let home = root.0.join("container");
    fs::create_dir_all(home.join(".1C/1cestart")).unwrap();
    let list = home.join(".1C/1cestart/ibases.v8i");
    fs::write(&list, "[Shared]\nConnect=Srvr=\"host\";Ref=\"base\";\n").unwrap();
    fs::create_dir(root.0.join("config")).unwrap();
    fs::write(
        root.0.join("config/config.toml"),
        "[build]\ncontainer = \"configured\"\n",
    )
    .unwrap();
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    for (explicit, environment, expected) in [
        (None, None, "configured"),
        (None, Some("environment"), "environment"),
        (Some("selected"), Some("environment"), "selected"),
    ] {
        let mut cmd = command(&root, "en-US");
        cmd.arg("--include-distrobox")
            .arg("--list")
            .arg(&list)
            .env("PATH", &path)
            .env("CONTAINER_HOME", &home)
            .env("DBX_NON_INTERACTIVE", "1")
            .env("EXPECTED_CONTAINER", expected);
        if let Some(value) = environment {
            cmd.env("ESKA_DISTROBOX", value);
        }
        if let Some(value) = explicit {
            cmd.arg(format!("--distrobox={value}"));
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["sources"],
            json!({"host":"ready","distrobox":{"container":expected,"status":"ready"}})
        );
        assert_eq!(value["entries"].as_array().unwrap().len(), 2);
        assert_eq!(value["entries"][0]["source"], json!({"kind":"host"}));
        assert_eq!(
            value["entries"][1]["source"],
            json!({"kind":"distrobox","container":expected})
        );
    }
    let output = command(&root, "en-US")
        .args(["--include-distrobox", "--distrobox=selected"])
        .env("PATH", &path)
        .env("CONTAINER_HOME", &home)
        .env("EXPECTED_CONTAINER", "selected")
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["sources"]["host"], "unavailable");
    assert_eq!(value["sources"]["distrobox"]["status"], "ready");
    assert_eq!(value["entries"].as_array().unwrap().len(), 1);
    for container in ["selected", "../unsafe"] {
        let output = command(&root, "ru-RU")
            .args(["--include-distrobox", "--distrobox", container])
            .arg("--list")
            .arg(&list)
            .env("PATH", &path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["sources"]["host"], "ready");
        assert_eq!(value["sources"]["distrobox"]["status"], "unavailable");
        assert_eq!(value["entries"].as_array().unwrap().len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn bounded_container_queries_return_unavailable() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let root = TestDir::new();
    let mock = root.0.join("distrobox");
    let path = std::env::join_paths(
        std::iter::once(root.0.clone())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    for body in ["exec head -c 1048577 /dev/zero", "exec sleep 20"] {
        fs::write(&mock, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
        let start = Instant::now();
        let output = command(&root, "en-US")
            .args(["--include-distrobox", "--distrobox=selected"])
            .env("PATH", &path)
            .output()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["sources"]["distrobox"]["status"], "unavailable");
    }
}
