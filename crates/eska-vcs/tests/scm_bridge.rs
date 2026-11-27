//! Autonomous SCM process contract and targeted mutation safety.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
#[path = "../../../tests/support/mod.rs"]
mod support;
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
fn bridge(root: &Path, locale: &str, args: &[&str], paths: Option<Value>, success: bool) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_eska-vcs"))
        .args(["--lang", locale, "--project-dir"])
        .arg(root)
        .arg("__bridge")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(paths) = paths {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(paths.to_string().as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.success(), success, "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn unborn_crud_exact_rename_binary_and_locale_independence() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::write(root.join("a"), b"\0\xffabc").unwrap();
    bridge(
        root,
        "ru",
        &["scm-files", "stage", "--paths-stdin"],
        Some(json!(["a"])),
        true,
    );
    let blob = bridge(
        root,
        "en",
        &["scm-blob", "--revision", "index", "--", "a"],
        None,
        true,
    );
    assert_eq!(blob["data"]["content"], "AP9hYmM=");
    bridge(root, "ru", &["scm-files", "unstage", "--", "a"], None, true);
    assert_eq!(
        bridge(
            root,
            "en",
            &["scm-blob", "--revision", "index", "--", "a"],
            None,
            true
        )["data"]["content"],
        Value::Null
    );
    bridge(root, "en", &["scm-files", "stage", "--", "a"], None, true);
    git(root, &["commit", "-m", "base"]);
    fs::rename(root.join("a"), root.join("b")).unwrap();
    bridge(
        root,
        "en",
        &["scm-files", "stage", "--paths-stdin"],
        Some(json!(["a", "b"])),
        true,
    );
    let status = bridge(root, "en", &["scm-status"], None, true);
    assert_eq!(status, bridge(root, "ru", &["scm-status"], None, true));
    assert_eq!(status["data"]["renames"][0]["from"], "a");
    bridge(root, "en", &["scm-files", "unstage", "--", "b"], None, true);
    assert_eq!(
        bridge(
            root,
            "en",
            &["scm-blob", "--revision", "index", "--", "a"],
            None,
            true
        )["data"]["content"],
        "AP9hYmM="
    );
    bridge(root, "en", &["scm-files", "discard", "--", "a"], None, true);
    assert_eq!(fs::read(root.join("a")).unwrap(), b"\0\xffabc");
    bridge(root, "en", &["scm-files", "discard", "--", "b"], None, true);
    assert!(!root.join("b").exists());
}
#[test]
fn rejects_recursive_foreign_and_metadata_paths_before_mutation() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::create_dir(root.join("dir")).unwrap();
    fs::write(root.join("dir/file"), "safe").unwrap();
    for path in ["dir", "../outside", ".git/config", ":(glob)**"] {
        bridge(root, "en", &["scm-files", "stage", "--", path], None, false);
    }
    git(&root.join("dir"), &["init"]);
    bridge(
        root,
        "en",
        &["scm-files", "stage", "--", "dir/file"],
        None,
        false,
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("dir", root.join("link")).unwrap();
        bridge(
            root,
            "en",
            &["scm-files", "discard", "--", "link/file"],
            None,
            false,
        );
    }
    assert_eq!(fs::read_to_string(root.join("dir/file")).unwrap(), "safe");
}

