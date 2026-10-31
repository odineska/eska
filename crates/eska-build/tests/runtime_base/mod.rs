//! Real process boundaries test cache ownership; native 1C acceptance is separate.

use super::*;
use eska_build_runtime::build::{
    self, BaseConfiguration, BuildError, BuildPlan, BuildStage, Ibcmd, RunnerPreference,
    ToolOptions,
};

fn platform() -> Ibcmd {
    Ibcmd::discover(
        &build::PlatformVersion::parse("8.3.27.2325").unwrap(),
        &ToolOptions::new(Some(ibcmd_fixture::executable(&repository())), None, None)
            .with_machine_defaults(RunnerPreference::Host, None, None),
    )
    .unwrap()
}

fn plan(consumer: &Project, source: &Path) -> BuildPlan {
    BuildPlan::new(consumer, None)
        .unwrap()
        .with_base_configuration(Some(BaseConfiguration::DesignerXml {
            source: source.to_owned(),
            library: None,
        }))
        .unwrap()
}

fn run(plan: &BuildPlan, platform: &Ibcmd) -> usize {
    let mut base_lines = 0;
    build::execute_streaming(plan, platform, |stage, _, _| {
        if stage == BuildStage::PrepareBaseConfiguration {
            base_lines += 1;
        }
    })
    .unwrap();
    base_lines
}

fn state(plan: &BuildPlan) -> Value {
    serde_json::from_slice(&fs::read(plan.infobase_root().join("base.json")).unwrap()).unwrap()
}

#[test]
fn xml_base_reuses_verified_cf_and_detects_same_size_same_mtime_changes() {
    let fixture = support::TestDir::new();
    let library = project(
        &fixture.0.join("БСП с пробелами"),
        ProjectType::Configuration,
    );
    fs::remove_file(library.root().join("eska.toml")).unwrap();
    let module = library.source().join("Library.bsl");
    fs::write(&module, "one").unwrap();
    let platform = platform();
    for kind in [ProjectType::Processing, ProjectType::Report] {
        let consumer = project(&fixture.0.join(kind.as_str()), kind);
        let plan = plan(&consumer, library.source());
        assert!(run(&plan, &platform) > 0);
        let original = state(&plan);
        let marker = plan.infobase_root().join("data/retained");
        fs::write(&marker, "owned data").unwrap();
        assert_eq!(
            run(&plan, &platform),
            0,
            "an unchanged library cannot start another base import"
        );
        assert_eq!(state(&plan), original);
        assert!(marker.is_file());

        let modified = fs::metadata(&module).unwrap().modified().unwrap();
        let previous = fs::read(&module).unwrap();
        let next = if previous == b"one" { b"two" } else { b"one" };
        fs::write(&module, next).unwrap();
        fs::File::options()
            .write(true)
            .open(&module)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(fs::metadata(&module).unwrap().modified().unwrap(), modified);
        assert!(run(&plan, &platform) > 0);
        assert_ne!(state(&plan)["source_snapshot"], original["source_snapshot"]);
        assert_eq!(fs::read(&module).unwrap(), next);

        fs::write(plan.infobase_root().join("base.cf"), "corrupt cache").unwrap();
        assert!(run(&plan, &platform) > 0);
        fs::remove_file(plan.infobase_root().join("base.cf")).unwrap();
        assert!(run(&plan, &platform) > 0);

        build::execute_streaming_with_manifest(
            &plan,
            &consumer,
            Some(kind.as_str()),
            build::GitProvenance::unavailable(),
            &platform,
            |_, _, _| {},
        )
        .unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(build::manifest_path(plan.output())).unwrap())
                .unwrap();
        assert_eq!(manifest["source"]["base_context"]["kind"], "designer-xml");
        assert!(manifest["source"]["base_context"].get("library").is_none());
        assert_eq!(
            manifest["source"]["base_context"]["snapshot_id"],
            state(&plan)["source_snapshot"]
        );
        assert_eq!(
            manifest["source"]["base_configuration"]["value"],
            state(&plan)["cf_sha256"]
        );
    }
    assert!(!library.root().join("eska.toml").exists());
    assert!(!library.root().join("artifacts").exists());
}

