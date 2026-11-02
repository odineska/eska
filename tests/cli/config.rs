#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Command};

use crate::support::TestDir;

#[test]
#[cfg(unix)]
fn initializes_and_transactionally_edits_global_config() {
    let fixture = TestDir::new();
    let config_dir = fixture.0.join("settings");
    let init = Command::new(env!("CARGO_BIN_EXE_eska"))
        .env("ESKA_CONFIG_DIR", &config_dir)
        .args(["--lang", "en", "config", "init"])
        .output()
        .expect("initialize config");
    assert!(init.status.success(), "{init:?}");
    let config = config_dir.join("config.toml");
    assert_eq!(
        fs::read_to_string(&config).expect("config"),
        "[build]\nrunner = \"auto\"\n"
    );

    let editor = fixture.0.join("editor");
    fs::write(
        &editor,
        "#!/bin/sh\nprintf '[build]\\nrunner = \"host\"\\n' > \"$1\"\n",
    )
    .expect("editor script");
    let mut permissions = fs::metadata(&editor).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&editor, permissions).expect("permissions");
    let edit = Command::new(env!("CARGO_BIN_EXE_eska"))
        .env("ESKA_CONFIG_DIR", &config_dir)
        .env("ESKA_EDITOR", &editor)
        .args(["--lang", "en", "config", "edit"])
        .output()
        .expect("edit config");
    assert!(edit.status.success(), "{edit:?}");
    assert_eq!(
        fs::read_to_string(&config).expect("updated config"),
        "[build]\nrunner = \"host\"\n"
    );
    let backups: Vec<_> = fs::read_dir(&config_dir)
        .expect("config directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("backup"))
        .collect();
    assert_eq!(backups.len(), 1);
}

#[test]
fn a_concurrent_config_change_preserves_both_versions_in_either_locale() {
    let fixture = TestDir::new();
    #[cfg(windows)]
    let editor = {
        let path = fixture.0.join("editor.cmd");
        fs::write(&path, "@echo off\r\n>\"%~1\" echo # editor draft\r\n>\"%ESKA_CONFIG_DIR%\\config.toml\" echo # concurrent change\r\n").unwrap();
        path
    };
    #[cfg(unix)]
    let editor = {
        let path = fixture.0.join("editor");
        fs::write(&path, "#!/bin/sh\nprintf '# editor draft\\n' > \"$1\"\nprintf '# concurrent change\\n' > \"$ESKA_CONFIG_DIR/config.toml\"\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    for (lang, message) in [
        ("ru", "Ваша отредактированная копия"),
        ("en", "Your edited copy"),
    ] {
        let directory = fixture.0.join(lang);
        let result = Command::new(env!("CARGO_BIN_EXE_eska"))
            .env("ESKA_CONFIG_DIR", &directory)
            .env("ESKA_EDITOR", &editor)
            .args(["--lang", lang, "config", "edit"])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert_eq!(result.stdout.as_slice(), b"");
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(error.contains(message), "{error}");
        let mut drafts = fs::read_dir(&directory).unwrap().filter_map(|entry| {
            let path = entry.unwrap().path();
            path.file_name()?
                .to_str()?
                .starts_with("config.toml.edit-")
                .then_some(path)
        });
        let draft = drafts.next().unwrap();
        assert!(drafts.next().is_none());
        assert!(error.contains(draft.file_name().unwrap().to_str().unwrap()));
        assert_eq!(
            fs::read_to_string(draft).unwrap().trim_end(),
            "# editor draft"
        );
        assert_eq!(
            fs::read_to_string(directory.join("config.toml"))
                .unwrap()
                .trim_end(),
            "# concurrent change"
        );
    }
}