#[test]
fn conflict_stages_read_without_worktree_and_discard_is_refused() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::write(root.join("file"), "base\n").unwrap();
    git(root, &["add", "file"]);
    git(root, &["commit", "-m", "base"]);
    git(root, &["checkout", "-b", "other"]);
    fs::write(root.join("file"), "theirs\n").unwrap();
    git(root, &["commit", "-am", "theirs"]);
    git(root, &["checkout", "-b", "ours", "HEAD~1"]);
    fs::write(root.join("file"), "ours\n").unwrap();
    git(root, &["commit", "-am", "ours"]);
    let merge = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "merge",
            "other",
        ])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let status = bridge(root, "ru", &["scm-status"], None, true);
    assert_eq!(
        status["data"]["conflicts"],
        json!([{"path":"file","path_encoding":"utf-8","base":true,"ours":true,"theirs":true}])
    );
    let monitored = Command::new(env!("CARGO_BIN_EXE_eska-vcs"))
        .args(["--project-dir"])
        .arg(root)
        .args(["monitor", "--events-stdin", "--format", "json"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(monitored.status.success(), "{monitored:?}");
    let snapshot: Value = serde_json::from_slice(&monitored.stdout).unwrap();
    assert_eq!(snapshot["conflicts"], status["data"]["conflicts"]);
    fs::remove_file(root.join("file")).unwrap();
    for (revision, expected) in [
        ("base", "YmFzZQo="),
        ("ours", "b3Vycwo="),
        ("theirs", "dGhlaXJzCg=="),
    ] {
        assert_eq!(
            bridge(
                root,
                "en",
                &["scm-blob", "--revision", revision, "--", "file"],
                None,
                true
            )["data"]["content"],
            expected
        );
    }
    bridge(
        root,
        "en",
        &["scm-files", "discard", "--", "file"],
        None,
        false,
    );
    assert!(!root.join("file").exists());
}

#[test]
fn oversized_blob_is_refused_and_discard_preserves_index_and_sibling() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::write(root.join("large"), vec![0; 32 * 1024 * 1024 + 1]).unwrap();
    fs::write(root.join("file"), "staged\n").unwrap();
    fs::write(root.join("sibling"), "keep\n").unwrap();
    git(root, &["add", "."]);
    bridge(
        root,
        "en",
        &["scm-blob", "--revision", "index", "--", "large"],
        None,
        false,
    );
    fs::write(root.join("file"), "unstaged\n").unwrap();
    fs::write(root.join("sibling"), "other change\n").unwrap();
    bridge(
        root,
        "en",
        &["scm-files", "discard", "--", "file"],
        None,
        true,
    );
    assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "staged\n");
    assert_eq!(
        fs::read_to_string(root.join("sibling")).unwrap(),
        "other change\n"
    );
    assert_eq!(
        bridge(
            root,
            "en",
            &["scm-blob", "--revision", "index", "--", "file"],
            None,
            true
        )["data"]["content"],
        "c3RhZ2VkCg=="
    );
}

#[test]
fn rejects_nested_linked_worktree_and_gitlink_without_metadata() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::write(root.join("base"), "base").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    git(root, &["worktree", "add", "--detach", "foreign", "HEAD"]);
    fs::write(root.join("foreign/base"), "foreign dirty").unwrap();
    bridge(
        root,
        "en",
        &["scm-files", "discard", "--", "foreign/base"],
        None,
        false,
    );
    assert_eq!(
        fs::read_to_string(root.join("foreign/base")).unwrap(),
        "foreign dirty"
    );
    fs::create_dir(root.join("submodule")).unwrap();
    fs::write(root.join("submodule/file"), "keep").unwrap();
    let revision = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let revision = String::from_utf8(revision.stdout).unwrap();
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},submodule", revision.trim()),
        ],
    );
    bridge(
        root,
        "en",
        &["scm-files", "stage", "--", "submodule/file"],
        None,
        false,
    );
    bridge(
        root,
        "en",
        &["scm-files", "discard", "--", "submodule"],
        None,
        false,
    );
    assert_eq!(
        fs::read_to_string(root.join("submodule/file")).unwrap(),
        "keep"
    );
}

#[test]
fn repeated_reads_are_stable_and_repeated_discard_fails_closed() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::write(root.join("file"), "base\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    for operation in ["scm-root", "scm-status"] {
        let first = bridge(root, "en", &[operation], None, true);
        assert_eq!(first, bridge(root, "en", &[operation], None, true));
    }
    fs::write(root.join("file"), "dirty\n").unwrap();
    bridge(
        root,
        "en",
        &["scm-files", "discard", "--", "file"],
        None,
        true,
    );
    let repeat = bridge(
        root,
        "en",
        &["scm-files", "discard", "--", "file"],
        None,
        false,
    );
    assert_eq!(repeat["error"], "stale_status");
    assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "base\n");
    let mut snapshot = bridge(root, "en", &["scm-status"], None, true)["data"].clone();
    snapshot.as_object_mut().unwrap().remove("renames");
    snapshot.as_object_mut().unwrap().remove("conflicts");
    assert_eq!(snapshot["schema_version"], 1);
    assert_eq!(snapshot["kind"], "snapshot");
    assert_eq!(snapshot["repository"]["head"], "attached");
    assert_eq!(snapshot["changes"], json!([]));
}

