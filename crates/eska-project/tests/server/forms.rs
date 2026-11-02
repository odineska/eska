use super::*;
use eska_project_core::project::metadata_edit::snapshot;

const PATH: &str = "CommonForms/Preview/Ext/Form.xml";
const INPUT: &str = "\u{feff}<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\"><ChildItems><InputField name=\"Поле\" id=\"9223372036854775807\"><Title>Исходный</Title></InputField></ChildItems></Form>\r\n";
fn fixture() -> support::TestDir {
    let directory = support::TestDir::new();
    fs::write(
        directory.0.join("eska.toml"),
        "[project]\ntype='configuration'\n",
    )
    .unwrap();
    fs::create_dir(directory.0.join("src")).unwrap();
    fs::write(directory.0.join("src/Configuration.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Configuration uuid=\"root\"><Properties><Name>Fixture</Name></Properties><ChildObjects><CommonForm>Preview</CommonForm></ChildObjects></Configuration></MetaDataObject>").unwrap();
    fs::create_dir_all(directory.0.join("src/CommonForms/Preview/Ext")).unwrap();
    fs::write(directory.0.join("src/CommonForms/Preview.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><CommonForm uuid=\"form\"><Properties><Name>Preview</Name></Properties></CommonForm></MetaDataObject>").unwrap();
    fs::write(directory.0.join("src").join(PATH), INPUT).unwrap();
    directory
}
fn connect(client: &mut Client, root: &Path, locale: &str, minor: u32) -> (Value, Value) {
    let hello = client.request("initialize", &json!({"apiVersion":{"major":1,"minor":minor},"client":{"name":"forms-test","version":"1"},"locale":locale}));
    assert_eq!(
        hello["capabilities"]["formPreview"].is_object(),
        minor >= 36
    );
    let opened = client.request("workspace/open", &json!({"start":{"value":root,"encoding":"utf-8"},"selection":{"kind":"current"},"diskCache":false}));
    let project = &opened["projects"][0];
    let context = json!({"sessionId":opened["sessionId"],"projectId":project["projectId"],"generation":project["generation"]});
    let mut input = context.clone();
    input["path"] = json!({"value":PATH,"encoding":"utf-8"});
    let revealed = client.request("metadata/revealSource", &input);
    let node = revealed["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["metadataKind"] == "common-form")
        .unwrap()["id"]
        .clone();
    (context, node)
}
fn params(context: &Value, values: Value) -> Value {
    let mut value = context.clone();
    let Value::Object(values) = values else {
        panic!("fixture parameters");
    };
    value.as_object_mut().unwrap().extend(values);
    value
}

#[test]
fn form_view_protocol_is_read_only_versioned_locale_independent_and_keeps_last_valid_model() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    let mut models = Vec::new();
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 36);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        assert_eq!(opened["status"], "ready");
        assert_eq!(opened["modelComplete"], true);
        assert_eq!(
            opened["model"]["root"]["children"][0]["identity"]["key"]["value"],
            "9223372036854775807"
        );
        models.push(opened["model"].clone());
        let id = opened["formSessionId"].clone();
        let source = opened["dependencies"][0]["id"].clone();
        let changed = client.request("form/update", &params(&context, json!({"formSessionId":id,"revision":"1","overlays":[{"source":source,"version":"7","text":INPUT.replace("Исходный","Несохранённый")}]})));
        assert_eq!(changed["revision"], "2");
        assert_eq!(
            changed["model"]["root"]["children"][0]["presentation"]["caption"]["text"],
            "Несохранённый"
        );
        let target = json!({"kind":"element","identity":changed["model"]["root"]["children"][0]["identity"]});
        let location = client.request(
            "form/locate",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"2","target":target}),
            ),
        );
        assert_eq!(location["source"], source);
        assert_eq!(
            location["snapshot"],
            snapshot(&INPUT.replace("Исходный", "Несохранённый"))
        );
        let invalid = client.response("form/update", &params(&context, json!({"formSessionId":id,"revision":"2","overlays":[{"source":source,"version":"8","text":"<Form"}]})));
        assert_eq!(invalid["error"]["data"]["details"]["reason"], "invalid_xml");
        let retained = client.request("form/get", &params(&context, json!({"formSessionId":id})));
        assert_eq!(retained["status"], "stale");
        assert_eq!(retained["revision"], "2");
        assert_eq!(retained["model"], changed["model"]);
        let stale = client.response(
            "form/locate",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"2","target":target}),
            ),
        );
        assert_eq!(
            stale["error"]["data"]["details"]["reason"],
            "stale_form_model"
        );
        let foreign = client.response("form/update", &params(&context, json!({"formSessionId":id,"revision":"2","overlays":[{"source":"unrelated","version":"9","text":INPUT}]})));
        assert_eq!(
            foreign["error"]["data"]["details"]["reason"],
            "foreign_form_input"
        );
        let restored = client.request(
            "form/update",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"2","overlays":[]}),
            ),
        );
        assert_eq!(restored["status"], "ready");
        assert_eq!(restored["revision"], "3");
        assert_eq!(restored["model"], opened["model"]);
        let closed = client.request("form/close", &params(&context, json!({"formSessionId":id})));
        assert_eq!(closed["released"], true);
        assert_eq!(
            client.request("form/close", &params(&context, json!({"formSessionId":id})))["released"],
            false
        );
        client.finish();
        assert_eq!(fs::read_to_string(&path).unwrap(), INPUT);
    }
    assert_eq!(models[0], models[1]);
}

