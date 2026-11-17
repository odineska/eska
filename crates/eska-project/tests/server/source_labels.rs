use super::*;

/// SCM captions remain negotiated, locale-independent and generation-checked without a full index.
#[test]
fn source_labels_protocol_is_bounded_and_versioned() {
    let fixture = support::TestDir::new();
    let root = fixture.0.join("source-labels");
    create::create(&root, &ProjectConfig::new(ProjectType::Configuration)).unwrap();
    fs::write(root.join("src/Configuration.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Configuration uuid=\"root\"><Properties><Name>Пример</Name></Properties><ChildObjects/></Configuration></MetaDataObject>").unwrap();
    let mut captions = Vec::new();
    for minor in [53, 54] {
        for locale in ["ru", "en"] {
            let mut client = Client::new(&root, locale);
            let hello = client.request("initialize", &json!({"apiVersion":{"major":1,"minor":minor},"client":{"name":"source-labels","version":"1"}}));
            assert_eq!(
                hello["capabilities"]["sourceLabels"],
                if minor == 54 {
                    json!(true)
                } else {
                    Value::Null
                }
            );
            let opened = client.request("workspace/open", &json!({"start":{"value":root,"encoding":"utf-8"},"selection":{"kind":"current"},"diskCache":false}));
            let project = &opened["projects"][0];
            let context = json!({"sessionId":opened["sessionId"],"projectId":project["projectId"],"generation":project["generation"]});
            let mut params = context.clone();
            params["paths"] = json!([
                {"value":"Documents/ВыдачаЗайма/Ext/ObjectModule.bsl","encoding":"utf-8"},
                {"value":"unknown.txt","encoding":"utf-8"}
            ]);
            if minor < 54 {
                assert_eq!(
                    client.response("metadata/sourceLabels", &params)["error"]["code"],
                    -32601
                );
            } else {
                let result = client.request("metadata/sourceLabels", &params);
                assert_eq!(
                    result["labels"][0]["translations"]["ru-RU"],
                    "Документ.ВыдачаЗайма.МодульОбъекта"
                );
                assert!(result["labels"][1]["translations"].is_null());
                assert_eq!(result["generation"], context["generation"]);
                captions.push(result["labels"].clone());
                params["paths"] = json!([{"value":"../outside.xml","encoding":"utf-8"}]);
                assert_eq!(
                    client.response("metadata/sourceLabels", &params)["error"]["code"],
                    -32602
                );
                params["paths"] = json!(vec![
                    json!({"value":"Configuration.xml","encoding":"utf-8"});
                    513
                ]);
                assert_eq!(
                    client.response("metadata/sourceLabels", &params)["error"]["code"],
                    -32602
                );
                params["paths"] = json!([]);
                assert_eq!(
                    client.request("metadata/sourceLabels", &params)["labels"],
                    json!([])
                );
                let mut status = context.clone();
                status["action"] = json!("status");
                assert_eq!(
                    client.request("metadata/index", &status)["progress"]["state"],
                    "not_started"
                );
                let mut refresh = context;
                refresh["node"] = project["root"].clone();
                client.request("metadata/refresh", &refresh);
                assert_eq!(
                    client.response("metadata/sourceLabels", &params)["error"]["data"]["kind"],
                    "stale_generation"
                );
            }
            client.finish();
        }
    }
    assert_eq!(captions[0], captions[1]);
    assert!(
        !root.join("src/Documents").exists(),
        "captions do not require existing source files"
    );
}
