//! The optional shell preserves the standalone complete-analysis policy and process result.

use crate::support::TestDir;
#[cfg(unix)]
use std::path::Path;
use std::{fs, process::Command};

/// Keep real check JSON and failure statuses identical while PATH contains no other ESKA tools.
#[cfg(unix)]
#[test]
fn delegated_check_preserves_complete_threshold_and_incomplete_results() {
    let directory = TestDir::new();
    let root = directory.0.join("check");
    fs::create_dir_all(root.join("src/Ext")).unwrap();
    fs::write(
        root.join("eska.toml"),
        "[project]\ntype='configuration'\n[tools.eska-check]\nmax_warnings=0\n",
    )
    .unwrap();
    fs::write(root.join("src/Configuration.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Configuration uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>Пример</Name><CompatibilityMode>Version8_3_27</CompatibilityMode></Properties><ChildObjects/></Configuration></MetaDataObject>\r\n").unwrap();
    let module = root.join("src/Ext/ObjectModule.bsl");
    let original = "\u{feff}Процедура Пример()\r\n    А = 1;\r\nКонецПроцедуры\r\n";
    fs::write(&module, original).unwrap();
    let analyzer = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/analyzer-check.py");
    for (mode, code, complete) in [
        ("ok", 0, true),
        ("warning", 1, true),
        ("major", 1, true),
        ("critical", 1, true),
        ("blocker", 1, true),
        ("no-done", 3, false),
    ] {
        fs::write(root.join("stream-mode"), mode).unwrap();
        let mut localized = Vec::new();
        for locale in ["ru", "en"] {
            let mut outputs = Vec::new();
            for owner in ["eska", "eska-check"] {
                let mut command = Command::new(
                    Path::new(env!("CARGO_BIN_EXE_eska"))
                        .with_file_name(format!("{owner}{}", std::env::consts::EXE_SUFFIX)),
                );
                command.current_dir(&directory.0).env("PATH", "").args([
                    "--lang",
                    locale,
                    "--project-dir",
                    "check",
                ]);
                if owner == "eska" {
                    command.arg("check");
                }
                let output = command
                    .args(["--format", "json", "--analyzer"])
                    .arg(&analyzer)
                    .output()
                    .unwrap();
                assert_eq!(output.status.code(), Some(code), "{output:?}");
                let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(document["complete"], complete);
                if matches!(mode, "major" | "critical" | "blocker") {
                    assert_eq!(document["status"], "threshold_exceeded");
                    assert_eq!(document["counts"]["errors"], 1);
                    assert_eq!(document["counts"]["warnings"], 0);
                    assert_eq!(document["files"][0]["diagnostics"][0]["severity"], "Error");
                }
                outputs.push(output);
            }
            assert_eq!(outputs[0].stdout, outputs[1].stdout);
            assert_eq!(outputs[0].stderr, outputs[1].stderr);
            localized.push(outputs[0].stdout.clone());
            assert_eq!(fs::read(&module).unwrap(), original.as_bytes());
        }
        assert_eq!(localized[0], localized[1]);
    }
}

/// Help parses locally even when an explicit component path is broken.
#[test]
fn delegated_check_help_does_not_require_the_component() {
    let directory = TestDir::new();
    for locale in ["ru", "en"] {
        let output = Command::new(env!("CARGO_BIN_EXE_eska"))
            .current_dir(&directory.0)
            .env("ESKA_CHECK_BINARY", directory.0.join("missing-check"))
            .args(["--lang", locale, "check", "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let help = String::from_utf8(output.stdout).unwrap();
        for flag in [
            "--application",
            "--analyzer",
            "--timeout",
            "--format",
            "--report",
        ] {
            assert!(help.contains(flag), "{help}");
        }
        assert_eq!(output.stderr, [] as [u8; 0]);
    }
    assert!(fs::read_dir(&directory.0).unwrap().next().is_none());
}

/// Explicit analyzer paths use the caller's cwd for both delegated checks and config preparation.
#[cfg(unix)]
#[test]
fn relative_analyzer_paths_survive_the_project_working_directory() {
    let directory = TestDir::new();
    let root = directory.0.join("project");
    fs::create_dir_all(root.join("src/Ext")).unwrap();
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::create_dir(directory.0.join("tools")).unwrap();
    fs::write(root.join("eska.toml"), "[project]\ntype='configuration'\n").unwrap();
    fs::write(root.join("src/Configuration.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Configuration uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>Пример</Name></Properties><ChildObjects/></Configuration></MetaDataObject>").unwrap();
    fs::write(
        root.join("src/Ext/ObjectModule.bsl"),
        "Процедура Пример()\nКонецПроцедуры\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/analyzer-check.py"),
        directory.0.join("tools/bsl-analyzer-app"),
    )
    .unwrap();
    // A mistaken project-relative spawn must fail, rather than accidentally use the same tool.
    fs::write(root.join("tools/bsl-analyzer-app"), "project-local file").unwrap();
    for locale in ["ru", "en"] {
        for binary in [
            "./tools/bsl-analyzer-app",
            "tools/bsl-analyzer-app",
            "bsl-analyzer-app",
        ] {
            for owner in ["eska", "eska-check", "eska-project"] {
                let executable = Path::new(env!("CARGO_BIN_EXE_eska"))
                    .with_file_name(format!("{owner}{}", std::env::consts::EXE_SUFFIX));
                let mut command = Command::new(executable);
                command
                    .current_dir(&directory.0)
                    .env("PATH", directory.0.join("tools"))
                    .args(["--lang", locale, "--project-dir", "project"]);
                match owner {
                    "eska" => {
                        command.arg("check");
                    }
                    "eska-project" => {
                        command.args(["analyzer-config", "prepare"]);
                    }
                    _ => {}
                }
                let output = command
                    .args(["--analyzer", binary, "--format", "json"])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{owner} {binary} {locale}: {output:?}"
                );
                let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                if owner != "eska-project" {
                    assert_eq!(document["complete"], true);
                    assert_eq!(document["status"], "passed");
                }
            }
        }
    }
}
