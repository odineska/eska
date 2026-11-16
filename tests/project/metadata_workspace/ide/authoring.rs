//! Structural authoring shares source permissions and mutation acknowledgement rules over RPC.

use super::*;
use std::fs;

fn source() -> TestDir {
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

fn initialize(client: &mut Client, minor: u32, writable: bool) -> Value {
    client.ok("initialize",json!({"apiVersion":{"major":1,"minor":minor},"client":{"name":"authoring-test","version":"1"},"allowPropertyEdits":writable}))
}

#[test]
fn object_and_form_authoring_negotiate_permissions_and_publish_invalidation() {
    for locale in ["ru-RU", "en-US"] {
        let directory = source();
        let mut client = Client::new(locale);
        let hello = initialize(&mut client, 49, true);
        assert_eq!(hello["capabilities"]["metadataObjects"]["enabled"], true);
        assert_eq!(hello["capabilities"]["formAuthoring"]["enabled"], true);
        let mut open = client.open(&directory.0);
        let schema = client.ok("metadata/objectSchema", context(&open, json!({})));
        assert!(
            schema["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "create")
        );
        let preview = client.ok(
            "metadata/previewObject",
            context(
                &open,
                json!({"request":{"operation":"create","kind":"CommonForm","name":"Authoring"}}),
            ),
        );
        assert_eq!(preview["objectId"], "common-form:Authoring");
        assert!(!directory.0.join("src/CommonForms/Authoring.xml").exists());
        assert!(
            preview["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path["value"] == "Configuration.xml")
        );
        let applied = client.ok(
            "metadata/applyObject",
            context(&open, json!({"prepared":preview["prepared"]})),
        );
        assert!(
            client
                .events
                .iter()
                .any(|event| event["method"] == "metadata/changed"
                    && event["params"]["generation"] == applied["generation"])
        );
        open["projects"][0]["generation"] = applied["generation"].clone();
        let schema = client.ok(
            "metadata/formEditSchema",
            context(&open, json!({"form":applied["objectId"]})),
        );
        let request = json!({"form":applied["objectId"],"snapshot":schema["snapshot"],"operations":[{"kind":"addElement","parent":{"kind":"root"},"element":{"kind":"UsualGroup","name":"Totals"}}]});
        let plan = client.ok(
            "metadata/previewFormEdit",
            context(&open, json!({"request":request})),
        );
        let edited = client.ok(
            "metadata/applyFormEdit",
            context(&open, json!({"prepared":plan["prepared"]})),
        );
        assert_eq!(edited["assignedIds"], plan["assignedIds"]);
        assert_ne!(edited["generation"], applied["generation"]);
        open["projects"][0]["generation"] = edited["generation"].clone();
        let repeated = client.request(
            "metadata/applyFormEdit",
            context(&open, json!({"prepared":plan["prepared"]})),
        );
        assert_eq!(repeated["error"]["data"]["kind"], "property_conflict");
        client.finish();
    }
}

#[test]
fn authoring_never_writes_for_old_or_read_only_clients_and_hides_new_property_shapes() {
    let directory = source();
    for (minor, writable) in [(48, true), (49, false)] {
        let mut client = Client::new("en-US");
        let hello = initialize(&mut client, minor, writable);
        let open = client.open(&directory.0);
        if minor == 48 {
            assert!(hello["capabilities"].get("metadataObjects").is_none());
            assert_eq!(
                client.request("metadata/objectSchema", context(&open, json!({})))["error"]["code"],
                -32601
            );
        } else {
            assert_eq!(hello["capabilities"]["metadataObjects"]["enabled"], false);
            let schema = client.ok("metadata/objectSchema", context(&open, json!({})));
            assert_eq!(schema["operations"], json!([]));
            for method in ["metadata/applyObject", "metadata/applyFormEdit"] {
                assert_eq!(
                    client.request(method, context(&open, json!({"prepared":{}})))["error"]["data"]
                        ["kind"],
                    "property_read_only"
                );
            }
        }
        client.ok(
            "metadata/creationSchema",
            context(&open, json!({"owner":"document:Продажа"})),
        );
        let schema = client.ok(
            "metadata/propertyEditing",
            context(&open, json!({"objectId":"document:Продажа"})),
        );
        let has_references = schema["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["schema"]["kind"] == "references");
        assert_eq!(has_references, minor >= 49);
        client.finish();
    }
}
