//! Partial staging uses immutable editor snapshots and never writes the worktree.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
#[path = "../../../tests/support/mod.rs"]
mod support;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
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
    output.stdout
}
fn hash(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut result = String::new();
    for byte in Sha256::digest(bytes) {
        write!(result, "{byte:02x}").unwrap();
    }
    result
}
fn fixture(old: &[u8]) -> support::TestDir {
    let fixture = support::TestDir::new();
    git(&fixture.0, &["init", "-b", "main"]);
    git(&fixture.0, &["config", "core.autocrlf", "false"]);
    fs::write(fixture.0.join("module.bsl"), old).unwrap();
    fs::write(fixture.0.join("sibling"), b"base\n").unwrap();
    git(&fixture.0, &["add", "."]);
    git(&fixture.0, &["commit", "-m", "base"]);
    fixture
}
fn index_blob(root: &Path, file: &str) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["show", &format!(":{file}")])
        .output()
        .unwrap();
    if output.status.success() {
        output.stdout
    } else {
        Vec::new()
    }
}
fn request(root: &Path, file: &str, side: &str, ranges: &[(usize, usize)]) -> Value {
    json!({"schema_version":1,"index_sha256":hash(&index_blob(root, file)),"worktree_sha256":hash(&fs::read(root.join(file)).unwrap_or_default()),"side":side,"ranges":ranges.iter().map(|(start,end)|json!({"start":start,"end":end})).collect::<Vec<_>>()})
}
fn stage(root: &Path, file: &str, request: &Value, error: Option<&str>) -> Value {
    stage_system(root, file, request, error, None)
}
fn stage_system(
    root: &Path,
    file: &str,
    request: &Value,
    error: Option<&str>,
    system: Option<&Path>,
) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_eska-vcs"));
    if let Some(system) = system {
        command
            .env_remove("GIT_CONFIG_NOSYSTEM")
            .env("GIT_CONFIG_SYSTEM", system);
    }
    let mut child = command
        .args(["--lang", "ru", "--project-dir"])
        .arg(root)
        .args(["__bridge", "scm-stage-lines", "--", file])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.success(), error.is_none(), "{output:?}");
    assert_eq!(output.stderr, [] as [u8; 0]);
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    if let Some(error) = error {
        assert_eq!(reply["error"], error, "{reply}");
    }
    reply
}
#[test]
fn one_to_one_replacement_preserves_other_lines_staging_and_worktree() {
    let fixture = fixture(b"one\ntwo\nthree\n");
    let root = &fixture.0;
    fs::write(root.join("module.bsl"), b"ONE\nTWO\nthree\n").unwrap();
    fs::write(root.join("sibling"), b"staged sibling\n").unwrap();
    git(root, &["add", "sibling"]);
    let sibling = git(root, &["ls-files", "--stage", "--", "sibling"]);
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        None,
    );
    assert_eq!(git(root, &["show", ":module.bsl"]), b"ONE\ntwo\nthree\n");
    assert_eq!(
        git(root, &["ls-files", "--stage", "--", "sibling"]),
        sibling
    );
    assert_eq!(
        fs::read(root.join("module.bsl")).unwrap(),
        b"ONE\nTWO\nthree\n"
    );
    git(root, &["commit", "-m", "selected lines"]);
    assert_eq!(
        git(root, &["show", "HEAD:module.bsl"]),
        b"ONE\ntwo\nthree\n"
    );
}
#[test]
fn additions_deletions_and_untracked_allow_individual_lines() {
    let fixture = fixture(b"one\ntwo\nthree\n");
    let root = &fixture.0;
    fs::write(
        root.join("module.bsl"),
        b"one\ninsert one\ninsert two\ntwo\nthree\n",
    )
    .unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(2, 3)]),
        None,
    );
    assert_eq!(
        git(root, &["show", ":module.bsl"]),
        b"one\ninsert two\ntwo\nthree\n"
    );
    fs::write(root.join("module.bsl"), b"one\nthree\n").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "index", &[(1, 2)]),
        None,
    );
    assert_eq!(git(root, &["show", ":module.bsl"]), b"one\ntwo\nthree\n");
    fs::write(root.join("new"), b"first\nsecond\nthird\n").unwrap();
    stage(
        root,
        "new",
        &request(root, "new", "worktree", &[(1, 2)]),
        None,
    );
    assert_eq!(git(root, &["show", ":new"]), b"second\n");
    assert_eq!(
        fs::read(root.join("new")).unwrap(),
        b"first\nsecond\nthird\n"
    );
}
#[test]
fn stale_ambiguous_empty_and_binary_selections_never_mutate_index() {
    let fixture = fixture(b"one\ntwo\nthree\n");
    let root = &fixture.0;
    fs::write(root.join("module.bsl"), b"A\nB\nC\nD\n").unwrap();
    let raw = fs::read(root.join(".git/index")).unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("ambiguous_lines"),
    );
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[]),
        Some("empty_selection"),
    );
    let stale = request(root, "module.bsl", "worktree", &[(0, 4)]);
    fs::write(root.join("module.bsl"), b"changed again\n").unwrap();
    stage(root, "module.bsl", &stale, Some("stale_status"));
    fs::write(root.join("module.bsl"), b"\0binary").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("unsupported_text"),
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), raw);
    assert!(!root.join(".git/index.lock").exists());
}
#[test]
fn crlf_normalization_attributes_and_preserved_staged_mode() {
    let fixture = fixture(b"one\ntwo\nthree\n");
    let root = &fixture.0;
    fs::write(root.join(".gitattributes"), b"*.bsl text eol=lf\n").unwrap();
    git(root, &["update-index", "--chmod=+x", "module.bsl"]);
    fs::write(root.join("module.bsl"), b"ONE\r\nTWO\r\nthree\r\n").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(1, 2)]),
        None,
    );
    assert_eq!(git(root, &["show", ":module.bsl"]), b"one\nTWO\nthree\n");
    assert!(git(root, &["ls-files", "--stage", "module.bsl"]).starts_with(b"100755 "));
    assert_eq!(
        fs::read(root.join("module.bsl")).unwrap(),
        b"ONE\r\nTWO\r\nthree\r\n"
    );
    fs::write(root.join(".gitattributes"), b"*.bsl filter=custom\n").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("unsupported_text"),
    );
}
#[test]
fn locked_ignored_and_foreign_paths_are_rejected() {
    let fixture = fixture(b"one\n");
    let root = &fixture.0;
    fs::write(root.join("module.bsl"), b"ONE\n").unwrap();
    fs::write(root.join(".git/index.lock"), b"other owner").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("index_locked"),
    );
    assert_eq!(
        fs::read(root.join(".git/index.lock")).unwrap(),
        b"other owner"
    );
    fs::remove_file(root.join(".git/index.lock")).unwrap();
    fs::write(root.join(".gitignore"), b"ignored\n").unwrap();
    fs::write(root.join("ignored"), b"new\n").unwrap();
    stage(
        root,
        "ignored",
        &request(root, "ignored", "worktree", &[(0, 1)]),
        Some("ignored"),
    );
    fs::create_dir(root.join("nested")).unwrap();
    git(&root.join("nested"), &["init"]);
    fs::write(root.join("nested/file"), b"new\n").unwrap();
    stage(
        root,
        "nested/file",
        &request(root, "nested/file", "worktree", &[(0, 1)]),
        Some("path"),
    );
}