#[test]
fn cache_lease_cleanup_and_bad_input_keep_the_previous_artifact() {
    let fixture = support::TestDir::new();
    let library = project(&fixture.0.join("library"), ProjectType::Configuration);
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let plan = plan(&consumer, library.source());
    let platform = platform();
    run(&plan, &platform);
    let before = fs::read(plan.output()).unwrap();
    let cache = fs::read(plan.infobase_root().join("base.cf")).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(plan.infobase_root().with_extension("erf.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(matches!(
        build::execute(&plan, &platform),
        Err(BuildError::ManagedInfobase(
            build::ManagedInfobaseError::Locked { .. }
        ))
    ));
    assert!(build::clean_infobase(plan.infobase_root(), consumer.root()).is_err());
    drop(lock);

    let descriptor = library.source().join("Configuration.xml");
    let xml = fs::read(&descriptor).unwrap();
    fs::write(&descriptor, "<broken>").unwrap();
    assert!(matches!(
        build::execute(&plan, &platform),
        Err(BuildError::BaseConfigurationXmlInvalid(_))
    ));
    assert_eq!(fs::read(plan.output()).unwrap(), before);
    assert_eq!(
        fs::read(plan.infobase_root().join("base.cf")).unwrap(),
        cache
    );
    fs::write(&descriptor, xml).unwrap();
    assert_eq!(
        build::clean_infobase(plan.infobase_root(), consumer.root()).unwrap(),
        build::InfobaseCleanOutcome::Removed
    );
    assert!(!plan.infobase_root().exists());
    assert_eq!(fs::read(plan.output()).unwrap(), before);
    assert!(run(&plan, &platform) > 0);
}

#[test]
fn readonly_library_permissions_survive_and_temporary_copies_are_removed() {
    let fixture = support::TestDir::new();
    let library = project(&fixture.0.join("library"), ProjectType::Configuration);
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
    let plan = plan(&consumer, library.source());
    let path = library.source().join("Configuration.xml");
    let original = fs::read(&path).unwrap();
    let permissions = fs::metadata(&path).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly).unwrap();
    #[cfg(unix)]
    let directory_permissions = {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::metadata(library.source()).unwrap().permissions();
        fs::set_permissions(library.source(), fs::Permissions::from_mode(0o555)).unwrap();
        permissions
    };
    let result = build::execute(&plan, &platform());
    let still_readonly = fs::metadata(&path).unwrap().permissions().readonly();
    fs::set_permissions(&path, permissions).unwrap();
    #[cfg(unix)]
    fs::set_permissions(library.source(), directory_permissions).unwrap();
    result.unwrap();
    assert!(still_readonly);
    assert_eq!(fs::read(path).unwrap(), original);
    for entry in fs::read_dir(consumer.root().join("artifacts")).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            path == plan.output() || plan.infobase_root().starts_with(&path),
            "temporary operation directory remains: {path:?}"
        );
    }
}

#[test]
fn xml_base_rejects_staging_and_explicit_output_inside_its_sources() {
    let fixture = support::TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
    let library = project(&fixture.0.join("library"), ProjectType::Configuration);
    let input = || BaseConfiguration::DesignerXml {
        source: library.source().to_owned(),
        library: None,
    };
    let output = library.source().join("nested/result.epf");
    let explicit = BuildPlan::new(&consumer, Some(&output))
        .unwrap()
        .with_base_configuration(Some(input()))
        .unwrap();
    assert!(matches!(
        build::preflight(&explicit),
        Err(BuildError::BaseConfigurationOutputOverlap { .. })
    ));
    assert!(!library.source().join("nested").exists());

    let artifacts = consumer.root().join("artifacts");
    fs::create_dir(&artifacts).unwrap();
    fs::copy(
        library.source().join("Configuration.xml"),
        artifacts.join("Configuration.xml"),
    )
    .unwrap();
    let internal = BuildPlan::new(&consumer, None)
        .unwrap()
        .with_base_configuration(Some(BaseConfiguration::DesignerXml {
            source: artifacts,
            library: None,
        }))
        .unwrap();
    assert!(matches!(
        build::preflight(&internal),
        Err(BuildError::BaseConfigurationOutputOverlap { .. })
    ));
    assert!(!internal.infobase_root().exists());
}
