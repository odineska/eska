//! Exercise declared base inputs through the standalone CLI process and stable JSON.

use super::*;
use eska_build_runtime::build::BuildPlan;
use std::fmt::Write as _;

mod shared;

fn catalog(root: &Path, entries: &[(&str, &Path)]) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let mut text = "[build]\nrunner='host'\n".to_owned();
    for (name, source) in entries {
        write!(
            text,
            "[libraries.{name}]\nsource={}\n",
            serde_json::to_string(source.to_str().unwrap()).unwrap()
        )
        .unwrap();
    }
    fs::write(root.join("config.toml"), text).unwrap();
    root.to_owned()
}

fn declare(project: &Project, declaration: &str) {
    let path = project.root().join("eska.toml");
    fs::write(
        &path,
        format!(
            "{}\n[analysis.configuration]\n{declaration}\n",
            fs::read_to_string(&path).unwrap()
        ),
    )
    .unwrap();
}

fn builder(root: &Path, catalog: &Path, locale: &str) -> Command {
    let mut result = command(root, catalog, locale);
    result.env("ESKA_IBCMD", ibcmd_fixture::executable(&repository()));
    result
}

#[test]
fn missing_platform_points_to_persistent_configuration_in_both_languages() {
    let fixture = support::TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
    let manifest = consumer.root().join("eska.toml");
    fs::write(&manifest, "[project]\ntype='processing'\n").unwrap();
    let mut errors = Vec::new();
    for locale in ["ru", "en"] {
        let output = builder(consumer.root(), &fixture.0.join("machine"), locale)
            .args(["build", "--format", "json"])
            .output()
            .unwrap();
        let error = document(&output, false);
        assert_eq!(error["error"]["code"], "platform-version-missing");
        assert_eq!(error["error"]["stage"], "plan");
        assert_eq!(
            error["error"]["data"]["file"]["value"],
            manifest.to_str().unwrap()
        );
        assert_eq!(error["error"]["data"]["field"], "build.platform_version");
        assert!(String::from_utf8_lossy(&output.stderr).contains(manifest.to_str().unwrap()));
        errors.push(error);
    }
    assert_eq!(errors[0], errors[1]);
}

#[test]
fn configured_standalone_builds_resolve_named_and_direct_xml_without_extra_flags() {
    let fixture = support::TestDir::new();
    let library = project(
        &fixture.0.join("Библиотека с пробелами"),
        ProjectType::Configuration,
    );
    fs::remove_file(library.root().join("eska.toml")).unwrap();
    let catalog = catalog(&fixture.0.join("machine"), &[("ssl_3_1", library.source())]);
    let original = fs::read(library.source().join("Configuration.xml")).unwrap();
    for kind in [ProjectType::Processing, ProjectType::Report] {
        for named in [true, false] {
            let consumer = project(&fixture.0.join(format!("{}-{named}", kind.as_str())), kind);
            let declaration = if named {
                "library='ssl_3_1'".to_owned()
            } else {
                format!(
                    "source={}",
                    serde_json::to_string(library.source().to_str().unwrap()).unwrap()
                )
            };
            declare(&consumer, &declaration);
            let mut previews = Vec::new();
            for locale in ["ru", "en"] {
                let preview = document(
                    &builder(consumer.root(), &catalog, locale)
                        .args(["build", "--dry-run", "--format", "json"])
                        .output()
                        .unwrap(),
                    true,
                );
                let base = &preview["projects"][0]["base_configuration"];
                assert_eq!(base["kind"], "designer-xml");
                assert_eq!(base["source"]["path"], library.source().to_str().unwrap());
                assert_eq!(base.get("library").is_some(), named);
                assert_eq!(
                    base["steps"],
                    serde_json::json!(["prepare-base-configuration", "load-base-cf"])
                );
                previews.push(preview);
            }
            assert_eq!(previews[0], previews[1]);
            assert!(!consumer.root().join("artifacts").exists());
            let output = builder(consumer.root(), &catalog, "ru")
                .arg("build")
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            let plan = BuildPlan::new(&consumer, None).unwrap();
            assert!(plan.output().is_file());
            let result = builder(consumer.root(), &catalog, "en")
                .args(["build", "--manifest", "--format", "json"])
                .output()
                .unwrap();
            document(&result, true);
            let manifest: Value = serde_json::from_slice(
                &fs::read(eska_build_runtime::build::manifest_path(plan.output())).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest["source"]["base_context"]["kind"], "designer-xml");
            assert_eq!(
                manifest["source"]["base_context"].get("library").is_some(),
                named
            );
        }
    }
    assert_eq!(
        fs::read(library.source().join("Configuration.xml")).unwrap(),
        original
    );
    assert!(!library.root().join("eska.toml").exists());
}