#[test]
fn unborn_index_and_whole_deleted_file_are_supported() {
    let fixture = support::TestDir::new();
    let root = &fixture.0;
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "core.autocrlf", "false"]);
    fs::write(root.join("new"), b"first\nsecond\n").unwrap();
    assert!(!root.join(".git/index").exists());
    stage(
        root,
        "new",
        &request(root, "new", "worktree", &[(0, 1)]),
        None,
    );
    assert_eq!(git(root, &["show", ":new"]), b"first\n");
    fs::remove_file(root.join("new")).unwrap();
    stage(root, "new", &request(root, "new", "index", &[(0, 1)]), None);
    assert_eq!(git(root, &["ls-files", "--", "new"]), [] as [u8; 0]);
}

#[test]
fn whole_unequal_replacement_and_stale_index_are_checked() {
    let fixture = fixture(b"one\ntwo\nthree\n");
    let root = &fixture.0;
    fs::write(root.join("module.bsl"), b"A\nB\nC\nD\n").unwrap();
    let stale = request(root, "module.bsl", "worktree", &[(0, 4)]);
    git(root, &["add", "module.bsl"]);
    stage(root, "module.bsl", &stale, Some("stale_status"));
    fs::write(root.join("module.bsl"), b"one\ntwo\nthree\n").unwrap();
    git(root, &["add", "module.bsl"]);
    fs::write(root.join("module.bsl"), b"A\nB\nC\nD\n").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 4)]),
        None,
    );
    assert_eq!(git(root, &["show", ":module.bsl"]), b"A\nB\nC\nD\n");
}

#[test]
fn conflicted_path_is_rejected_without_resolving_index() {
    let fixture = fixture(b"base\n");
    let root = &fixture.0;
    git(root, &["checkout", "-b", "other"]);
    fs::write(root.join("module.bsl"), b"other\n").unwrap();
    git(root, &["add", "module.bsl"]);
    git(root, &["commit", "-m", "other"]);
    git(root, &["checkout", "main"]);
    fs::write(root.join("module.bsl"), b"main\n").unwrap();
    git(root, &["add", "module.bsl"]);
    git(root, &["commit", "-m", "main"]);
    let merged = Command::new("git")
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
    assert!(!merged.status.success());
    let index = fs::read(root.join(".git/index")).unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("conflict"),
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}

