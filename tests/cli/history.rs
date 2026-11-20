use std::{fs, path::Path, process::Command};

use serde_json::Value;

use crate::{
    support::TestDir,
    vcs::support::{commit, git, repository},
};

/// Run the built CLI with an explicit locale and isolated project directory.
fn eska(current_dir: &Path, locale: &str, args: &[&str]) -> std::process::Output {
    Command::new(crate::vcs_cli())
        .current_dir(current_dir)
        .env_remove("ESKA_LANG")
        .args(["--lang", locale])
        .args(args)
        .output()
        .expect("run eska")
}

/// Create a configured trunk project with one base and one task commit.
fn project() -> (TestDir, gix::ObjectId, gix::ObjectId) {
    let fixture = repository();
    fs::create_dir(fixture.0.join("src")).expect("create source directory");
    fs::write(
        fixture.0.join("eska.toml"),
        "[project]\ntype = 'configuration'\n[vcs.workflow]\npreset = 'trunk'\n",
    )
    .expect("write project configuration");
    let base = commit(&fixture.0, "base.txt");
    git(&fixture.0, &["checkout", "-b", "task/FI-18"]);
    let task = commit(&fixture.0, "task.txt");
    (fixture, base, task)
}

#[test]
fn json_output_is_stable_locale_independent_and_honors_limit() {
    let (fixture, base, task) = project();
    let mut documents = Vec::new();
    for locale in ["ru", "en"] {
        let output = eska(
            &fixture.0,
            locale,
            &["history", "--limit", "1", "--format", "json"],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stderr, [] as [u8; 0]);
        documents.push(serde_json::from_slice::<Value>(&output.stdout).expect("valid JSON"));
    }

    assert_eq!(documents[0], documents[1]);
    let document = &documents[0];
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["commits"].as_array().unwrap().len(), 1);
    assert_eq!(document["commits"][0]["id"], task.to_string());
    assert_eq!(document["commits"][0]["parents"][0], base.to_string());
    assert_eq!(document["commits"][0]["author"]["name"], "Eska Test");
    assert_eq!(document["commits"][0]["author"]["name_encoding"], "utf-8");
    assert_eq!(
        document["commits"][0]["author"]["email"],
        "eska@example.invalid"
    );
    assert_eq!(document["commits"][0]["author"]["email_encoding"], "utf-8");
    assert_eq!(
        document["commits"][0]["authored_at"],
        "2026-01-01T00:00:00+00:00"
    );
    assert_eq!(document["commits"][0]["subject"], "task.txt");
    assert_eq!(document["commits"][0]["subject_encoding"], "utf-8");
    assert_eq!(document["commits"][0]["task"], "FI-18");
}

#[test]
fn human_output_and_help_are_localized() {
    let (fixture, _base, _task) = project();
    for (locale, expected) in [
        (
            "ru",
            [
                "Показать локальную историю",
                "Задача:",
                "Автор:",
                "Дата:",
                "1 января 2026, 00:00:00 UTC+00:00",
            ],
        ),
        (
            "en",
            [
                "Show local commit history",
                "Task:",
                "Author:",
                "Date:",
                "January 1, 2026, 00:00:00 UTC+00:00",
            ],
        ),
    ] {
        let help = eska(&fixture.0, locale, &["history", "--help"]);
        assert!(help.status.success(), "{help:?}");
        assert!(String::from_utf8_lossy(&help.stdout).contains(expected[0]));

        let output = eska(&fixture.0, locale, &["history", "-n", "1"]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stderr, [] as [u8; 0]);
        let text = String::from_utf8(output.stdout)
            .expect("UTF-8 human output")
            .replace(['\u{2068}', '\u{2069}'], "");
        assert!(!text.contains('\x1b'), "redirected output contains escapes");
        assert!(text.contains("task.txt"), "{text}");
        assert!(!text.contains("base.txt"), "{text}");
        for fragment in &expected[1..] {
            assert!(text.contains(fragment), "missing `{fragment}` in:\n{text}");
        }
        let task = text.find(expected[1]).unwrap();
        let author = text.find(expected[2]).unwrap();
        let date = text.find(expected[3]).unwrap();
        assert!(
            task < author && author < date,
            "unexpected field order:\n{text}"
        );
    }
}

#[test]
fn reading_history_does_not_require_workflow_or_modify_git_state() {
    let fixture = repository();
    fs::create_dir(fixture.0.join("src")).expect("create source directory");
    fs::write(
        fixture.0.join("eska.toml"),
        "[project]\ntype = 'configuration'\n",
    )
    .expect("write project configuration");
    commit(&fixture.0, "base.txt");
    let head = fs::read(fixture.0.join(".git/HEAD")).expect("read HEAD");
    let index = fs::read(fixture.0.join(".git/index")).expect("read index");
    let reference = fs::read(fixture.0.join(".git/refs/heads/main")).expect("read main ref");

    let output = eska(&fixture.0, "en", &["history", "--format", "json"]);
    assert!(output.status.success(), "{output:?}");
    let document: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(document["commits"][0]["task"], Value::Null);
    assert_eq!(fs::read(fixture.0.join(".git/HEAD")).unwrap(), head);
    assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read(fixture.0.join(".git/refs/heads/main")).unwrap(),
        reference
    );
}

#[test]
fn plain_git_history_stays_inside_the_worktree_and_keeps_invalid_manifests_visible() {
    let fixture = TestDir::new();
    fs::write(fixture.0.join("eska.toml"), "broken = [").unwrap();
    let root = fixture.0.join("plain");
    let nested = root.join("dir");
    fs::create_dir_all(&nested).unwrap();
    git(&root, &["init", "--initial-branch=main", "--template="]);
    let id = commit(&root, "base.txt");
    let args = ["history", "--format", "json"];
    let ru = eska(&nested, "ru", &args);
    let en = eska(&nested, "en", &args);
    assert!(ru.status.success() && en.status.success(), "{ru:?} {en:?}");
    assert!(ru.stderr.is_empty() && en.stderr.is_empty());
    assert_eq!(ru.stdout, en.stdout);
    let result: Value = serde_json::from_slice(&en.stdout).unwrap();
    assert_eq!(result["commits"][0]["id"], id.to_string());
    assert_eq!(result["commits"][0]["task"], Value::Null);
    fs::write(root.join("eska.toml"), "also_broken = [").unwrap();
    let invalid = eska(&nested, "en", &args);
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
}

#[test]
fn workspace_root_and_member_use_the_shared_history_policy() {
    let (fixture, _, task) = project();
    fs::write(
        fixture.0.join("eska.toml"),
        "[workspace]\n[workspace.members.app]\nconfiguration = 'member'\n[vcs.workflow]\npreset = 'trunk'\n",
    ).unwrap();
    let member = fixture.0.join("member");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(
        member.join("eska.toml"),
        "[project]\nname = 'app'\ntype = 'configuration'\n",
    )
    .unwrap();
    for start in [&fixture.0, &member] {
        let output = eska(start, "en", &["history", "-n", "1", "--format", "json"]);
        assert!(output.status.success(), "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["commits"][0]["id"], task.to_string());
        assert_eq!(result["commits"][0]["task"], "FI-18");
    }
}

#[test]
fn rejects_limits_outside_the_documented_range() {
    let fixture = TestDir::new();
    for limit in ["0", "1001"] {
        let output = eska(&fixture.0, "en", &["history", "--limit", limit]);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
}
