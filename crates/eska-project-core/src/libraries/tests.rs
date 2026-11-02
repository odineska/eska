use super::*;
use crate::{
    config::ProjectConfig,
    project::{
        create,
        discovery::{self, DiscoveryContext},
    },
    test_support::TestDir,
};
use std::cell::Cell;

const XML: &str = "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Configuration uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>Library</Name></Properties><ChildObjects/></Configuration></MetaDataObject>\r\n";
const REVISION: &str = "1111111111111111111111111111111111111111";

fn project(root: &Path, kind: ProjectType) -> Project {
    create::create(root, &ProjectConfig::new(kind)).unwrap()
}
fn fetch(_library: KnownLibrary, target: &Path) -> Result<Revision, Error> {
    fs::create_dir_all(target.join("src/cf")).unwrap();
    fs::write(target.join("src/cf/Configuration.xml"), XML).unwrap();
    fs::write(target.join("LICENSE"), "vendor license\r\n").unwrap();
    fs::write(target.join("build-script"), "not executed").unwrap();
    Revision::parse(REVISION.into())
}
fn inspect(fixture: &TestDir, project: &Project, library: KnownLibrary) -> AddPlan {
    AddPlan::inspect_at(
        project,
        None,
        library,
        &fixture.0.join("home/.eska/lib"),
        &fixture.0.join("machine/config.toml"),
    )
    .unwrap()
}
fn no_fetch(_: KnownLibrary, _: &Path) -> Result<Revision, Error> {
    panic!("offline reuse cannot fetch")
}

#[test]
fn catalog_replacement_remains_readable_during_publication() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let fixture = TestDir::new();
    let path = fixture.0.join("config.toml");
    let before: &[u8] = b"# machine policy\n[libraries.demo]\nsource='first'\n";
    let after: &[u8] = b"# machine policy\n[libraries.demo]\nsource='second'\n";
    fs::write(&path, before).unwrap();
    let stopped = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut reads = 0;
            while !stopped.load(Ordering::Relaxed) {
                let bytes = fs::read(&path)?;
                if bytes != before && bytes != after {
                    return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
                }
                reads += 1;
            }
            Ok(reads)
        });
        let writing = (|| {
            for index in 0..400 {
                let mut edit = write::Edit::read(&path)?;
                edit.set(if index % 2 == 0 { after } else { before }.to_vec());
                edit.publish()?;
            }
            Ok::<_, Error>(())
        })();
        stopped.store(true, Ordering::Relaxed);
        writing.unwrap();
        assert!(reader.join().unwrap().unwrap() > 0);
    });
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn installs_known_libraries_once_and_reuses_local_sources_without_rewriting_project() {
    for library in [KnownLibrary::Ssl31, KnownLibrary::Ssl32] {
        let fixture = TestDir::new();
        let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
        let config = fixture.0.join("machine/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        let machine = "# keep machine policy\n[build]\nrunner='distrobox'\ncontainer='test'\nplatform_arch='arm64'\n[libraries.other]\nsource='unchanged'\n";
        fs::write(&config, machine).unwrap();
        let first = inspect(&fixture, &consumer, library)
            .execute(fetch, || false)
            .unwrap();
        assert!(first.installed && first.catalog_changed && first.project_changed);
        let root = fixture.0.join("home/.eska/lib").join(library.name());
        assert_eq!(first.source, root.join("src/cf"));
        assert_eq!(
            fs::read(root.join("LICENSE")).unwrap(),
            b"vendor license\r\n"
        );
        assert_eq!(
            fs::read(root.join("src/cf/Configuration.xml")).unwrap(),
            XML.as_bytes()
        );
        assert_eq!(first.revision.as_deref(), Some(REVISION));
        let installed = ProjectConfig::load(&root.join("eska.toml"))
            .unwrap()
            .into_project(root.clone())
            .unwrap();
        assert_eq!(installed.source(), root.join("src/cf"));
        assert_eq!(
            installed.configuration().project_type(),
            ProjectType::Configuration
        );
        let text = fs::read_to_string(&config).unwrap();
        assert!(text.contains("# keep machine policy"));
        assert!(text.contains("container='test'"));
        let manifest = consumer.root().join("eska.toml");
        let before = fs::read(&manifest).unwrap();
        fs::write(root.join("src/cf/Local.bsl"), "uncommitted bytes").unwrap();
        let consumer = discovery::discover(consumer.root()).unwrap();
        let second = inspect(&fixture, &consumer, library)
            .execute(no_fetch, || false)
            .unwrap();
        assert!(!second.installed && !second.catalog_changed && !second.project_changed);
        assert_eq!(fs::read(&manifest).unwrap(), before);
        assert_eq!(
            fs::read(root.join("src/cf/Local.bsl")).unwrap(),
            b"uncommitted bytes"
        );
        fs::remove_file(&config).unwrap();
        let reused = inspect(&fixture, &consumer, library)
            .execute(no_fetch, || false)
            .unwrap();
        assert!(!reused.installed && reused.catalog_changed && !reused.project_changed);
    }
}

#[test]
fn a_registered_user_export_stays_in_place_without_managed_home_writes() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
    let source = fixture.0.join("user-export");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("Configuration.xml"), XML).unwrap();
    let config = fixture.0.join("machine/config.toml");
    fs::create_dir(config.parent().unwrap()).unwrap();
    let original = "# existing source\n[libraries.ssl_3_1]\nsource='../user-export'\n";
    fs::write(&config, original).unwrap();
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31)
        .execute(no_fetch, || false)
        .unwrap();
    assert!(!result.installed && !result.catalog_changed && result.project_changed);
    assert_eq!(fs::read(&config).unwrap(), original.as_bytes());
    assert!(!fixture.0.join("home").exists());
    assert!(!source.join("eska.toml").exists());
    assert_eq!(
        fs::read(source.join("Configuration.xml")).unwrap(),
        XML.as_bytes()
    );
}