#[test]
fn system_autocrlf_input_normalizes_selected_lines() {
    let fixture = fixture(b"one\ntwo\n");
    let root = &fixture.0;
    git(root, &["config", "--unset", "core.autocrlf"]);
    let system = root.join("system.gitconfig");
    fs::write(&system, b"[core]\nautocrlf=input\n").unwrap();
    fs::write(root.join("module.bsl"), b"ONE\r\nTWO\r\n").unwrap();
    stage_system(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(1, 2)]),
        None,
        Some(&system),
    );
    assert_eq!(git(root, &["show", ":module.bsl"]), b"one\nTWO\n");
    assert_eq!(
        fs::read(root.join("module.bsl")).unwrap(),
        b"ONE\r\nTWO\r\n"
    );
}

#[test]
fn system_attributes_control_selected_lines_and_global_ignores_are_respected() {
    let fixture = fixture(b"one\ntwo\n");
    let root = &fixture.0;
    let system = root.join("system.gitconfig");
    let attributes = root.join("project.attributes");
    let ignores = root.join("project.ignores");
    fs::write(&attributes, b"*.bsl text eol=lf\n").unwrap();
    fs::write(&ignores, b"ignored\n").unwrap();
    let config_path = |path: &Path| path.to_string_lossy().replace('\\', "/");
    fs::write(
        &system,
        format!(
            "[core]\nattributesFile=\"{}\"\nexcludesFile=\"{}\"\n",
            config_path(&attributes),
            config_path(&ignores)
        ),
    )
    .unwrap();
    fs::write(root.join("module.bsl"), b"ONE\r\nTWO\r\n").unwrap();
    stage_system(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(1, 2)]),
        None,
        Some(&system),
    );
    assert_eq!(git(root, &["show", ":module.bsl"]), b"one\nTWO\n");
    fs::write(root.join("ignored"), b"new\n").unwrap();
    stage_system(
        root,
        "ignored",
        &request(root, "ignored", "worktree", &[(0, 1)]),
        Some("ignored"),
        Some(&system),
    );
}

#[cfg(unix)]
#[test]
fn new_paths_obey_git_filemode_preference() {
    use std::os::unix::fs::PermissionsExt;
    for (config, mode) in [("false", b"100644 "), ("true", b"100755 ")] {
        let fixture = fixture(b"base\n");
        let root = &fixture.0;
        git(root, &["config", "core.fileMode", config]);
        let path = root.join("new");
        fs::write(&path, b"one\ntwo\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        stage(
            root,
            "new",
            &request(root, "new", "worktree", &[(0, 1)]),
            None,
        );
        assert!(git(root, &["ls-files", "--stage", "--", "new"]).starts_with(mode));
    }
}

#[test]
fn intent_to_add_uses_empty_blob_as_selection_base() {
    let fixture = fixture(b"base\n");
    let root = &fixture.0;
    fs::write(root.join("new"), b"one\ntwo\nthree\n").unwrap();
    git(root, &["add", "--intent-to-add", "new"]);
    let mut selection = request(root, "new", "worktree", &[(1, 2)]);
    selection["index_sha256"] = json!(hash(&[]));
    stage(root, "new", &selection, None);
    assert_eq!(git(root, &["show", ":new"]), b"two\n");
    assert_eq!(fs::read(root.join("new")).unwrap(), b"one\ntwo\nthree\n");
}

#[test]
fn unchanged_oversize_and_binary_attributes_are_rejected() {
    let fixture = fixture(b"one\ntwo\n");
    let root = &fixture.0;
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("empty_selection"),
    );
    fs::write(root.join("module.bsl"), b"ONE\ntwo\n").unwrap();
    fs::write(root.join(".gitattributes"), b"*.bsl binary\n").unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("unsupported_text"),
    );
    fs::remove_file(root.join(".gitattributes")).unwrap();
    fs::write(root.join("module.bsl"), vec![b'a'; 8 * 1024 * 1024 + 1]).unwrap();
    stage(
        root,
        "module.bsl",
        &request(root, "module.bsl", "worktree", &[(0, 1)]),
        Some("oversize"),
    );
}

#[test]
fn file_directory_replacement_does_not_remove_unselected_index_entries() {
    let fixture = fixture(b"one\n");
    let root = &fixture.0;
    fs::remove_file(root.join("module.bsl")).unwrap();
    fs::create_dir(root.join("module.bsl")).unwrap();
    fs::write(root.join("module.bsl/child"), b"new\n").unwrap();
    let raw = fs::read(root.join(".git/index")).unwrap();
    stage(
        root,
        "module.bsl/child",
        &request(root, "module.bsl/child", "worktree", &[(0, 1)]),
        Some("path"),
    );
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), raw);
}

#[cfg(unix)]
#[test]
fn symlink_content_is_never_staged_as_lines() {
    let fixture = fixture(b"one\n");
    let root = &fixture.0;
    std::os::unix::fs::symlink("sibling", root.join("link")).unwrap();
    stage(
        root,
        "link",
        &request(root, "link", "worktree", &[(0, 1)]),
        Some("unsupported_text"),
    );
}
