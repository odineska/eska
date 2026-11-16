//! Structural creation negotiates its contract and never replays an acknowledged mutation.

use super::*;
use std::fs;

fn creation_source() -> TestDir {
    let directory = TestDir::new();
    fixture(&directory.0, "configuration");
    fs::create_dir_all(directory.0.join("src/Ext")).unwrap();
    fs::write(
        directory.0.join("src/Ext/ParentConfigurations.bin"),
        "{6,0,0,0,0,0}",
    )
    .unwrap();
    directory
}

fn initialize_creation(client: &mut Client, minor: u32, editing: bool) -> Value {
    client.ok(
        "initialize",
        json!({"apiVersion":{"major":1,"minor":minor},
        "client":{"name":"creation-test","version":"1"},"allowPropertyEdits":editing}),
    )
}

fn request(schema: &Value, name: &str) -> Value {
    json!({"owner":schema["owner"],"snapshot":schema["snapshot"],
        "kind":"attribute","name":name,"types":[{"kind":"boolean"}]})
}

#[test]
fn creation_preview_apply_and_invalidation_work_in_both_locales() {
    for locale in ["ru-RU", "en-US"] {
        let directory = creation_source();
        let descriptor = directory.0.join("src/Catalogs/Контрагенты.xml");
        let original = fs::read(&descriptor).unwrap();
        let mut client = Client::new(locale);
        let hello = initialize_creation(&mut client, 48, true);
        assert_eq!(hello["capabilities"]["metadataCreation"]["enabled"], true);
        let mut open = client.open(&directory.0);
        let schema = client.ok(
            "metadata/creationSchema",
            context(&open, json!({"owner":"catalog:Контрагенты"})),
        );
        assert_eq!(schema["writable"], true);
        assert!(
            schema["kindOptions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind["caption"]["ru-RU"] == "Реквизит")
        );
        let preview = client.ok(
            "metadata/previewCreate",
            context(&open, json!({"request":request(&schema,"Проверено")})),
        );
        assert_eq!(fs::read(&descriptor).unwrap(), original);
        assert_eq!(
            preview["objectId"],
            "catalog:Контрагенты/attribute:Проверено"
        );
        assert_eq!(
            preview["prepared"]["identity"]["uuid"]
                .as_str()
                .unwrap()
                .len(),
            36
        );
        assert_eq!(preview["source"]["encoding"], "utf-8");
        let mut expected = String::from_utf8(original).unwrap();
        for change in preview["changes"].as_array().unwrap().iter().rev() {
            expected.replace_range(
                usize::try_from(change["range"]["start"].as_u64().unwrap()).unwrap()
                    ..usize::try_from(change["range"]["end"].as_u64().unwrap()).unwrap(),
                change["replacement"].as_str().unwrap(),
            );
        }
        let applied = client.ok(
            "metadata/create",
            context(&open, json!({"prepared":preview["prepared"]})),
        );
        assert_eq!(applied["objectId"], preview["objectId"]);
        assert_eq!(applied["uuid"], preview["prepared"]["identity"]["uuid"]);
        assert_eq!(fs::read_to_string(&descriptor).unwrap(), expected);
        assert!(
            client
                .events
                .iter()
                .any(|event| event["method"] == "metadata/changed"
                    && event["params"]["generation"] == applied["generation"])
        );
        open["projects"][0]["generation"] = applied["generation"].clone();
        let object = client.ok(
            "metadata/get",
            context(&open, json!({"objectId":applied["objectId"]})),
        );
        assert_eq!(object["object"]["name"], "Проверено");
        let repeated = client.request(
            "metadata/create",
            context(&open, json!({"prepared":preview["prepared"]})),
        );
        assert!(repeated.get("error").is_some());
        assert_eq!(fs::read_to_string(&descriptor).unwrap(), expected);
        client.finish();
    }
}

#[test]
fn creation_requires_negotiation_and_explicit_write_permission() {
    let directory = creation_source();
    for (minor, writable) in [(47, true), (48, false)] {
        let mut client = Client::new("en-US");
        let hello = initialize_creation(&mut client, minor, writable);
        let open = client.open(&directory.0);
        if minor == 47 {
            assert!(hello["capabilities"].get("metadataCreation").is_none());
            let response = client.request(
                "metadata/creationSchema",
                context(&open, json!({"owner":"catalog:Контрагенты"})),
            );
            assert_eq!(response["error"]["code"], -32601);
        } else {
            assert_eq!(hello["capabilities"]["metadataCreation"]["enabled"], false);
            let schema = client.ok(
                "metadata/creationSchema",
                context(&open, json!({"owner":"catalog:Контрагенты"})),
            );
            assert_eq!(schema["writable"], false);
            let response =
                client.request("metadata/create", context(&open, json!({"prepared":{}})));
            assert_eq!(response["error"]["data"]["kind"], "property_read_only");
        }
        client.finish();
    }
}

#[test]
fn creation_rejects_forged_fields_and_external_edits_without_writing() {
    let directory = creation_source();
    let descriptor = directory.0.join("src/Catalogs/Контрагенты.xml");
    let mut client = Client::new("en-US");
    initialize_creation(&mut client, 48, true);
    let open = client.open(&directory.0);
    let schema = client.ok(
        "metadata/creationSchema",
        context(&open, json!({"owner":"catalog:Контрагенты"})),
    );
    let preview = client.ok(
        "metadata/previewCreate",
        context(&open, json!({"request":request(&schema,"Проверено")})),
    );
    let mut forged = preview["prepared"].clone();
    forged["request"]["xml"] = json!("<Attribute/>");
    let before = fs::read(&descriptor).unwrap();
    let response = client.request(
        "metadata/create",
        context(&open, json!({"prepared":forged})),
    );
    assert_eq!(response["error"]["code"], -32602);
    assert_eq!(fs::read(&descriptor).unwrap(), before);
    let external = format!(
        "{}\n<!-- external edit -->\n",
        String::from_utf8(before).unwrap()
    );
    fs::write(&descriptor, &external).unwrap();
    let response = client.request(
        "metadata/create",
        context(&open, json!({"prepared":preview["prepared"]})),
    );
    assert!(response.get("error").is_some());
    assert_eq!(fs::read_to_string(&descriptor).unwrap(), external);
    client.finish();
}