#[test]
fn local_branch_contract_is_sorted_locale_independent_and_manifest_free() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("file"), "base\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    git(root, &["branch", "topic/ветка"]);
    git(root, &["branch", "alpha"]);
    git(root, &["tag", "tag-only"]);
    git(
        root,
        &["update-ref", "refs/remotes/origin/remote-only", "HEAD"],
    );
    git(root, &["pack-refs", "--all"]);
    let expected = json!({"branches":[
        {"name":"alpha","name_encoding":"utf-8"},
        {"name":"main","name_encoding":"utf-8"},
        {"name":"topic/ветка","name_encoding":"utf-8"}
    ]});
    for locale in ["ru-RU", "en-US"] {
        let discovered = bridge(root, locale, &["scm-root"], None, true);
        assert_eq!(discovered["data"]["branches_available"], true);
        assert_eq!(discovered["data"]["mutations_available"], true);
        let listed = bridge(root, locale, &["scm-branches"], None, true);
        assert_eq!(listed["operation"], "scm_branches");
        assert_eq!(listed["schema_version"], 1);
        assert_eq!(listed["error"], Value::Null);
        assert_eq!(listed["data"], expected);
        git(root, &["checkout", "--detach"]);
        let switched = bridge(
            root,
            locale,
            &["scm-checkout", "--", "topic/ветка"],
            None,
            true,
        );
        assert_eq!(switched["operation"], "scm_checkout");
        assert_eq!(switched["data"], json!({}));
        assert_eq!(switched["error"], Value::Null);
        assert_eq!(
            bridge(root, locale, &["scm-status"], None, true)["data"]["repository"]["branch"],
            "topic/ветка"
        );
        for (name, reason) in [
            ("--force", "branch_invalid"),
            ("-", "branch_invalid"),
            ("@{-1}", "branch_invalid"),
            ("HEAD", "branch_invalid"),
            ("@", "branch_invalid"),
            ("main~1", "branch_invalid"),
            ("origin/remote-only", "branch_not_found"),
            ("tag-only", "branch_not_found"),
            ("absent", "branch_not_found"),
        ] {
            let rejected = bridge(root, locale, &["scm-checkout", "--", name], None, false);
            assert_eq!(rejected["error"], reason, "{name}");
            assert_eq!(rejected["data"], Value::Null);
        }
    }
}

#[test]
fn branch_checkout_preserves_conflicting_tracked_staged_untracked_and_ignored_files() {
    for locale in ["ru-RU", "en-US"] {
        let fixture = support::TestDir::new();
        let root = &fixture.0;
        git(root, &["init", "-b", "main"]);
        fs::write(root.join("file"), "base\n").unwrap();
        fs::write(root.join(".gitignore"), "ignored\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "base"]);
        git(root, &["checkout", "-b", "target"]);
        for path in ["file", "untracked", "ignored"] {
            fs::write(root.join(path), "target\n").unwrap();
            git(root, &["add", "-f", "--", path]);
        }
        git(root, &["commit", "-m", "target"]);
        git(root, &["checkout", "main"]);
        for (path, staged) in [
            ("file", false),
            ("file", true),
            ("untracked", false),
            ("ignored", false),
        ] {
            fs::write(root.join(path), "keep my work\n").unwrap();
            if staged {
                git(root, &["add", "file"]);
            }
            let head = fs::read(root.join(".git/HEAD")).unwrap();
            let rejected = bridge(root, locale, &["scm-checkout", "--", "target"], None, false);
            assert_eq!(rejected["error"], "checkout_failed");
            assert_eq!(
                fs::read_to_string(root.join(path)).unwrap(),
                "keep my work\n"
            );
            assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head);
            // Git may refresh stat entries while refusing checkout; staged content
            // must remain unchanged even if that non-semantic refresh occurs.
            if staged {
                let blob = bridge(
                    root,
                    locale,
                    &["scm-blob", "--revision", "index", "--", "file"],
                    None,
                    true,
                );
                assert_eq!(blob["data"]["content"], "a2VlcCBteSB3b3JrCg==");
                git(root, &["restore", "--staged", "file"]);
            } else {
                assert_eq!(
                    bridge(
                        root,
                        locale,
                        &["scm-blob", "--revision", "index", "--", "file"],
                        None,
                        true
                    )["data"]["content"],
                    "YmFzZQo="
                );
            }
            if path == "file" {
                git(root, &["restore", "file"]);
            } else {
                fs::remove_file(root.join(path)).unwrap();
            }
        }
        let foreign = root.join("other-worktree");
        git(
            root,
            &["worktree", "add", foreign.to_str().unwrap(), "target"],
        );
        let rejected = bridge(root, locale, &["scm-checkout", "--", "target"], None, false);
        assert_eq!(rejected["error"], "checkout_failed");
        assert_eq!(fs::read_to_string(root.join("file")).unwrap(), "base\n");
        assert_eq!(
            fs::read_to_string(foreign.join("file")).unwrap(),
            "target\n"
        );
    }
}