#[test]
fn scope_conflicts_and_occupied_directories_never_download_or_replace_user_files() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let path = consumer.root().join("eska.toml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("{original}\n[analysis.configuration]\nsource='../old-base'\n"),
    )
    .unwrap();
    let changed = discovery::discover(consumer.root()).unwrap();
    assert!(matches!(
        AddPlan::inspect_at(
            &changed,
            None,
            KnownLibrary::Ssl31,
            &fixture.0.join("home"),
            &fixture.0.join("config")
        ),
        Err(Error::Conflict { .. })
    ));
    let configuration = project(&fixture.0.join("configuration"), ProjectType::Configuration);
    assert!(matches!(
        AddPlan::inspect_at(
            &configuration,
            None,
            KnownLibrary::Ssl31,
            &fixture.0.join("home"),
            &fixture.0.join("config")
        ),
        Err(Error::UnsupportedType(_))
    ));
    fs::write(&path, original).unwrap();
    let destination = fixture.0.join("home/.eska/lib/ssl_3_1");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("mine"), "user content").unwrap();
    assert!(matches!(
        inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(no_fetch, || false),
        Err(Error::InvalidReceipt(_))
    ));
    assert_eq!(fs::read(destination.join("mine")).unwrap(), b"user content");
    assert!(!fixture.0.join("machine/config.toml").exists());
}

#[test]
fn failed_or_cancelled_fetch_leaves_no_binding_or_partial_installation() {
    for mode in ["network", "xml", "cancel"] {
        let fixture = TestDir::new();
        let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
        let before = fs::read(consumer.root().join("eska.toml")).unwrap();
        let cancelled = Cell::new(false);
        let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(
            |library, target| {
                let revision = fetch(library, target)?;
                if mode == "network" {
                    return Err(Error::Download);
                }
                if mode == "xml" {
                    fs::write(target.join("src/cf/Configuration.xml"), "<broken>").unwrap();
                }
                if mode == "cancel" {
                    cancelled.set(true);
                }
                Ok(revision)
            },
            || cancelled.get(),
        );
        assert!(result.is_err(), "{mode}");
        assert_eq!(fs::read(consumer.root().join("eska.toml")).unwrap(), before);
        assert!(!fixture.0.join("machine/config.toml").exists());
        assert!(!fixture.0.join("home/.eska/lib/ssl_3_1").exists());
        assert!(
            !fs::read_dir(fixture.0.join("home/.eska/lib"))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".eska-library-"))
        );
    }
}

