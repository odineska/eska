//! Git's system-level EOL settings must apply to both SCM and monitor processes.
use eska_vcs::vcs::git::native_path;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};
#[path = "../../../tests/support/mod.rs"]
mod support;

struct Fixture {
    directory: support::TestDir,
    root: PathBuf,
    system: PathBuf,
    global: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = support::TestDir::new();
        let base = native_path(&directory.0);
        let root = base.join("repository");
        fs::create_dir(&root).unwrap();
        let system = base.join("system.gitconfig");
        let global = base.join("global.gitconfig");
        fs::write(&system, "[core]\n\tautocrlf = input\n").unwrap();
        fs::write(&global, "").unwrap();
        let fixture = Self {
            directory,
            root,
            system,
            global,
        };
        fixture.git(&["init", "-b", "main"]);
        fs::write(fixture.root.join("module.bsl"), b"first\nsecond\n").unwrap();
        fixture.git(&["add", "module.bsl"]);
        fixture.git(&["commit", "-m", "baseline"]);
        fixture.write(b"first\r\nsecond\r\n");
        fixture
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(&self.root)
            .env_remove("GIT_CONFIG_NOSYSTEM")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_CONFIG_COUNT", "0")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", &self.system)
            .env("GIT_CONFIG_GLOBAL", &self.global)
            .env("HOME", native_path(&self.directory.0).as_os_str())
            .env(
                "XDG_CONFIG_HOME",
                native_path(&self.directory.0).as_os_str(),
            )
            // The oracle must never refresh the index and hide a conversion bug.
            .env("GIT_OPTIONAL_LOCKS", "0");
        command
    }

    fn git(&self, args: &[&str]) -> Vec<u8> {
        let output = self
            .command("git")
            .args([
                "-c",
                "user.name=Test",
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
        Self::success(&output);
        output.stdout
    }

    fn success(output: &Output) {
        assert!(output.status.success(), "{output:?}");
    }

    fn write(&self, content: &[u8]) {
        fs::write(self.root.join("module.bsl"), content).unwrap();
        self.invalidate_stat();
    }

    fn invalidate_stat(&self) {
        // Zero cached stat fields force content inspection after SCM stage has
        // recorded CRLF worktree stat alongside the normalized LF blob.
        let blob = String::from_utf8(self.git(&["rev-parse", ":module.bsl"])).unwrap();
        self.git(&[
            "update-index",
            "--cacheinfo",
            "100644",
            blob.trim(),
            "module.bsl",
        ]);
    }

    fn scm_command(&self, locale: &str, args: &[&str]) -> Command {
        let mut command = self.command(env!("CARGO_BIN_EXE_eska-vcs"));
        command
            .args(["--lang", locale, "--project-dir"])
            .arg(&self.root)
            .arg("__bridge")
            .args(args);
        command
    }

    fn scm(&self, locale: &str, args: &[&str]) -> Value {
        let output = self.scm_command(locale, args).output().unwrap();
        Self::success(&output);
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value["data"].clone()
    }

    fn monitor(&self, locale: &str) -> Value {
        let output = self
            .command(env!("CARGO_BIN_EXE_eska-vcs"))
            .args(["--lang", locale, "--project-dir"])
            .arg(&self.root)
            .args(["monitor", "--events-stdin", "--format", "json"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Self::success(&output);
        assert!(output.stderr.is_empty(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn assert_status(&self, porcelain: &[u8], changes: &Value) {
        self.invalidate_stat();
        let index = fs::read(self.root.join(".git/index")).unwrap();
        assert_eq!(self.git(&["status", "--porcelain=v1", "-uno"]), porcelain);
        for locale in ["ru-RU", "en-US"] {
            let output = self
                .command(env!("CARGO_BIN_EXE_eska-vcs"))
                .args(["--lang", locale, "status", "--format", "json"])
                .output()
                .unwrap();
            Self::success(&output);
            let status: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                status["changes"]["files"],
                changes.as_array().unwrap().len()
            );
            assert_eq!(&self.scm(locale, &["scm-status"])["changes"], changes);
            assert_eq!(&self.monitor(locale)["changes"], changes);
        }
        assert_eq!(fs::read(self.root.join(".git/index")).unwrap(), index);
    }
}

#[test]
fn system_input_crlf_only_is_clean_and_a_stale_stage_leaves_the_index_identical() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.git(&["config", "--show-scope", "--get", "core.autocrlf"]),
        b"system\tinput\n"
    );
    fixture.assert_status(b"", &json!([]));
    let index = fs::read(fixture.root.join(".git/index")).unwrap();
    let output = fixture
        .scm_command("en-US", &["scm-files", "stage", "--", "module.bsl"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"],
        "stale_status"
    );
    assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    assert_eq!(fixture.git(&["show", ":module.bsl"]), b"first\nsecond\n");
    assert_eq!(
        fs::read(fixture.root.join("module.bsl")).unwrap(),
        b"first\r\nsecond\r\n"
    );
}

#[test]
fn system_input_keeps_real_crlf_edit_and_normalizes_scm_stage_blob() {
    let fixture = Fixture::new();
    fixture.write(b"first\r\nchanged\r\n");
    let expected = json!([{"path":"module.bsl", "path_encoding":"utf-8",
        "index":null, "worktree":"modified"}]);
    fixture.assert_status(b" M module.bsl\n", &expected);
    fixture.scm("ru-RU", &["scm-files", "stage", "--", "module.bsl"]);
    assert_eq!(fixture.git(&["show", ":module.bsl"]), b"first\nchanged\n");
    fixture.assert_status(
        b"M  module.bsl\n",
        &json!([{"path":"module.bsl", "path_encoding":"utf-8",
        "index":"modified", "worktree":null}]),
    );
}

#[test]
fn global_and_local_autocrlf_override_system_input() {
    let fixture = Fixture::new();
    let modified = json!([{"path":"module.bsl", "path_encoding":"utf-8",
        "index":null, "worktree":"modified"}]);
    fs::write(&fixture.global, "[core]\n\tautocrlf = false\n").unwrap();
    fixture.assert_status(b" M module.bsl\n", &modified);
    fixture.git(&["config", "--local", "core.autocrlf", "input"]);
    fixture.assert_status(b"", &json!([]));
    fs::write(&fixture.global, "[core]\n\tautocrlf = input\n").unwrap();
    fixture.git(&["config", "--local", "core.autocrlf", "false"]);
    fixture.assert_status(b" M module.bsl\n", &modified);
}

#[test]
fn branch_conditional_include_and_repository_attributes_follow_git() {
    let fixture = Fixture::new();
    fs::write(&fixture.system, "[core]\n\tautocrlf = false\n").unwrap();
    fs::write(
        &fixture.global,
        "[includeIf \"onbranch:main\"]\n\tpath = line-endings.gitconfig\n",
    )
    .unwrap();
    fs::write(
        fixture.system.with_file_name("line-endings.gitconfig"),
        "[core]\n\tautocrlf = input\n",
    )
    .unwrap();
    fixture.assert_status(b"", &json!([]));
    let info = fixture.root.join(".git/info");
    fs::create_dir_all(&info).unwrap();
    fs::write(info.join("attributes"), b"*.bsl -text\n").unwrap();
    fixture.assert_status(
        b" M module.bsl\n",
        &json!([{"path":"module.bsl","path_encoding":"utf-8","index":null,"worktree":"modified"}]),
    );
}

#[test]
fn monitor_reconciles_real_edit_back_to_system_input_crlf_clean() {
    struct Running(Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for locale in ["ru-RU", "en-US"] {
        let fixture = Fixture::new();
        assert_eq!(fixture.git(&["status", "--porcelain=v1", "-uno"]), b"");
        assert_eq!(fixture.monitor(locale)["changes"], json!([]));
        fixture.write(b"first\r\nchanged\r\n");
        let index = fs::read(fixture.root.join(".git/index")).unwrap();
        let mut process = Running(
            fixture
                .command(env!("CARGO_BIN_EXE_eska-vcs"))
                .args(["--lang", locale, "--project-dir"])
                .arg(&fixture.root)
                .args(["monitor", "--events-stdin", "--format", "json"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = process.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender
                    .send(serde_json::from_str::<Value>(&line.unwrap()).unwrap())
                    .is_err()
                {
                    break;
                }
            }
        });
        let first = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first["changes"][0]["worktree"], "modified");
        // This changes both content and size, invalidating the monitor's own
        // stat cache without touching repository metadata during observation.
        fs::write(fixture.root.join("module.bsl"), b"first\r\nsecond\r\n").unwrap();
        assert_eq!(fixture.git(&["status", "--porcelain=v1", "-uno"]), b"");
        let input = process.0.stdin.as_mut().unwrap();
        writeln!(
            input,
            "{}",
            json!({"schema_version":1,"paths":["module.bsl"]})
        )
        .unwrap();
        input.flush().unwrap();
        let clean = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(clean["changes"], json!([]));
        // A scoped hint after an EOL policy change has to reclassify other paths too.
        fs::write(&fixture.system, "[core]\n\tautocrlf = false\n").unwrap();
        writeln!(
            input,
            "{}",
            json!({"schema_version":1,"paths":["unrelated"]})
        )
        .unwrap();
        input.flush().unwrap();
        let changed = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(changed["changes"][0]["path"], "module.bsl");
        assert_eq!(changed["changes"][0]["worktree"], "modified");
        assert_eq!(fs::read(fixture.root.join(".git/index")).unwrap(), index);
    }
}