#[test]
fn configured_base_refreshes_on_content_path_and_platform_and_preserves_results_on_failure() {
    let fixture = support::TestDir::new();
    let library = project(&fixture.0.join("base"), ProjectType::Configuration);
    let other = project(&fixture.0.join("other"), ProjectType::Configuration);
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    declare(&consumer, "library='ssl_3_1'");
    let catalog = catalog(&fixture.0.join("machine"), &[("ssl_3_1", library.source())]);
    let plan = BuildPlan::new(&consumer, None).unwrap();
    for source in [library.source(), other.source(), consumer.source()] {
        fs::write(source.join("Payload.bsl"), "one").unwrap();
    }
    let run = |extra: &[&str], version: &str, fail: bool| {
        let mut command = builder(consumer.root(), &catalog, "en");
        command
            .env("FAKE_IBCMD_ARTIFACT_SOURCE_FILE", "Payload.bsl")
            .env("FAKE_IBCMD_VERSION", version)
            .args(["build", "--format", "json"])
            .args(extra);
        if fail {
            command.env("FAKE_IBCMD_FAIL_SOURCE_CONTAINS", "base-source");
        }
        command.output().unwrap()
    };
    document(&run(&[], "8.3.27.2325", false), true);
    let marker = plan.infobase_root().join("data/retained");
    fs::write(&marker, "sentinel").unwrap();
    document(&run(&[], "8.3.27.2325", false), true);
    assert!(marker.exists());
    let module = library.source().join("Payload.bsl");
    let modified = fs::metadata(&module).unwrap().modified().unwrap();
    fs::write(&module, "two").unwrap();
    fs::File::options()
        .write(true)
        .open(&module)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    document(&run(&[], "8.3.27.2325", false), true);
    assert!(!marker.exists());
    fs::write(&marker, "sentinel").unwrap();
    self::catalog(&catalog, &[("ssl_3_1", other.source())]);
    document(&run(&[], "8.3.27.2325", false), true);
    assert!(!marker.exists());
    fs::write(&marker, "sentinel").unwrap();
    let path = consumer.root().join("eska.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("8.3.27.2325", "8.3.28.1000"),
    )
    .unwrap();
    document(&run(&[], "8.3.28.1000", false), true);
    assert!(!marker.exists());
    let previous = fs::read(plan.output()).unwrap();
    let cache_path = shared::namespaces(&catalog)
        .into_iter()
        .find(|root| {
            let state: Value =
                serde_json::from_slice(&fs::read(root.join("base.json")).unwrap()).unwrap();
            state["platform_version"] == "8.3.28.1000"
        })
        .unwrap()
        .join("base.cf");
    let cache = fs::read(&cache_path).unwrap();
    fs::write(other.source().join("Payload.bsl"), "bad").unwrap();
    let failed = document(&run(&[], "8.3.28.1000", true), false);
    assert_eq!(failed["error"]["stage"], "prepare-base-configuration");
    assert_eq!(fs::read(plan.output()).unwrap(), previous);
    assert_eq!(fs::read(&cache_path).unwrap(), cache);
}

