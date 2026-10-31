//! Distinct CLI processes share verified derivations and retain separate mutable databases.

use super::*;
use std::time::{Duration, Instant};

pub(super) fn namespaces(config: &Path) -> Vec<PathBuf> {
    fs::read_dir(config.join("cache/libraries"))
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.path())
        .collect()
}

struct Fixture {
    owned: support::TestDir,
    library: Project,
    config: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let owned = support::TestDir::new();
        let library = project(&owned.0.join("library"), ProjectType::Configuration);
        fs::write(library.source().join("Payload.bsl"), "one").unwrap();
        let config = catalog(&owned.0.join("machine"), &[("ssl_3_1", library.source())]);
        let log = owned.0.join("ibcmd.log");
        Self {
            owned,
            library,
            config,
            log,
        }
    }

    fn consumer(&self, name: &str, kind: ProjectType) -> Project {
        let result = project(&self.owned.0.join(name), kind);
        declare(&result, "library='ssl_3_1'");
        fs::write(result.source().join("Payload.bsl"), name).unwrap();
        result
    }

    fn command(&self, project: &Project) -> Command {
        let mut result = builder(project.root(), &self.config, "en");
        result
            .env("FAKE_IBCMD_LOG", &self.log)
            .env("FAKE_IBCMD_ARTIFACT_SOURCE_FILE", "Payload.bsl")
            .args(["build", "--format", "json"]);
        result
    }

    fn run(&self, project: &Project, extra: &[&str]) -> Value {
        document(&self.command(project).args(extra).output().unwrap(), true)
    }

    fn assert_calls(&self, cf_imports: usize, cf_loads: usize) {
        let log = fs::read_to_string(&self.log).unwrap();
        assert_eq!(
            log.lines()
                .filter(|line| line.starts_with("infobase config import ")
                    && line.contains("base-source"))
                .count(),
            cf_imports,
            "{log}"
        );
        assert_eq!(
            log.lines()
                .filter(|line| line.starts_with("infobase create ") && line.contains("--load="))
                .count(),
            cf_loads,
            "{log}"
        );
    }

    fn cache(&self) -> PathBuf {
        let roots = namespaces(&self.config);
        assert_eq!(roots.len(), 1);
        roots[0].clone()
    }
}

fn template(cache: &Path) -> PathBuf {
    cache.join("template")
}

#[test]
fn ordinary_library_build_serves_independent_consumers_without_a_manifest_flag() {
    let fixture = Fixture::new();
    fixture.run(&fixture.library, &[]);
    fixture.assert_calls(1, 0);
    let library_plan = BuildPlan::new(&fixture.library, None).unwrap();
    assert!(!eska_build_runtime::build::manifest_path(library_plan.output()).exists());
    let cache = fixture.cache();
    assert_eq!(fs::read(cache.join("base.cf")).unwrap(), b"one");
    for (name, kind) in [
        ("report", ProjectType::Report),
        ("processing", ProjectType::Processing),
    ] {
        let consumer = fixture.consumer(name, kind);
        fixture.run(&consumer, &["--manifest"]);
        let plan = BuildPlan::new(&consumer, None).unwrap();
        assert_eq!(
            fs::read(plan.infobase_root().join("data/loaded.cf")).unwrap(),
            b"one"
        );
        assert!(!plan.infobase_root().join("base.cf").exists());
        assert!(!plan.infobase_root().join("data/private").exists());
        fs::write(plan.infobase_root().join("data/private"), name).unwrap();
        let manifest: Value = serde_json::from_slice(
            &fs::read(eska_build_runtime::build::manifest_path(plan.output())).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["source"]["base_context"]["library"], "ssl_3_1");
        fixture.run(&consumer, &[]);
        assert_eq!(
            fs::read(plan.infobase_root().join("data/private")).unwrap(),
            name.as_bytes()
        );
    }
    fixture.run(&fixture.library, &["--manifest"]);
    let manifest: Value = serde_json::from_slice(
        &fs::read(eska_build_runtime::build::manifest_path(
            library_plan.output(),
        ))
        .unwrap(),
    )
    .unwrap();
    assert!(manifest["source"].get("base_context").is_none());
    fixture.assert_calls(1, 1);
    assert!(!template(&cache).join("data/private").exists());
}

