//! A living server re-resolves catalogs while cross-process leases protect the shared config.

use super::*;

fn export(root: &Path, kind: ProjectType) {
    create::create(root, &ProjectConfig::new(kind)).unwrap();
    let (tag, name) = if kind == ProjectType::Report {
        ("ExternalReport", "Report.xml")
    } else {
        ("Configuration", "Configuration.xml")
    };
    fs::write(root.join("src").join(name), format!(
        "<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses'><{tag} uuid='12345678-1234-1234-1234-123456789012'><Properties><Name>Пример</Name></Properties><ChildObjects/></{tag}></MetaDataObject>"
    )).unwrap();
}

fn catalog(config: &Path, source: &Path, other: &str) {
    let source = source.to_str().unwrap();
    fs::write(
        config,
        format!("[libraries.ssl_3_1]\nsource={source:?}\n[libraries.other]\nsource={other:?}\n"),
    )
    .unwrap();
}

fn initialize(root: &Path, config: &Path, locale: &str) -> Client {
    let mut client = Client::with_config(root, locale, Some(config));
    client.request("initialize", &json!({"apiVersion":{"major":1,"minor":12},"client":{"name":"library-test","version":"1"}}));
    client
}

#[test]
fn living_server_resolves_catalog_changes_and_recovers_without_reopening_workspace() {
    let fixture = support::TestDir::new();
    let root = fixture.0.join("report");
    export(&root, ProjectType::Report);
    fs::write(
        root.join("eska.toml"),
        "[project]\ntype='report'\n[analysis.configuration]\nlibrary='ssl_3_1'\n",
    )
    .unwrap();
    let config = fixture.0.join("config.toml");
    for locale in ["ru", "en"] {
        let mut client = initialize(&root, &fixture.0, locale);
        let params = json!({"start":{"value":root,"encoding":"utf-8"}});
        let first = fixture.0.join(format!("first-{locale}"));
        let second = fixture.0.join(format!("second-{locale}"));
        export(&first, ProjectType::Configuration);
        export(&second, ProjectType::Configuration);
        catalog(&config, &first.join("src"), "unselected/one");
        let initial = client.request("analyzer/configStatus", &params);
        catalog(&config, &first.join("src"), "unselected/two");
        assert_eq!(client.request("analyzer/configStatus", &params), initial);
        catalog(&config, &second.join("src"), "unselected/two");
        let changed = client.request("analyzer/configStatus", &params);
        assert_ne!(
            changed["ownership"]["input_fingerprint"],
            initial["ownership"]["input_fingerprint"]
        );
        let context = client.request("analyzer/context", &params);
        let expected = second.join("src").canonicalize().unwrap();
        assert_eq!(
            context["context"]["sources"][0]["source"]["value"],
            expected.to_str().unwrap()
        );
        fs::remove_file(&config).unwrap();
        let failure = client.response("analyzer/context", &params);
        assert_eq!(
            failure["error"]["data"]["details"]["data"]["library"],
            "ssl_3_1"
        );
        catalog(&config, &second.join("src"), "unused");
        assert_eq!(client.request("analyzer/context", &params), context);
        client.finish();
    }
    assert!(!root.join("bsl-analyzer.toml").exists());
}

/// The fixture analyzer only validates transaction boundaries; native language acceptance uses the real app.
#[test]
#[cfg(unix)]
fn two_live_servers_cannot_replace_a_library_config_until_both_leases_are_released() {
    let fixture = support::TestDir::new();
    let root = fixture.0.join("report");
    export(&root, ProjectType::Report);
    fs::write(root.join("eska.toml"), "[project]\ntype='report'\n[analysis.configuration]\nlibrary='ssl_3_1'\n[tools.bsl-analyzer.diagnostics.parameters]\nLineLength=false\n").unwrap();
    let first = fixture.0.join("first");
    let second = fixture.0.join("second");
    export(&first, ProjectType::Configuration);
    export(&second, ProjectType::Configuration);
    let config = fixture.0.join("config.toml");
    catalog(&config, &first.join("src"), "unused");
    let mut one = initialize(&root, &fixture.0, "ru");
    let mut two = initialize(&root, &fixture.0, "en");
    let analyzer = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/analyzer-config.py")
        .canonicalize()
        .unwrap();
    let params = json!({"start":{"value":root,"encoding":"utf-8"},"analyzer":{"value":analyzer,"encoding":"utf-8"}});
    let prepared = one.request("analyzer/configPrepare", &params);
    assert_eq!(prepared["status"], "current");
    let old = fs::read(root.join("bsl-analyzer.toml")).unwrap();
    let lease_one = one.request("analyzer/acquire", &params);
    let lease_two = two.request("analyzer/acquire", &params);
    catalog(&config, &second.join("src"), "unused");
    assert_eq!(
        one.request("analyzer/configStatus", &params)["status"],
        "stale"
    );
    let busy =
        |value: Value| assert_eq!(value["error"]["data"]["details"]["code"], "busy", "{value}");
    busy(one.response("analyzer/configPrepare", &params));
    one.request("analyzer/release", &json!({"leaseId":lease_one["leaseId"]}));
    busy(one.response("analyzer/configPrepare", &params));
    assert_eq!(fs::read(root.join("bsl-analyzer.toml")).unwrap(), old);
    two.request("analyzer/release", &json!({"leaseId":lease_two["leaseId"]}));
    let fresh = one.request("analyzer/configPrepare", &params);
    assert_eq!(fresh["status"], "current");
    assert_ne!(
        fresh["ownership"]["input_fingerprint"],
        prepared["ownership"]["input_fingerprint"]
    );
    let status = two.request("analyzer/configStatus", &params);
    assert_eq!(status["status"], "current");
    assert_eq!(status["ownership"], fresh["ownership"]);
    let text = fs::read_to_string(root.join("bsl-analyzer.toml")).unwrap();
    assert!(text.contains("LineLength = false"));
    for base in [first, second] {
        assert!(!base.join("bsl-analyzer.toml").exists());
        assert!(!base.join(".eska").exists());
    }
    one.finish();
    two.finish();
}