#[test]
fn declared_library_errors_are_localized_structured_and_never_fall_back_to_empty_context() {
    let fixture = support::TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
    declare(&consumer, "library='ssl_3_1'");
    let library = project(&fixture.0.join("base"), ProjectType::Configuration);
    let config = fixture.0.join("machine");
    let log = fixture.0.join("native.log");
    for source in [
        None,
        Some(fixture.0.join("missing")),
        Some(library.source().to_owned()),
    ] {
        catalog(
            &config,
            &source
                .as_ref()
                .map(|path| vec![("ssl_3_1", path.as_path())])
                .unwrap_or_default(),
        );
        if source.as_deref() == Some(library.source()) {
            fs::write(library.source().join("Configuration.xml"), "<broken>").unwrap();
        }
        let mut documents = Vec::new();
        for locale in ["ru", "en"] {
            let output = builder(consumer.root(), &config, locale)
                .env("FAKE_IBCMD_LOG", &log)
                .args(["build", "--format", "json"])
                .output()
                .unwrap();
            let error = document(&output, false);
            assert_eq!(error["error"]["code"], "base-reference");
            assert_eq!(error["error"]["data"]["field"], "analysis.configuration");
            assert_eq!(error["error"]["data"]["cause"]["library"], "ssl_3_1");
            assert!(String::from_utf8_lossy(&output.stderr).contains("ssl_3_1"));
            documents.push(error);
        }
        assert_eq!(documents[0], documents[1]);
        assert!(
            !log.exists(),
            "invalid reference must fail before platform discovery"
        );
        assert!(!consumer.root().join("artifacts").exists());
        let override_cf = fixture.0.join("override.cf");
        fs::write(&override_cf, "valid explicit base").unwrap();
        let result = builder(consumer.root(), &config, "en")
            .args([
                "build",
                "--dry-run",
                "--format",
                "json",
                "--base-configuration",
            ])
            .arg(&override_cf)
            .output()
            .unwrap();
        let preview = document(&result, true);
        assert_eq!(
            preview["projects"][0]["base_configuration"]["path"],
            override_cf.to_str().unwrap()
        );
    }
}

#[test]
fn workspace_build_resolves_each_app_for_current_selectors_and_aggregate() {
    let fixture = support::TestDir::new();
    let root = fixture.0.join("workspace");
    fs::create_dir(&root).unwrap();
    let first = project(&fixture.0.join("base-first"), ProjectType::Configuration);
    let second = project(&fixture.0.join("base-second"), ProjectType::Configuration);
    let config = catalog(
        &fixture.0.join("machine"),
        &[("first", first.source()), ("second", second.source())],
    );
    for name in ["one", "two"] {
        let member = project(&root.join(name), ProjectType::Report);
        fs::write(
            member.root().join("eska.toml"),
            format!("[project]\nname='{name}'\ntype='report'\nsource='src'\n"),
        )
        .unwrap();
    }
    fs::write(root.join("eska.toml"), "[workspace]\n[build]\nplatform_version='8.3.27.2325'\n[analysis]\ndefault_application='app-first'\n[workspace.members.app-first]\nconfiguration={library='first'}\nexternal_objects=['one']\n[workspace.members.app-second]\nconfiguration={library='second'}\nexternal_objects=['two']\n").unwrap();
    for (cwd, flags, expected) in [
        (root.join("two"), vec![], vec!["second"]),
        (root.clone(), vec!["-p", "two"], vec!["second"]),
        (root.clone(), vec!["--workspace"], vec!["first", "second"]),
    ] {
        let output = builder(&cwd, &config, "en")
            .args(["build", "--dry-run", "--format", "json"])
            .args(&flags)
            .output()
            .unwrap();
        let preview = document(&output, true);
        let chosen: Vec<_> = preview["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|project| project["base_configuration"]["library"].as_str().unwrap())
            .collect();
        assert_eq!(chosen, expected);
        let built = builder(&cwd, &config, "ru")
            .arg("build")
            .args(&flags)
            .output()
            .unwrap();
        assert!(built.status.success(), "{built:?}");
    }
}