#[test]
fn concurrent_manifest_edits_are_preserved_before_installation_publication() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let manifest = consumer.root().join("eska.toml");
    let original = fs::read_to_string(&manifest).unwrap();
    let changed = format!("{original}\n# user edit during download\n");
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(
        |library, target| {
            let revision = fetch(library, target)?;
            fs::write(&manifest, &changed).unwrap();
            Ok(revision)
        },
        || false,
    );
    assert!(matches!(result, Err(Error::Changed(_))));
    assert_eq!(fs::read_to_string(&manifest).unwrap(), changed);
    assert!(!fixture.0.join("home/.eska/lib/ssl_3_1").exists());
    assert!(!fixture.0.join("machine/config.toml").exists());
}

#[test]
fn cancellation_after_registration_rolls_back_both_new_catalog_and_installation() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let original = fs::read(consumer.root().join("eska.toml")).unwrap();
    let calls = Cell::new(0);
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(fetch, || {
        calls.set(calls.get() + 1);
        calls.get() == 4
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(
        fs::read(consumer.root().join("eska.toml")).unwrap(),
        original
    );
    assert!(!fixture.0.join("home/.eska/lib/ssl_3_1").exists());
    assert!(!fixture.0.join("machine/config.toml").exists());
}

#[test]
fn a_project_write_failure_restores_the_existing_catalog_without_losing_user_policy() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let manifest = consumer.root().join("eska.toml");
    let original = fs::read(&manifest).unwrap();
    let permissions = fs::metadata(&manifest).unwrap().permissions();
    let config = fixture.0.join("machine/config.toml");
    fs::create_dir(config.parent().unwrap()).unwrap();
    let before = b"# user policy\n[build]\nrunner='host'\n";
    fs::write(&config, before).unwrap();
    let calls = Cell::new(0);
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(fetch, || {
        calls.set(calls.get() + 1);
        if calls.get() == 4 {
            let mut readonly = permissions.clone();
            readonly.set_readonly(true);
            fs::set_permissions(&manifest, readonly).unwrap();
        }
        false
    });
    fs::set_permissions(&manifest, permissions).unwrap();
    assert!(matches!(result, Err(Error::Io { .. })));
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(!fixture.0.join("home/.eska/lib/ssl_3_1").exists());
}

#[test]
fn an_installation_lease_rejects_a_second_writer_before_fetch() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let root = fixture.0.join("home/.eska/lib");
    let _lock = write::lock(&root.join("ssl_3_1.eska-add.lock")).unwrap();
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(no_fetch, || false);
    assert!(matches!(result, Err(Error::Locked(_))));
    assert!(!root.join("ssl_3_1").exists());
    assert!(!fixture.0.join("machine/config.toml").exists());
}

#[test]
fn workspace_attachment_changes_only_the_selected_owner() {
    let fixture = TestDir::new();
    let root = fixture.0.join("workspace");
    fs::create_dir(&root).unwrap();
    for name in ["one", "two"] {
        let member = project(&root.join(name), ProjectType::Report);
        fs::write(
            member.root().join("eska.toml"),
            format!("[project]\nname='{name}'\ntype='report'\n"),
        )
        .unwrap();
    }
    fs::write(root.join("eska.toml"),"# workspace policy\n[workspace]\n[analysis]\ndefault_application='first'\n[workspace.members.first]\nexternal_objects=['one']\n[workspace.members.second]\nexternal_objects=['two']\n").unwrap();
    let DiscoveryContext::Workspace { workspace, .. } = discovery::discover_context(&root).unwrap()
    else {
        panic!()
    };
    let selected = workspace
        .members()
        .iter()
        .find(|member| member.name().as_str() == "two")
        .unwrap();
    let config = fixture.0.join("machine/config.toml");
    let plan = AddPlan::inspect_at(
        selected.project(),
        Some(&workspace),
        KnownLibrary::Ssl31,
        &fixture.0.join("home/.eska/lib"),
        &config,
    )
    .unwrap();
    plan.execute(fetch, || false).unwrap();
    let text = fs::read_to_string(root.join("eska.toml")).unwrap();
    assert!(text.contains("# workspace policy"));
    let document: toml::Value = toml::from_str(&text).unwrap();
    assert!(
        document["workspace"]["members"]["first"]
            .get("configuration")
            .is_none()
    );
    assert_eq!(
        document["workspace"]["members"]["second"]["configuration"]["library"].as_str(),
        Some("ssl_3_1")
    );
    assert!(
        !fs::read_to_string(root.join("two/eska.toml"))
            .unwrap()
            .contains("library")
    );
}

