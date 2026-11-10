use super::*;

#[test]
fn reference_branches_are_negotiated_bounded_and_retained_on_refresh() {
    let directory = fixture();
    let source = directory.0.join("src");
    fs::create_dir(source.join("Catalogs")).unwrap();
    fs::write(source.join("Catalogs/Company.xml"), r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"><Catalog uuid="company"><Properties><Name>Company</Name></Properties><ChildObjects><Attribute uuid="value"><Properties><Name>Value</Name><Type xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:xs="http://www.w3.org/2001/XMLSchema"><v8:Type>xs:string</v8:Type></Type></Properties></Attribute></ChildObjects></Catalog></MetaDataObject>"#).unwrap();
    let config = source.join("Configuration.xml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace("<ChildObjects>", "<ChildObjects><Catalog>Company</Catalog>"),
    )
    .unwrap();
    let xml = r#"<Form xmlns="http://v8.1c.ru/8.3/xcf/logform"><Attributes><Attribute name="Link" id="1"><Type xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config"><v8:Type>cfg:CatalogRef.Company</v8:Type></Type></Attribute></Attributes></Form>"#;
    fs::write(source.join(PATH), xml).unwrap();
    for minor in [46, 47] {
        let mut client = Client::new(&directory.0, "ru-RU");
        let (context, node) = connect(&mut client, &directory.0, "ru-RU", minor);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        assert_eq!(opened["model"]["dataFields"], json!([]));
        let update = params(
            &context,
            json!({"overlays":[],"formSessionId":opened["formSessionId"],"revision":opened["revision"],"expandedData":[["Link"]]}),
        );
        if minor == 46 {
            assert_eq!(
                client.response("form/update", &update)["error"]["code"],
                -32602
            );
            assert!(opened["model"].get("expandedData").is_none());
            client.finish();
            continue;
        }
        let updated = client.request("form/update", &update);
        assert_eq!(updated["model"]["expandedData"], json!([["Link"]]));
        assert!(
            updated["model"]["dataFields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field["path"] == json!(["Link", "Value"]))
        );
        let refresh = params(
            &context,
            json!({"overlays":[],"formSessionId":updated["formSessionId"],"revision":updated["revision"]}),
        );
        let refreshed = client.request("form/update", &refresh);
        assert_eq!(refreshed["model"], updated["model"]);
        for paths in [
            json!([["Link.Value"]]),
            json!([[]]),
            json!([[""]]),
            json!([vec!["Link"; 65]]),
            json!([["x".repeat(2049)]]),
        ] {
            let invalid = params(
                &context,
                json!({"overlays":[],"formSessionId":refreshed["formSessionId"],"revision":refreshed["revision"],"expandedData":paths}),
            );
            assert_eq!(
                client.response("form/update", &invalid)["error"]["code"],
                -32602
            );
        }
        let over_limit = params(
            &context,
            json!({"node":node,"expandedData":vec![vec!["Link"];129]}),
        );
        assert!(
            client
                .response("form/open", &over_limit)
                .get("error")
                .is_some()
        );
        client.finish();
    }
    assert_eq!(fs::read_to_string(source.join(PATH)).unwrap(), xml);
}