#[test]
fn consumer_miss_populates_shared_results_and_clean_or_recreate_preserves_them() {
    let fixture = Fixture::new();
    let first = fixture.consumer("first", ProjectType::Report);
    let second = fixture.consumer("second", ProjectType::Processing);
    fixture.run(&first, &[]);
    fixture.run(&second, &[]);
    fixture.assert_calls(1, 1);
    let plan = BuildPlan::new(&first, None).unwrap();
    let marker = plan.infobase_root().join("data/private");
    fs::write(&marker, "private").unwrap();
    fixture.run(&first, &["--recreate-infobase"]);
    assert!(!marker.exists());
    let second_plan = BuildPlan::new(&second, None).unwrap();
    let second_marker = second_plan.infobase_root().join("data/private");
    fs::write(&second_marker, "second").unwrap();
    eska_build_runtime::build::clean_infobase(plan.infobase_root(), first.root()).unwrap();
    assert!(fixture.cache().join("base.cf").is_file());
    fixture.run(&first, &[]);
    assert!(second_marker.is_file());
    fixture.assert_calls(1, 1);
    fs::remove_dir_all(fixture.cache()).unwrap();
    fixture.run(&first, &["--recreate-infobase"]);
    fixture.assert_calls(2, 2);
}

#[test]
fn damaged_cf_and_template_are_rebuilt_without_adopting_unowned_directories() {
    let fixture = Fixture::new();
    let first = fixture.consumer("first", ProjectType::Report);
    fixture.run(&first, &[]);
    let cache = fixture.cache();
    fs::write(template(&cache).join("data/loaded.cf"), "corruption").unwrap();
    fixture.run(&first, &["--recreate-infobase"]);
    fixture.assert_calls(1, 2);
    fs::remove_file(template(&cache).join("state.json")).unwrap();
    fixture.run(&first, &["--recreate-infobase"]);
    fixture.assert_calls(1, 3);
    for remove in [false, true] {
        let cf = cache.join("base.cf");
        if remove {
            fs::remove_file(cf).unwrap();
        } else {
            fs::write(cf, "bad").unwrap();
        }
        fixture.run(&first, &[]);
    }
    fixture.assert_calls(3, 3);
    let plan = BuildPlan::new(&first, None).unwrap();
    let before = fs::read(plan.output()).unwrap();
    fs::remove_file(template(&cache).join("owner")).unwrap();
    let failed = document(
        &fixture
            .command(&first)
            .arg("--recreate-infobase")
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(failed["error"]["code"], "library-cache-unowned");
    assert_eq!(failed["error"]["stage"], "prepare-base-configuration");
    assert_eq!(fs::read(plan.output()).unwrap(), before);
    assert!(template(&cache).join("data/loaded.cf").exists());
}

#[test]
fn source_bytes_invalidate_shared_cf_and_immutable_template_with_unchanged_mtime() {
    let fixture = Fixture::new();
    let first = fixture.consumer("first", ProjectType::Report);
    let second = fixture.consumer("second", ProjectType::Processing);
    fixture.run(&first, &[]);
    let path = fixture.library.source().join("Payload.bsl");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "two").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    fixture.run(&second, &["--manifest"]);
    fixture.run(&first, &[]);
    fixture.assert_calls(2, 2);
    for consumer in [&first, &second] {
        let plan = BuildPlan::new(consumer, None).unwrap();
        assert_eq!(
            fs::read(plan.infobase_root().join("data/loaded.cf")).unwrap(),
            b"two"
        );
    }
}