#[test]
fn branches_capability_is_false_without_git_and_reads_remain_available() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("file"), "base\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    let empty_path = root.join("empty-path");
    fs::create_dir(&empty_path).unwrap();
    for locale in ["ru-RU", "en-US"] {
        for (operation, success) in [
            ("scm-root", true),
            ("scm-status", true),
            ("scm-history", true),
            ("scm-branches", false),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_eska-vcs"))
                .env("PATH", &empty_path)
                .args(["--lang", locale, "--project-dir"])
                .arg(root)
                .args(["__bridge", operation])
                .output()
                .unwrap();
            assert_eq!(output.status.success(), success, "{output:?}");
            assert!(output.stderr.is_empty(), "{output:?}");
            let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
            if operation == "scm-root" {
                assert_eq!(reply["data"]["safe_switch_available"], true);
                assert_eq!(reply["data"]["history_available"], true);
                assert_eq!(reply["data"]["branches_available"], false);
                assert_eq!(reply["data"]["mutations_available"], false);
            } else if !success {
                assert_eq!(reply["error"], "git_unavailable");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn branch_listing_preserves_non_utf8_names() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("file"), "base\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    let head = fs::read(root.join(".git/refs/heads/main")).unwrap();
    fs::write(
        root.join(".git/refs/heads")
            .join(OsStr::from_bytes(b"raw-\xff")),
        head,
    )
    .unwrap();
    for locale in ["ru-RU", "en-US"] {
        assert_eq!(
            bridge(root, locale, &["scm-branches"], None, true)["data"]["branches"],
            json!([
                {"name":"main","name_encoding":"utf-8"},
                {"name":"%72%61%77%2D%FF","name_encoding":"percent"}
            ])
        );
    }
}

#[test]
fn branch_picker_creates_from_exact_refs_and_preserves_existing_names() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    git(root, &["commit", "--allow-empty", "-m", "base"]);
    git(root, &["tag", "v1"]);
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repository",
        ],
    );
    git(root, &["update-ref", "refs/remotes/origin/topic", "HEAD"]);
    for locale in ["ru", "en"] {
        let refs = bridge(root, locale, &["scm-refs"], None, true);
        assert!(
            refs["data"]["refs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reference| reference["kind"] == "tag" && reference["name"] == "v1")
        );
        let branch = format!("created-{locale}");
        bridge(
            root,
            locale,
            &[
                "scm-switch",
                "--kind",
                "tag",
                "--branch",
                &branch,
                "--",
                "v1",
            ],
            None,
            true,
        );
        assert_eq!(
            bridge(root, locale, &["scm-status"], None, true)["data"]["repository"]["branch"],
            branch
        );
        assert_eq!(
            bridge(
                root,
                locale,
                &[
                    "scm-switch",
                    "--kind",
                    "head",
                    "--branch",
                    &branch,
                    "--",
                    "HEAD"
                ],
                None,
                false
            )["error"],
            "branch_exists"
        );
        assert_eq!(
            bridge(
                root,
                locale,
                &[
                    "scm-switch",
                    "--kind",
                    "head",
                    "--branch",
                    "bad..name",
                    "--",
                    "HEAD"
                ],
                None,
                false
            )["error"],
            "branch_invalid"
        );
        bridge(
            root,
            locale,
            &["scm-switch", "--kind", "tag", "--", "v1"],
            None,
            true,
        );
        assert!(
            bridge(root, locale, &["scm-status"], None, true)["data"]["repository"]["branch"]
                .is_null()
        );
    }
    bridge(
        root,
        "en",
        &["scm-switch", "--kind", "remote", "--", "origin/topic"],
        None,
        true,
    );
}

