//! Real-pipe activity lifecycle: persistence, disabled IO, generations and unchanged CLI scope.
use super::*;
use std::fs;

fn initialize(client: &mut Client, locale: &str) {
    let hello = client.ok("initialize", json!({"apiVersion":{"major":1,"minor":35},"client":{"name":"activity","version":"1"},"locale":locale}));
    assert_eq!(
        hello["capabilities"]["workspaceActivity"]["schemaVersion"],
        1
    );
}
fn open(client: &mut Client, root: &std::path::Path) -> Value {
    client.ok("workspace/open", json!({"start":{"value":root,"encoding":"utf-8"},"selection":{"kind":"all"},"diskCache":false}))
}
fn activity(client: &mut Client, info: &Value, index: usize, enabled: bool) -> Value {
    client.ok("workspace/activitySet", json!({"sessionId":info["sessionId"],"revision":info["activity"]["revision"],"projectId":info["projects"][index]["projectId"],"enabled":enabled}))
}
fn request_context(info: &Value, index: usize) -> Value {
    json!({"sessionId":info["sessionId"],"projectId":info["projects"][index]["projectId"],"generation":info["projects"][index]["generation"]})
}

#[test]
fn workspace_activity_persists_without_reading_disabled_xml_or_changing_cli_scope() {
    for locale in ["ru-RU", "en-US"] {
        let directory = TestDir::new();
        let root = directory.0.join("workspace");
        fs::create_dir(&root).unwrap();
        for name in ["one", "two"] {
            let member = root.join(name);
            fixture(&member, "processing");
            fs::write(
                member.join("eska.toml"),
                format!("[project]\nname='{name}'\ntype='processing'\n"),
            )
            .unwrap();
        }
        fs::write(
            root.join("eska.toml"),
            "[workspace]\n[workspace.members.app]\nexternal_objects=['one','two']\n",
        )
        .unwrap();
        let before = bytes(&root);
        let config = directory.0.join("machine");
        let mut client = Client::with_config(locale, Some(&config));
        initialize(&mut client, locale);
        let initial = open(&mut client, &root);
        assert_eq!(initial["activity"]["hideDisabled"], false);
        let one = request_context(&initial, 0);
        let two = request_context(&initial, 1);
        let mut index = one.clone();
        index["action"] = json!("start");
        client.ok("metadata/index", index);
        let off = activity(&mut client, &initial, 0, false);
        assert_eq!(off["projects"][0]["activity"]["enabled"], false);
        assert_ne!(
            off["projects"][0]["generation"],
            initial["projects"][0]["generation"]
        );
        assert_eq!(off["projects"][1], initial["projects"][1]);
        let denied = client.request("metadata/root", request_context(&off, 0));
        assert_eq!(denied["error"]["data"]["kind"], "project_inactive");
        client.ok("metadata/root", two);
        let stale = client.request("workspace/activitySet", json!({"sessionId":off["sessionId"],"revision":initial["activity"]["revision"],"projectId":off["projects"][0]["projectId"],"enabled":true}));
        assert_eq!(stale["error"]["data"]["details"]["reason"], "conflict");
        let hidden = client.ok("workspace/activityFilter", json!({"sessionId":off["sessionId"],"revision":off["activity"]["revision"],"hideDisabled":true}));
        assert_eq!(hidden["projects"], off["projects"]);
        assert_eq!(bytes(&root), before);
        // Explicit CLI composition still contains both members.
        let context = Command::new(
            std::path::Path::new(env!("CARGO_BIN_EXE_eska"))
                .with_file_name(format!("eska-project{}", std::env::consts::EXE_SUFFIX)),
        )
        .args(["context", "--format", "json"])
        .current_dir(&root)
        .output()
        .unwrap();
        assert!(
            context.status.success(),
            "{}",
            String::from_utf8_lossy(&context.stderr)
        );
        let context: Value = serde_json::from_slice(&context.stdout).unwrap();
        assert_eq!(context["context"]["sources"].as_array().unwrap().len(), 2);
        client.finish();
        fs::rename(root.join("one/src"), root.join("one/saved-source")).unwrap();
        fs::create_dir(root.join("one/src")).unwrap();
        let mut restarted = Client::with_config(locale, Some(&config));
        initialize(&mut restarted, locale);
        let restored = open(&mut restarted, &root);
        assert_eq!(restored["activity"], hidden["activity"]);
        assert_eq!(
            restored["projects"][0]["displayName"],
            hidden["projects"][0]["displayName"]
        );
        assert_eq!(restored["projects"][0]["activity"]["enabled"], false);
        let failed = restarted.request("workspace/activitySet", json!({"sessionId":restored["sessionId"],"revision":restored["activity"]["revision"],"projectId":restored["projects"][0]["projectId"],"enabled":true}));
        assert!(failed.get("error").is_some());
        assert_eq!(
            restarted.ok("project/info", json!({"sessionId":restored["sessionId"]})),
            restored
        );
        fs::remove_dir(root.join("one/src")).unwrap();
        fs::rename(root.join("one/saved-source"), root.join("one/src")).unwrap();
        let resumed = activity(&mut restarted, &restored, 0, true);
        assert_eq!(resumed["projects"][0]["activity"]["enabled"], true);
        let mut index = request_context(&resumed, 0);
        index["action"] = json!("status");
        assert_eq!(
            restarted.ok("metadata/index", index)["progress"]["state"],
            "not_started"
        );
        assert_eq!(bytes(&root), before);
        restarted.finish();
    }
}