#[test]
fn form_overlay_accepts_existing_chunk_upload_and_external_saved_changes() {
    let directory = fixture();
    let mut client = Client::new(&directory.0, "ru");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 36);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let id = opened["formSessionId"].clone();
    let text = INPUT.replace("Исходный", "Буфер") + &format!("<!--{}-->", "x".repeat(150_000));
    let begun = client.request(
        "query/bufferBegin",
        &params(
            &context,
            json!({"bytes":text.len(),"snapshot":snapshot(&text)}),
        ),
    );
    let buffer = begun["bufferId"].clone();
    client.request(
        "query/bufferAppend",
        &params(
            &context,
            json!({"bufferId":buffer,"offset":0,"text":&text[..100_000]}),
        ),
    );
    client.request(
        "query/bufferAppend",
        &params(
            &context,
            json!({"bufferId":buffer,"offset":100_000,"text":&text[100_000..]}),
        ),
    );
    let changed = client.request("form/update", &params(&context,json!({"formSessionId":id,"revision":"1","overlays":[{"source":opened["dependencies"][0]["id"],"version":"2","bufferId":buffer}]})));
    assert_eq!(changed["dependencies"][0]["snapshot"], snapshot(&text));
    client.request(
        "query/bufferClose",
        &params(&context, json!({"bufferId":buffer})),
    );
    let external = INPUT.replace("Исходный", "Сохранённый внешний");
    fs::write(directory.0.join("src").join(PATH), &external).unwrap();
    let saved = client.request(
        "form/update",
        &params(
            &context,
            json!({"formSessionId":id,"revision":"2","overlays":[]}),
        ),
    );
    assert_eq!(saved["dependencies"][0]["snapshot"], snapshot(&external));
    client.finish();
}

#[test]
fn form_methods_are_not_exposed_to_older_clients() {
    let directory = fixture();
    let mut client = Client::new(&directory.0, "ru");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 35);
    assert_eq!(
        client.response("form/open", &params(&context, json!({"node":node})))["error"]["code"],
        -32601
    );
    client.finish();
}

#[test]
fn common_picture_payloads_are_bounded_images_and_binary_dependencies_require_api_37() {
    let directory = fixture();
    let root = directory.0.join("src");
    fs::create_dir_all(root.join("CommonPictures/Icon/Ext/Picture")).unwrap();
    fs::write(root.join("CommonPictures/Icon.xml"), "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><CommonPicture uuid=\"picture\"><Properties><Name>Icon</Name></Properties></CommonPicture></MetaDataObject>").unwrap();
    fs::write(root.join("CommonPictures/Icon/Ext/Picture.xml"),"<ExtPicture xmlns=\"http://v8.1c.ru/8.3/xcf/extrnprops\" xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\"><Picture><xr:Abs>Icon.svg</xr:Abs></Picture></ExtPicture>").unwrap();
    let bytes = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>";
    fs::write(root.join("CommonPictures/Icon/Ext/Picture/Icon.svg"), bytes).unwrap();
    fs::write(root.join(PATH),"<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\"><ChildItems><PictureDecoration name=\"Logo\" id=\"1\"><Picture xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\"><xr:Ref>CommonPicture.Icon</xr:Ref></Picture></PictureDecoration></ChildItems></Form>").unwrap();
    for minor in [36, 37] {
        let mut client = Client::new(&directory.0, "ru-RU");
        let (context, node) = connect(&mut client, &directory.0, "ru-RU", minor);
        let view = client.request("form/open", &params(&context, json!({"node":node})));
        assert_eq!(view["modelComplete"], true);
        assert_eq!(
            view["model"]["assets"][0]["picture"]["mimeType"],
            "image/svg+xml"
        );
        let binaries = view["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|input| input["role"] == "picture")
            .collect::<Vec<_>>();
        assert_eq!(binaries.len(), usize::from(minor >= 37));
        if minor >= 37 {
            let rejected=client.response("form/update",&params(&context,json!({"formSessionId":view["formSessionId"],"revision":view["revision"],"overlays":[{"source":binaries[0]["id"],"version":"1","text":"not binary"}]})));
            assert_eq!(
                rejected["error"]["data"]["details"]["reason"],
                "foreign_form_input"
            );
        }
        client.finish();
    }
}
