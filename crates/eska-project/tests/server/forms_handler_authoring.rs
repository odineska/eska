use super::*;

#[test]
fn event_authoring_plans_missing_module_and_regions_without_writing_and_is_idempotent() {
    for (locale, script, expected) in [
        ("ru-RU", "English", "OnOpen"),
        ("en-US", "Russian", "ПриОткрытии"),
    ] {
        let directory = fixture();
        let config = directory.0.join("src/Configuration.xml");
        fs::write(
            &config,
            fs::read_to_string(&config).unwrap().replace(
                "</Properties>",
                &format!("<ScriptVariant>{script}</ScriptVariant></Properties>"),
            ),
        )
        .unwrap();
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 54);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let request = json!({"formSessionId":opened["formSessionId"],"revision":"1","target":{"kind":"root"},"event":"OnOpen"});
        let source = client.request("form/handlerEditSource", &params(&context, request.clone()));
        assert_eq!(source["exists"], false);
        assert_eq!(source["name"], expected);
        assert_eq!(
            source_path(&source),
            Path::new("CommonForms/Preview/Ext/Form/Module.bsl")
        );
        let text = "\u{feff}// 😀 user comment\r\n#Region FormEventHandlers\r\n#EndRegion\n#Region Private\nProcedure Other()\r\nEndProcedure\n#EndRegion\r\n";
        let mut plan_request = request.clone();
        plan_request["input"] = json!({"source":source["source"],"version":"4","text":text});
        let plan = client.request(
            "form/handlerEditPlan",
            &params(&context, plan_request.clone()),
        );
        assert_eq!(plan["snapshot"], snapshot(text));
        let at = usize::try_from(plan["change"]["range"]["start"].as_u64().unwrap()).unwrap();
        assert_eq!(plan["change"]["range"]["end"], at);
        let addition = plan["change"]["text"].as_str().unwrap();
        assert!(addition.contains(&format!(
            "{expected}({})\r\n",
            if script == "English" {
                "Cancel"
            } else {
                "Отказ"
            }
        )));
        let output = format!("{}{}{}", &text[..at], addition, &text[at..]);
        assert_eq!(plan["outputSnapshot"], snapshot(&output));
        plan_request["input"]["text"] = json!(output);
        let repeated = client.request(
            "form/handlerEditPlan",
            &params(&context, plan_request.clone()),
        );
        assert!(repeated["change"].is_null());
        assert_eq!(repeated["range"], plan["range"]);
        plan_request["input"]["source"] = json!("a".repeat(64));
        assert_eq!(
            client.response("form/handlerEditPlan", &params(&context, plan_request))["error"]["data"]
                ["details"]["reason"],
            "foreign_handler_input"
        );
        let mut stale = request.clone();
        stale["revision"] = json!("2");
        assert_eq!(
            client.response("form/handlerEditSource", &params(&context, stale))["error"]["data"]["details"]
                ["reason"],
            "stale_form_revision"
        );
        let mut unknown = request;
        unknown["event"] = json!("ImaginaryEvent");
        assert_eq!(
            client.response("form/handlerEditSource", &params(&context, unknown))["error"]["data"]
                ["details"]["reason"],
            "handler_unavailable"
        );
        client.finish();
        assert!(
            !directory
                .0
                .join("src/CommonForms/Preview/Ext/Form/Module.bsl")
                .exists()
        );
        assert_eq!(
            fs::read_to_string(directory.0.join("src").join(PATH)).unwrap(),
            INPUT
        );
    }
}

#[test]
fn event_authoring_uses_the_dynamic_list_server_signature_and_table_region() {
    let directory = fixture();
    let xml = r#"<Form xmlns="http://v8.1c.ru/8.3/xcf/logform"><ChildItems><Table name="Список" id="1"><DataPath>Список</DataPath><Events><Event name="OnGetDataAtServer">СписокПриПолученииДанныхНаСервере</Event></Events><ChildItems><InputField name="СписокНомер" id="2"><Events><Event name="OnChange">СписокНомерПриИзменении</Event></Events></InputField></ChildItems></Table></ChildItems></Form>"#;
    fs::write(directory.0.join("src").join(PATH), xml).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 54);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    for (id, event, parameters, directive) in [
        (
            1,
            "OnGetDataAtServer",
            "ИмяЭлемента, Настройки, Строки",
            "&НаСервереБезКонтекста",
        ),
        (2, "OnChange", "Элемент", "&НаКлиенте"),
    ] {
        let mut request = json!({"formSessionId":opened["formSessionId"],"revision":"1","target":{"kind":"element","id":id},"event":event});
        let source = client.request("form/handlerEditSource", &params(&context, request.clone()));
        request["input"] = json!({"source":source["source"],"version":"new","text":""});
        let plan = client.request("form/handlerEditPlan", &params(&context, request));
        let text = plan["change"]["text"].as_str().unwrap();
        assert!(text.contains("#Область ОбработчикиСобытийЭлементовТаблицыФормыСписок"));
        assert!(text.contains(directive));
        assert!(text.contains(&format!("({parameters})")));
    }
    client.finish();
}

#[test]
fn event_authoring_is_not_advertised_to_older_clients() {
    let directory = fixture();
    let mut client = Client::new(&directory.0, "en-US");
    let (context, node) = connect(&mut client, &directory.0, "en-US", 53);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    for method in ["form/handlerEditSource", "form/handlerEditPlan"] {
        assert_eq!(client.response(method, &params(&context, json!({"formSessionId":opened["formSessionId"],"revision":"1","target":{"kind":"root"},"event":"OnOpen"})))["error"]["code"], -32601);
    }
    client.finish();
}