#[test]
fn dry_run_creates_no_shared_state_and_cache_overlap_is_rejected_before_writes() {
    let fixture = Fixture::new();
    let consumer = fixture.consumer("report", ProjectType::Report);
    for project in [&fixture.library, &consumer] {
        fixture.run(project, &["--dry-run"]);
        assert!(!project.root().join("artifacts").exists());
    }
    assert!(!fixture.config.join("cache").exists());
    let config = catalog(
        &fixture.library.source().join("machine"),
        &[("ssl_3_1", fixture.library.source())],
    );
    let output = builder(consumer.root(), &config, "en")
        .args(["build", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        document(&output, false)["error"]["code"],
        "base-configuration-output-overlap"
    );
    assert!(!config.join("cache").exists());
    assert!(!consumer.root().join("artifacts").exists());
}

struct Child(Option<std::process::Child>);

impl Child {
    fn start(command: &mut Command) -> Self {
        Self(Some(
            command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn finish(&mut self) {
        let output = self.0.take().unwrap().wait_with_output().unwrap();
        document(&output, true);
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn parallel_processes_prepare_one_cf_and_one_template() {
    let fixture = Fixture::new();
    let first = fixture.consumer("first", ProjectType::Report);
    let second = fixture.consumer("second", ProjectType::Processing);
    let ready = fixture.owned.0.join("ready");
    let proceed = fixture.owned.0.join("continue");
    let mut first = Child::start(
        fixture
            .command(&first)
            .env("FAKE_IBCMD_IMPORT_READY", &ready)
            .env("FAKE_IBCMD_IMPORT_CONTINUE", &proceed),
    );
    let started = Instant::now();
    while !ready.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "first CF import never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let second_root = BuildPlan::new(&second, None)
        .unwrap()
        .infobase_root()
        .to_owned();
    let mut second = Child::start(&mut fixture.command(&second));
    while !second_root.join("owner").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "second consumer never acquired its lease"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::write(proceed, "continue").unwrap();
    first.finish();
    second.finish();
    fixture.assert_calls(1, 1);
}

#[test]
fn unowned_namespace_has_locale_independent_errors_and_keeps_previous_results() {
    let fixture = Fixture::new();
    let consumer = fixture.consumer("report", ProjectType::Report);
    fixture.run(&consumer, &["--manifest"]);
    let plan = BuildPlan::new(&consumer, None).unwrap();
    let artifact = fs::read(plan.output()).unwrap();
    let passport = fs::read(eska_build_runtime::build::manifest_path(plan.output())).unwrap();
    let cache = fixture.cache();
    fs::remove_file(cache.join("owner")).unwrap();
    let before = fs::read(cache.join("base.cf")).unwrap();
    let mut documents = Vec::new();
    for locale in ["ru", "en"] {
        let output = builder(consumer.root(), &fixture.config, locale)
            .args(["build", "--format", "json"])
            .output()
            .unwrap();
        let failed = document(&output, false);
        assert_eq!(failed["error"]["code"], "library-cache-unowned");
        assert_eq!(failed["error"]["stage"], "prepare-base-configuration");
        assert!(String::from_utf8_lossy(&output.stderr).contains(cache.to_str().unwrap()));
        documents.push(failed);
    }
    assert_eq!(documents[0], documents[1]);
    assert_eq!(fs::read(plan.output()).unwrap(), artifact);
    assert_eq!(
        fs::read(eska_build_runtime::build::manifest_path(plan.output())).unwrap(),
        passport
    );
    assert_eq!(fs::read(cache.join("base.cf")).unwrap(), before);
}

#[test]
fn root_layout_library_excludes_project_state_and_build_outputs_from_shared_inputs() {
    let mut fixture = Fixture::new();
    let root = fixture.library.root().to_owned();
    for entry in fs::read_dir(fixture.library.source()).unwrap() {
        let entry = entry.unwrap();
        fs::rename(entry.path(), root.join(entry.file_name())).unwrap();
    }
    fs::remove_dir(fixture.library.source()).unwrap();
    let config = ProjectConfig::load(&root.join("eska.toml"))
        .unwrap()
        .with_source(".".into())
        .unwrap();
    fs::write(root.join("eska.toml"), config.to_toml().unwrap()).unwrap();
    fixture.library = config.into_project(root.clone()).unwrap();
    catalog(&fixture.config, &[("ssl_3_1", fixture.library.source())]);
    fixture.run(&fixture.library, &[]);
    fixture.run(&fixture.library, &["--manifest"]);
    let consumer = fixture.consumer("report", ProjectType::Report);
    fixture.run(&consumer, &[]);
    fixture.assert_calls(1, 1);
    let bad_output = root.join("nested/result.cf");
    let failed = document(
        &fixture
            .command(&fixture.library)
            .arg("--output")
            .arg(&bad_output)
            .output()
            .unwrap(),
        false,
    );
    assert_eq!(failed["error"]["code"], "base-configuration-output-overlap");
    assert!(!bad_output.parent().unwrap().exists());
}

#[test]
fn named_library_is_never_made_writable_and_staging_copies_are_removed() {
    let fixture = Fixture::new();
    let consumer = fixture.consumer("report", ProjectType::Report);
    let descriptor = fixture.library.source().join("Configuration.xml");
    let original = fs::read(&descriptor).unwrap();
    let permissions = fs::metadata(&descriptor).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&descriptor, readonly).unwrap();
    let output = fixture.command(&consumer).output().unwrap();
    let still_readonly = fs::metadata(&descriptor).unwrap().permissions().readonly();
    fs::set_permissions(&descriptor, permissions).unwrap();
    document(&output, true);
    assert!(still_readonly);
    assert_eq!(fs::read(descriptor).unwrap(), original);
    assert!(
        !fs::read_dir(fixture.cache())
            .unwrap()
            .map(Result::unwrap)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("build.eska-work-"))
    );
    assert!(!fixture.library.root().join("artifacts").exists());
}

#[cfg(unix)]
#[test]
fn cancellation_while_waiting_for_shared_lease_preserves_results_and_consumer_data() {
    let fixture = Fixture::new();
    let consumer = fixture.consumer("report", ProjectType::Report);
    fixture.run(&consumer, &["--manifest"]);
    let plan = BuildPlan::new(&consumer, None).unwrap();
    let artifact = fs::read(plan.output()).unwrap();
    let passport_path = eska_build_runtime::build::manifest_path(plan.output());
    let passport = fs::read(&passport_path).unwrap();
    let marker = plan.infobase_root().join("data/private");
    fs::write(&marker, "keep").unwrap();
    let cache_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.cache().with_extension("lock"))
        .unwrap();
    cache_lock.try_lock().unwrap();
    let mut child = Child::start(fixture.command(&consumer).arg("--recreate-infobase"));
    let consumer_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(plan.infobase_root().with_extension("erf.lock"))
        .unwrap();
    let started = Instant::now();
    loop {
        match consumer_lock.try_lock() {
            Err(fs::TryLockError::WouldBlock) => break,
            Ok(()) => consumer_lock.unlock().unwrap(),
            Err(error) => panic!("consumer lease: {error}"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "consumer never entered the build pipeline"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        Command::new("kill")
            .arg("-INT")
            .arg(child.0.as_ref().unwrap().id().to_string())
            .status()
            .unwrap()
            .success()
    );
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    let failed = document(&output, false);
    assert_eq!(failed["error"]["code"], "interrupted");
    assert_eq!(failed["error"]["stage"], "prepare-base-configuration");
    assert_eq!(fs::read(plan.output()).unwrap(), artifact);
    assert_eq!(fs::read(passport_path).unwrap(), passport);
    assert_eq!(fs::read(marker).unwrap(), b"keep");
    fixture.assert_calls(1, 1);
    assert!(
        !fs::read_dir(plan.output().parent().unwrap())
            .unwrap()
            .map(Result::unwrap)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("build.eska-work-"))
    );
}