/// A live client updates only its own lease; another window's active lease remains authoritative.
#[cfg(unix)]
#[test]
fn workspace_activity_updates_analyzer_lease_and_preserves_other_consumers() {
    let directory = TestDir::new();
    let root = directory.0.join("workspace");
    fs::create_dir(&root).unwrap();
    for name in ["one", "two"] {
        let member = root.join(name);
        fixture(&member, "processing");
        fs::write(
            member.join("eska.toml"),
            format!("[project]\nname='{name}'\ntype='processing'\n"),
        )
        .unwrap();
    }
    fs::write(
        root.join("eska.toml"),
        "[workspace]\n[workspace.members.app]\nexternal_objects=['one','two']\n",
    )
    .unwrap();
    let config = directory.0.join("machine");
    let mut first = Client::with_config("en-US", Some(&config));
    initialize(&mut first, "en-US");
    let initial = open(&mut first, &root);
    let mut parameters = analyzer::start(&root);
    parameters["analyzer"] = json!({"value":analyzer::fake(&root),"encoding":"utf-8"});
    first.ok("analyzer/configPrepare", parameters.clone());
    let lease = first.ok("analyzer/acquire", analyzer::start(&root));
    let off = activity(&mut first, &initial, 0, false);
    parameters["leaseId"] = lease["leaseId"].clone();
    let updated = first.ok("analyzer/activityUpdate", parameters.clone());
    assert_ne!(updated["leaseId"], lease["leaseId"]);
    let effective = first.ok("analyzer/context", analyzer::start(&root));
    assert_eq!(effective["context"]["sources"].as_array().unwrap().len(), 1);
    let mut second = Client::with_config("ru-RU", Some(&config));
    initialize(&mut second, "ru-RU");
    open(&mut second, &root);
    let shared = second.ok("analyzer/acquire", analyzer::start(&root));
    let on = activity(&mut first, &off, 0, true);
    parameters["leaseId"] = updated["leaseId"].clone();
    let busy = first.request("analyzer/activityUpdate", parameters.clone());
    assert_eq!(
        busy["error"]["data"]["kind"],
        "analyzer_configuration_failed"
    );
    assert_eq!(busy["error"]["data"]["details"]["code"], "busy");
    second.ok("analyzer/release", json!({"leaseId":shared["leaseId"]}));
    second.finish();
    // The first lease was restored on failure, so the same lease identity can retry.
    let restored = first.ok("analyzer/activityUpdate", parameters);
    let off = activity(&mut first, &on, 0, false);
    let all = activity(&mut first, &off, 1, false);
    let none = first.request("analyzer/context", analyzer::start(&root));
    assert_eq!(none["error"]["data"]["kind"], "workspace_inactive");
    let released = first.request("analyzer/release", json!({"leaseId":restored["leaseId"]}));
    assert_eq!(released["error"]["data"]["kind"], "unknown_analyzer_lease");
    assert!(
        all["projects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|project| project["activity"]["enabled"] == false)
    );
    first.finish();
}
