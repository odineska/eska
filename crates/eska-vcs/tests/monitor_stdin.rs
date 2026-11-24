//! Portable input transport, authoritative reconciliation and metadata-only probes.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
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
            "user.email=test@example.invalid",
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
    input: Option<ChildStdin>,
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
            .args(["monitor", "--events-stdin", "--format", "json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender
                    .send(serde_json::from_str(&line.unwrap()).unwrap())
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            lines,
        }
    }
    fn send(&mut self, value: &Value) {
        writeln!(self.input.as_mut().unwrap(), "{value}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn paths(&mut self, paths: &[&str]) {
        self.send(&json!({"schema_version":1,"paths":paths}));
    }
    fn next(&self) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(5))
            .expect("snapshot within five seconds")
    }
    fn quiet(&self, duration: Duration) {
        assert!(matches!(
            self.lines.recv_timeout(duration),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    }
    fn close(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn entry<'a>(snapshot: &'a Value, path: &str) -> Option<&'a Value> {
    snapshot["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["path"] == path)
}
fn fixture() -> TestDir {
    let root = TestDir::new();
    git(&root.0, &["init", "-b", "main"]);
    fs::write(root.0.join("old"), "base\n").unwrap();
    fs::write(root.0.join("tracked"), "base\n").unwrap();
    git(&root.0, &["add", "."]);
    git(&root.0, &["commit", "-m", "base"]);
    root
}

#[test]
fn stdin_snapshots_reconcile_renames_deletions_and_ignores_in_both_languages() {
    for locale in ["ru-RU", "en-US"] {
        let root = fixture();
        let index = fs::read(root.0.join(".git/index")).unwrap();
        let mut stream = Stream::start(&root.0, locale);
        let initial = stream.next();
        assert_eq!(initial["schema_version"], 1);
        assert_eq!(initial["kind"], "snapshot");
        assert_eq!(initial["changes"], json!([]));
        fs::rename(root.0.join("old"), root.0.join("new")).unwrap();
        stream.paths(&["old", "new"]);
        let renamed = stream.next();
        assert_eq!(entry(&renamed, "old").unwrap()["worktree"], "deleted");
        assert_eq!(entry(&renamed, "new").unwrap()["worktree"], "untracked");
        fs::remove_file(root.0.join("new")).unwrap();
        stream.paths(&["new"]);
        assert!(entry(&stream.next(), "new").is_none());
        fs::write(root.0.join("hidden"), "ignored soon").unwrap();
        stream.paths(&["hidden"]);
        assert!(entry(&stream.next(), "hidden").is_some());
        fs::write(root.0.join(".gitignore"), "hidden\n").unwrap();
        stream.paths(&[".gitignore"]);
        let ignored = stream.next();
        assert!(entry(&ignored, "hidden").is_none());
        assert!(entry(&ignored, ".gitignore").is_some());
        fs::write(root.0.join("tracked"), "changed\n").unwrap();
        stream.paths(&["tracked"]);
        let changed = stream.next();
        assert_eq!(entry(&changed, "tracked").unwrap()["worktree"], "modified");
        stream.send(&json!({"schema_version":1,"refresh":true}));
        stream.quiet(Duration::from_millis(1200));
        assert_eq!(fs::read(root.0.join(".git/index")).unwrap(), index);
        stream.close();
    }
}

#[test]
fn linked_worktree_probes_index_and_head_without_input_or_idle_status_scans() {
    let root = fixture();
    let linked = root.0.join("linked");
    // Git for Windows does not accept canonical verbatim paths as worktree targets.
    git(&root.0, &["worktree", "add", "-b", "linked", "linked"]);
    let mut stream = Stream::start(&linked, "en-US");
    let initial = stream.next();
    assert_eq!(initial["repository"]["branch"], "linked");
    assert_eq!(
        fs::canonicalize(Path::new(initial["repository"]["root"].as_str().unwrap())).unwrap(),
        fs::canonicalize(&linked).unwrap()
    );
    // A worktree-only edit must remain quiet: the idle probe reads metadata, not status.
    fs::write(linked.join("tracked"), "staged externally\n").unwrap();
    stream.quiet(Duration::from_millis(1300));
    git(&linked, &["add", "tracked"]);
    let staged = stream.next();
    assert_eq!(entry(&staged, "tracked").unwrap()["index"], "modified");
    assert!(entry(&staged, "tracked").unwrap()["worktree"].is_null());
    git(&linked, &["switch", "-c", "renamed"]);
    assert_eq!(stream.next()["repository"]["branch"], "renamed");
    git(&linked, &["pack-refs", "--all"]);
    git(&linked, &["switch", "--detach"]);
    assert_eq!(stream.next()["repository"]["head"], "detached");
    stream.close();
}

#[test]
fn metadata_probe_is_not_starved_by_continuous_input() {
    let root = fixture();
    let mut stream = Stream::start(&root.0, "en-US");
    stream.next();
    fs::write(root.0.join("tracked"), "continuous\n").unwrap();
    git(&root.0, &["add", "tracked"]);
    let mut input = stream.input.take().unwrap();
    let sender = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            if writeln!(input, "{{\"schema_version\":1,\"paths\":[\"absent\"]}}").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        input
    });
    assert_eq!(
        entry(&stream.next(), "tracked").unwrap()["index"],
        "modified"
    );
    stream.input = Some(sender.join().unwrap());
    stream.close();
}

#[test]
fn metadata_probe_reports_an_invalid_index_without_input() {
    let root = fixture();
    let mut stream = Stream::start(&root.0, "en-US");
    stream.next();
    fs::write(root.0.join(".git/index"), b"broken").unwrap();
    let error = stream.next();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["code"], "invalid_index");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = stream.child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(1));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn exact_rename_details_exist_initially_and_after_index_event() {
    let fixture = TestDir::new();
    let root = &fixture.0;
    git(root, &["init"]);
    fs::write(root.join("before"), "same\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    fs::rename(root.join("before"), root.join("after")).unwrap();
    git(root, &["add", "--all"]);
    let mut stream = Stream::start(root, "ru");
    let initial = stream.next();
    assert_eq!(
        initial["renames"],
        json!([{"stage":"index","from":"before","from_encoding":"utf-8","to":"after","to_encoding":"utf-8"}])
    );
    assert_eq!(initial["conflicts"], json!([]));
    git(
        root,
        &[
            "restore",
            "--staged",
            "--source=HEAD",
            "--",
            "before",
            "after",
        ],
    );
    stream.send(&json!({"schema_version":1,"full":true}));
    assert_eq!(stream.next()["renames"], json!([]));
    stream.close();
}