#[test]
fn branch_picker_supports_unborn_and_rejects_missing_refs_and_occupied_branches() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    bridge(
        root,
        "en",
        &[
            "scm-switch",
            "--kind",
            "head",
            "--branch",
            "topic",
            "--",
            "HEAD",
        ],
        None,
        true,
    );
    git(root, &["commit", "--allow-empty", "-m", "base"]);
    let commit = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let commit = std::str::from_utf8(&commit.stdout).unwrap().trim();
    bridge(
        root,
        "ru",
        &[
            "scm-switch",
            "--kind",
            "commit",
            "--branch",
            "from-commit",
            "--",
            commit,
        ],
        None,
        true,
    );
    bridge(
        root,
        "ru",
        &["scm-switch", "--kind", "commit", "--", commit],
        None,
        true,
    );
    for (kind, target) in [
        ("local", "missing"),
        ("local", "--detach"),
        ("commit", "HEAD~1"),
        ("commit", "0000000"),
        ("head", "other"),
    ] {
        bridge(
            root,
            "en",
            &["scm-switch", "--kind", kind, "--", target],
            None,
            false,
        );
    }
    let worktree = root.join("linked");
    git(
        root,
        &["worktree", "add", worktree.to_str().unwrap(), "from-commit"],
    );
    bridge(
        root,
        "ru",
        &["scm-switch", "--kind", "local", "--", "from-commit"],
        None,
        false,
    );
}

#[test]
fn branch_picker_commit_prefix_does_not_resolve_hexadecimal_refs() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    git(root, &["commit", "--allow-empty", "-m", "base"]);
    let commit = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let commit = std::str::from_utf8(&commit.stdout).unwrap().trim();
    git(
        root,
        &[
            "-c",
            "advice.detachedHead=false",
            "commit",
            "--allow-empty",
            "-m",
            "other",
        ],
    );
    git(root, &["branch", &commit[..7]]);
    bridge(
        root,
        "en",
        &["scm-switch", "--kind", "commit", "--", &commit[..7]],
        None,
        true,
    );
    let actual = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        std::str::from_utf8(&actual.stdout).unwrap().trim(),
        commit,
        "a hexadecimal ref must not shadow an object prefix"
    );
}

/// Every actual branch-picker checkout shelves source bytes before changing HEAD.
#[test]
fn branch_picker_shelves_create_remote_tag_and_detached_transitions() {
    for selection in [
        vec!["--kind", "head", "--branch", "created", "--", "HEAD"],
        vec!["--kind", "remote", "--", "origin/remote-topic"],
        vec!["--kind", "tag", "--", "v1"],
        vec!["--kind", "local", "--detached", "--", "topic"],
    ] {
        let fixture = support::TestDir::new();
        let root = &fixture.0;
        git(root, &["init", "--initial-branch=main"]);
        fs::write(root.join("file"), b"base").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "base"]);
        git(root, &["branch", "topic"]);
        git(root, &["tag", "v1"]);
        git(root, &["remote", "add", "origin", "."]);
        git(
            root,
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        );
        git(
            root,
            &["update-ref", "refs/remotes/origin/remote-topic", "HEAD"],
        );
        fs::write(root.join("file"), b"staged\r\n").unwrap();
        git(root, &["add", "file"]);
        fs::write(root.join("file"), b"unstaged\r\n").unwrap();
        fs::write(root.join("new"), b"untracked").unwrap();
        let index = fs::read(root.join(".git/index")).unwrap();
        let mut args = vec!["scm-switch"];
        args.extend(selection);
        let result = bridge(root, "ru", &args, None, true);
        assert_eq!(result["data"]["saved"], true);
        assert_eq!(result["data"]["restored"], false);
        assert_eq!(fs::read(root.join("file")).unwrap(), b"base");
        assert!(!root.join("new").exists());
        let result = bridge(
            root,
            "en",
            &["scm-switch", "--kind", "local", "--", "main"],
            None,
            true,
        );
        assert_eq!(result["data"]["restored"], true);
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(root.join("file")).unwrap(), b"unstaged\r\n");
        assert_eq!(fs::read(root.join("new")).unwrap(), b"untracked");
    }
}

