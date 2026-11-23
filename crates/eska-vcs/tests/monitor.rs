//! Native events through the standalone NDJSON stream; no synthetic invalidations.
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
#[path = "../../../tests/support/mod.rs"]
mod support;
use support::TestDir;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.join("absent"))
        .args([
            "-c",
            "user.name=Eska Test",
            "-c",
            "user.email=eska@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
struct Stream {
    child: Child,
    lines: Receiver<Value>,
}
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Stream {
    fn start(root: &Path, locale: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_eska-vcs"))
            .args(["--lang", locale, "--project-dir"])
            .arg(root)
            .args(["monitor", "--format", "json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }
    fn next(&self) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(5))
            .expect("native snapshot within five seconds")
    }
    fn quiet(&self) {
        assert!(
            self.lines.recv_timeout(Duration::from_millis(180)).is_err(),
            "unchanged status must be silent"
        );
    }
}

#[test]
fn native_snapshots_reconcile_literal_paths_index_head_and_ignores_in_both_locales() {
    let root = TestDir::new();
    git(&root.0, &["init", "-b", "main"]);
    fs::write(root.0.join("tracked"), "base\n").unwrap();
    fs::write(root.0.join("other"), "base\n").unwrap();
    git(&root.0, &["add", "."]);
    git(&root.0, &["commit", "-m", "baseline"]);
    let mut baselines = Vec::new();
    for locale in ["ru-RU", "en-US"] {
        let start = Instant::now();
        let stream = Stream::start(&root.0, locale);
        let baseline = stream.next();
        eprintln!("monitor setup {locale}: {:?}", start.elapsed());
        assert_eq!(baseline["repository"]["head"], "attached");
        assert_eq!(baseline["repository"]["branch"], "main");
        assert_eq!(baseline["changes"], serde_json::json!([]));
        baselines.push(baseline);
        // Scoped updates must not drop a staged change outside their literal pathspec.
        fs::write(root.0.join("other"), "staged\n").unwrap();
        git(&root.0, &["add", "other"]);
        assert_eq!(stream.next()["changes"][0]["index"], "modified");
        let changed = Instant::now();
        fs::write(root.0.join("[literal]*"), "new\n").unwrap();
        let snapshot = stream.next();
        eprintln!("monitor native file {locale}: {:?}", changed.elapsed());
        assert_eq!(snapshot["changes"].as_array().unwrap().len(), 2);
        assert_eq!(snapshot["changes"][0]["path"], "[literal]*");
        fs::write(root.0.join("[literal]*"), "different\n").unwrap();
        stream.quiet();
        fs::remove_file(root.0.join("[literal]*")).unwrap();
        assert_eq!(stream.next()["changes"].as_array().unwrap().len(), 1);
        fs::write(root.0.join(".gitignore"), "ignored\n").unwrap();
        assert_eq!(stream.next()["changes"].as_array().unwrap().len(), 2);
        fs::write(root.0.join("ignored"), "ignored\n").unwrap();
        stream.quiet();
        fs::remove_file(root.0.join(".gitignore")).unwrap();
        let snapshot = stream.next();
        assert!(
            snapshot["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "ignored")
        );
        fs::remove_file(root.0.join("ignored")).unwrap();
        stream.next();
        git(&root.0, &["restore", "--staged", "--worktree", "other"]);
        assert_eq!(stream.next()["changes"], serde_json::json!([]));
        git(&root.0, &["checkout", "--detach"]);
        assert_eq!(stream.next()["repository"]["head"], "detached");
        git(&root.0, &["checkout", "main"]);
        assert_eq!(stream.next()["repository"]["head"], "attached");
    }
    assert_eq!(baselines[0], baselines[1]);
}

#[test]
fn unborn_and_linked_worktree_roots_need_no_manifest() {
    let root = TestDir::new();
    git(&root.0, &["init", "-b", "main"]);
    let stream = Stream::start(&root.0, "en-US");
    let initial = stream.next();
    assert_eq!(initial["repository"]["head"], "unborn");
    fs::write(root.0.join("tracked"), "base\n").unwrap();
    assert_eq!(stream.next()["changes"][0]["worktree"], "untracked");
    git(&root.0, &["add", "."]);
    git(&root.0, &["commit", "-m", "baseline"]);
    // Metadata may produce intermediate staged/unborn snapshots before the final HEAD event.
    loop {
        let value = stream.next();
        if value["repository"]["head"] == "attached" {
            break;
        }
    }
    let linked = root.0.join("linked");
    git(
        &root.0,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let linked_stream = Stream::start(&linked, "en-US");
    let snapshot = linked_stream.next();
    assert_eq!(snapshot["repository"]["root"], linked.to_str().unwrap());
    assert_eq!(snapshot["repository"]["branch"], "linked");
    fs::write(linked.join("tracked"), "changed\n").unwrap();
    assert_eq!(linked_stream.next()["changes"][0]["worktree"], "modified");
}

#[test]
fn terminal_errors_are_ndjson_and_localized_only_on_stderr() {
    let root = TestDir::new();
    let mut errors = Vec::new();
    let mut diagnostics = Vec::new();
    for locale in ["ru-RU", "en-US"] {
        let output = Command::new(env!("CARGO_BIN_EXE_eska-vcs"))
            .args(["--lang", locale, "--project-dir"])
            .arg(&root.0)
            .args(["monitor", "--format", "json"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        errors.push(serde_json::from_slice::<Value>(&output.stdout).unwrap());
        diagnostics.push(output.stderr);
    }
    assert_eq!(
        errors[0],
        serde_json::json!({"schema_version":1,"kind":"error","code":"repository_not_found"})
    );
    assert_eq!(errors[0], errors[1]);
    assert_ne!(diagnostics[0], diagnostics[1]);
    assert_ne!(diagnostics[0], [] as [u8; 0]);
}

#[test]
fn directory_removal_and_rename_reconcile_descendants_without_index_writes() {
    let root = TestDir::new();
    git(&root.0, &["init", "-b", "main"]);
    fs::create_dir(root.0.join("dir")).unwrap();
    fs::write(root.0.join("dir/child"), "base\n").unwrap();
    git(&root.0, &["add", "."]);
    git(&root.0, &["commit", "-m", "baseline"]);
    let index = fs::read(root.0.join(".git/index")).unwrap();
    let stream = Stream::start(&root.0, "en-US");
    stream.next();
    fs::rename(root.0.join("dir"), root.0.join("moved")).unwrap();
    // Both endpoints can arrive in distinct native batches.
    let mut snapshot = stream.next();
    while snapshot["changes"]
        .as_array()
        .unwrap_or_else(|| panic!("{snapshot:?}"))
        .len()
        != 2
    {
        snapshot = stream.next();
    }
    assert_eq!(snapshot["changes"][0]["path"], "dir/child");
    assert_eq!(snapshot["changes"][0]["worktree"], "deleted");
    assert_eq!(snapshot["changes"][1]["path"], "moved/child");
    fs::remove_dir_all(root.0.join("moved")).unwrap();
    assert_eq!(stream.next()["changes"].as_array().unwrap().len(), 1);
    drop(stream);
    assert_eq!(fs::read(root.0.join(".git/index")).unwrap(), index);
}

#[cfg(unix)]
#[test]
fn native_non_utf8_paths_keep_reversible_encoding() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let root = TestDir::new();
    git(&root.0, &["init", "-b", "main"]);
    let stream = Stream::start(&root.0, "en-US");
    stream.next();
    fs::write(root.0.join(OsStr::from_bytes(b"raw-\xFF")), "new\n").unwrap();
    let snapshot = stream.next();
    assert_eq!(snapshot["changes"][0]["path"], "%72%61%77%2D%FF");
    assert_eq!(snapshot["changes"][0]["path_encoding"], "percent");
}
