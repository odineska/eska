use super::{Client, TestDir, context, json};
use crate::project::metadata_workspace::standard_attributes::source;

#[test]
fn process_standard_attributes_are_localized_read_only_and_version_gated() {
    let directory = TestDir::new();
    source(
        &directory.0,
        "<CodeLength>9</CodeLength><CodeType>Number</CodeType><StandardAttributes><r:StandardAttribute name='Code'><r:FillFromFillingValue>true</r:FillFromFillingValue><r:Comment>Saved</r:Comment></r:StandardAttribute></StandardAttributes>",
    );
    let owner = json!({"kind":"object", "objectId":"catalog:Goods"});
    let group = json!({"kind":"collection","owner":"catalog:Goods","collection":{"kind":"standardAttributes"}});
    let code = json!({"kind":"standardAttribute","owner":"catalog:Goods","name":"Code"});
    let mut expected = None;
    for minor in [53, 54] {
        for locale in ["ru-RU", "en-US"] {
            let mut client = Client::new(locale);
            let handshake = client.ok("initialize", json!({"apiVersion":{"major":1,"minor":minor},"client":{"name":"standard-attribute-test","version":"1"},"locale":locale}));
            assert_eq!(
                handshake["capabilities"]["standardAttributes"].as_bool(),
                (minor >= 54).then_some(true)
            );
            let open = client.open(&directory.0);
            let children = client.ok("metadata/children", context(&open, json!({"node":owner})));
            assert_eq!(
                children["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["id"] == group),
                minor >= 54
            );
            if minor < 54 {
                let denied =
                    client.request("metadata/children", context(&open, json!({"node":group})));
                assert_eq!(denied["error"]["code"], -32602);
                let denied =
                    client.request("metadata/properties", context(&open, json!({"node":code})));
                assert_eq!(denied["error"]["code"], -32602);
            } else {
                let children =
                    client.ok("metadata/children", context(&open, json!({"node":group})));
                let field = children["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|node| node["id"] == code)
                    .unwrap();
                assert_eq!(field["label"]["translations"]["ru-RU"], "Код");
                assert_eq!(field["label"]["translations"]["en-US"], "Code");
                assert!(field.get("creationKinds").is_none());
                assert!(field.get("objectCreation").is_none());
                let properties =
                    client.ok("metadata/properties", context(&open, json!({"node":code})));
                assert_eq!(properties["readOnly"], true);
                assert_eq!(properties["sourceAvailable"], false);
                assert_eq!(
                    properties["properties"][0]["value"]["caption"]["ru-RU"],
                    "Код"
                );
                assert_eq!(
                    properties["properties"][1]["presentation"]["items"][0]["caption"]["ru-RU"],
                    "Число"
                );
                let boolean = properties["properties"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|value| value["key"]["name"] == "FillFromFillingValue")
                    .unwrap();
                assert_eq!(boolean["value"]["scalarType"], "boolean");
                if let Some(expected) = &expected {
                    assert_eq!(expected, &properties["properties"]);
                }
                expected = Some(properties["properties"].clone());
                let no_source =
                    client.request("metadata/source", context(&open, json!({"node":code})));
                assert_eq!(no_source["error"]["data"]["kind"], "source_missing");
                let no_editing = client.request(
                    "metadata/propertyEditing",
                    context(&open, json!({"node":code})),
                );
                assert_eq!(no_editing["error"]["code"], -32602);
            }
            client.finish();
        }
    }
}

#[test]
fn process_numbered_standard_attributes_have_shared_indexed_captions_and_types() {
    let directory = TestDir::new();
    crate::project::metadata_workspace::standard_attributes::accounting::fixture(&directory.0, 2);
    let owner = json!({"kind":"object", "objectId":"accounting-register:Entries"});
    let group = json!({"kind":"collection", "owner":"accounting-register:Entries", "collection":{"kind":"standardAttributes"}});
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(locale);
        client.ok("initialize", json!({"apiVersion":{"major":1,"minor":54},"client":{"name":"numbered-standard-attributes","version":"1"},"locale":locale}));
        let open = client.open(&directory.0);
        client.ok("metadata/children", context(&open, json!({"node":owner})));
        let children = client.ok("metadata/children", context(&open, json!({"node":group})));
        for (name, ru, en) in [
            ("ExtDimension2", "Субконто 2", "Extra dimension 2"),
            (
                "ExtDimensionType2",
                "Вид субконто 2",
                "Extra dimension type 2",
            ),
        ] {
            let id = json!({"kind":"standardAttribute", "owner":"accounting-register:Entries", "name":name});
            let node = children["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == id)
                .unwrap();
            assert_eq!(
                node["label"]["translations"]["ru-RU"]
                    .as_str()
                    .unwrap()
                    .replace(['\u{2068}', '\u{2069}'], ""),
                ru
            );
            assert_eq!(
                node["label"]["translations"]["en-US"]
                    .as_str()
                    .unwrap()
                    .replace(['\u{2068}', '\u{2069}'], ""),
                en
            );
            let properties = client.ok("metadata/properties", context(&open, json!({"node":id})));
            assert_eq!(
                properties["properties"][0]["value"]["caption"],
                node["label"]["translations"]
            );
            assert_eq!(properties["readOnly"], true);
            assert_eq!(properties["sourceAvailable"], false);
            assert_ne!(
                properties["properties"][1]["presentation"]["items"]
                    .as_array()
                    .unwrap()
                    .as_slice(),
                &[] as &[serde_json::Value]
            );
        }
        client.finish();
    }
}