#[test]
fn history_reads_merge_multi_tip_refs_and_snapshot_paging() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init", "-b", "main"]);
    let empty = bridge(root, "en", &["scm-history"], None, true);
    assert_eq!(empty["data"]["head"]["unborn"], true);
    assert_eq!(empty["data"]["commits"], json!([]));
    git(root, &["commit", "--allow-empty", "-m", "base"]);
    git(root, &["checkout", "-b", "side"]);
    git(root, &["commit", "--allow-empty", "-m", "side"]);
    git(root, &["tag", "-a", "release", "-m", "release"]);
    git(root, &["update-ref", "refs/remotes/origin/side", "HEAD"]);
    git(root, &["checkout", "main"]);
    git(root, &["commit", "--allow-empty", "-m", "main"]);
    git(root, &["merge", "--no-ff", "side", "-m", "merge"]);
    git(root, &["checkout", "-b", "other", "HEAD~1"]);
    git(root, &["commit", "--allow-empty", "-m", "other"]);
    git(root, &["checkout", "main"]);
    let all = bridge(root, "en", &["scm-history"], None, true);
    assert_eq!(all, bridge(root, "ru", &["scm-history"], None, true));
    let commits = all["data"]["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 5);
    let merge = commits.iter().find(|c| c["subject"] == "merge").unwrap();
    assert_eq!(merge["parents"].as_array().unwrap().len(), 2);
    let side = commits.iter().find(|c| c["subject"] == "side").unwrap();
    for kind in ["local", "remote", "tag"] {
        assert!(
            side["refs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["kind"] == kind)
        );
    }
    let mut paged = Vec::new();
    let mut cursor = None::<String>;
    loop {
        let mut args = vec!["scm-history", "--limit", "1"];
        if let Some(cursor) = cursor.as_deref() {
            args.extend(["--cursor", cursor]);
        }
        let page = bridge(root, "en", &args, None, true);
        paged.extend(page["data"]["commits"].as_array().unwrap().clone());
        cursor = page["data"]["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(&paged, commits);
    let first = bridge(root, "en", &["scm-history", "--limit", "1"], None, true);
    let cursor = first["data"]["next_cursor"].as_str().unwrap();
    git(root, &["checkout", "--detach"]);
    assert_eq!(
        bridge(
            root,
            "en",
            &["scm-history", "--cursor", cursor],
            None,
            false
        )["error"],
        "stale_cursor"
    );
    assert_eq!(
        bridge(root, "en", &["scm-history"], None, true)["data"]["head"]["detached"],
        true
    );
    assert_eq!(
        bridge(root, "en", &["scm-history", "--limit", "0"], None, false)["error"],
        "input"
    );
    assert_eq!(
        bridge(root, "en", &["scm-history", "--cursor", "bad"], None, false)["error"],
        "input"
    );
}

#[test]
fn history_respects_shallow_boundary_and_reports_broken_ancestry() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init", "-b", "main"]);
    git(root, &["commit", "--allow-empty", "-m", "base"]);
    git(root, &["commit", "--allow-empty", "-m", "tip"]);
    let tip = bridge(root, "en", &["scm-history"], None, true)["data"]["commits"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fs::write(root.join(".git/shallow"), format!("{tip}\n")).unwrap();
    let shallow = bridge(root, "en", &["scm-history"], None, true);
    assert_eq!(shallow["data"]["commits"].as_array().unwrap().len(), 1);
    assert_eq!(shallow["data"]["commits"][0]["parents"], json!([]));
    fs::remove_file(root.join(".git/shallow")).unwrap();
    let full = bridge(root, "en", &["scm-history"], None, true);
    let parent = full["data"]["commits"][1]["id"].as_str().unwrap();
    fs::remove_file(
        root.join(".git/objects")
            .join(&parent[..2])
            .join(&parent[2..]),
    )
    .unwrap();
    assert_eq!(
        bridge(root, "en", &["scm-history"], None, false)["error"],
        "history"
    );
}