#[test]
fn a_workspace_member_edit_during_download_aborts_before_publishing_the_root_link() {
    let fixture = TestDir::new();
    let root = fixture.0.join("workspace");
    fs::create_dir(&root).unwrap();
    let member = project(&root.join("report"), ProjectType::Report);
    let manifest = member.root().join("eska.toml");
    fs::write(&manifest, "[project]\nname='report'\ntype='report'\n").unwrap();
    let root_manifest = "[workspace]\n[workspace.members.app]\nexternal_objects=['report']\n";
    fs::write(root.join("eska.toml"), root_manifest).unwrap();
    let DiscoveryContext::Workspace { workspace, .. } = discovery::discover_context(&root).unwrap()
    else {
        panic!()
    };
    let config = fixture.0.join("machine/config.toml");
    let plan = AddPlan::inspect_at(
        workspace.members()[0].project(),
        Some(&workspace),
        KnownLibrary::Ssl31,
        &fixture.0.join("home/.eska/lib"),
        &config,
    )
    .unwrap();
    let changed = "[project]\nname='report'\ntype='configuration'\n";
    let result = plan.execute(
        |library, target| {
            let revision = fetch(library, target)?;
            fs::write(&manifest, changed).unwrap();
            Ok(revision)
        },
        || false,
    );
    assert!(matches!(result,Err(Error::Changed(path)) if path==manifest));
    assert_eq!(
        fs::read_to_string(root.join("eska.toml")).unwrap(),
        root_manifest
    );
    assert_eq!(fs::read_to_string(&manifest).unwrap(), changed);
    assert!(!config.exists());
    assert!(!fixture.0.join("home/.eska/lib/ssl_3_1").exists());
}

#[test]
fn an_existing_vendor_manifest_is_preserved_exactly() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Processing);
    let vendor = "# keep vendor settings\r\n[project]\r\ntype='configuration'\r\nsource='src/cf'\r\n[build]\r\nplatform_version='8.3.24.1778'\r\nartifacts_directory='compiled'\r\n";
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31)
        .execute(
            |library, target| {
                let revision = fetch(library, target)?;
                fs::write(target.join("eska.toml"), vendor).unwrap();
                Ok(revision)
            },
            || false,
        )
        .unwrap();
    assert!(result.installed);
    assert_eq!(
        fs::read(fixture.0.join("home/.eska/lib/ssl_3_1/eska.toml")).unwrap(),
        vendor.as_bytes()
    );
}

#[cfg(unix)]
#[test]
fn a_checkout_alias_cannot_turn_an_existing_user_directory_into_a_managed_installation() {
    let fixture = TestDir::new();
    let consumer = project(&fixture.0.join("consumer"), ProjectType::Report);
    let existing = fixture.0.join("user-repository");
    fs::create_dir_all(existing.join("src/cf")).unwrap();
    fs::write(existing.join("src/cf/Configuration.xml"), XML).unwrap();
    let result = inspect(&fixture, &consumer, KnownLibrary::Ssl31).execute(
        |_, target| {
            std::os::unix::fs::symlink(&existing, target).unwrap();
            Revision::parse(REVISION.into())
        },
        || false,
    );
    assert!(matches!(result, Err(Error::InvalidReceipt(_))));
    assert!(!existing.join("eska.toml").exists());
    assert!(!existing.join(".eska").exists());
    assert_eq!(
        fs::read(existing.join("src/cf/Configuration.xml")).unwrap(),
        XML.as_bytes()
    );
    assert!(!fixture.0.join("home/.eska/lib/ssl_3_1").exists());
    assert!(!fixture.0.join("machine/config.toml").exists());
}
