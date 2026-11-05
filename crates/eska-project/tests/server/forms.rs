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
    if minor >= 36 {
        assert_eq!(
            hello["capabilities"]["formPreview"]["inputPreparation"],
            minor >= 38
        );
        assert_eq!(
            hello["capabilities"]["formPreview"]["propertyPalette"],
            if minor >= 41 {
                json!(true)
            } else {
                Value::Null
            }
        );
    }
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

const GROUP: &str =
    include_str!("../../../../tests/fixtures/forms/native-property-palette/UsualGroup.xml");
const GROUP_OVERRIDES: &str = include_str!(
    "../../../../tests/fixtures/forms/native-property-palette/UsualGroupOverrides.xml"
);
const BOUND_INPUT: &str =
    include_str!("../../../../tests/fixtures/forms/native-bound-fields/InputField.xml");
const TABLE_INPUTS: &str =
    include_str!("../../../../tests/fixtures/forms/native-tables/Default.xml");
const NATIVE_GOODS: &str =
    include_str!("../../../../tests/fixtures/forms/native-context-tables/Catalog.xml");
const INPUT_PALETTE: &str =
    include_str!("../../../../tests/fixtures/forms/native-property-palette/input-field.json");

fn inspect_group(client: &mut Client, context: &Value, snapshot: &Value) -> Value {
    client.request(
        "form/inspect",
        &params(
            context,
            json!({
                "formSessionId": snapshot["formSessionId"],
                "revision": snapshot["revision"],
                "identity": snapshot["model"]["root"]["children"][0]["identity"]
            }),
        ),
    )
}

fn assert_native_row(row: &Value, expected: &Value, locale: &str) {
    let id = expected["nativeKey"].as_str().unwrap_or("UserVisibility");
    assert_eq!(row["id"], format!("platform:{id}"));
    assert_eq!(row["caption"], expected["label"]);
    assert_eq!(row["category"], expected["category"]);
    assert_eq!(row["editor"], expected["editor"]);
    let kind = match expected["sourcePresence"].as_str().unwrap() {
        "attribute" => "name",
        "element-kind" => "elementKind",
        "property" => "declared",
        "absent" => "absent",
        other => panic!("unrecognized native source {other}"),
    };
    let value = &row["value"];
    assert_eq!(value["kind"], kind);
    let display = &expected["displayValue"];
    match kind {
        "elementKind" => assert_eq!(value["caption"], *display),
        "absent" => match display["en-US"].as_str() {
            Some("") => assert_eq!(value["default"]["kind"], "empty"),
            Some(_) => assert_eq!(value["caption"], *display),
            None => assert_eq!(value["default"]["value"], display[locale]),
        },
        _ => {}
    }
}

fn inspect_input(
    client: &mut Client,
    context: &Value,
    opened: &Value,
    field: &Value,
    include_events: bool,
) -> Value {
    client.request(
        "form/inspect",
        &params(
            context,
            json!({
                "formSessionId":opened["formSessionId"],"revision":opened["revision"],
                "identity":field["identity"],"includeEventRows":include_events
            }),
        ),
    )
}

fn update_input(
    client: &mut Client,
    context: &Value,
    opened: &Value,
    version: u32,
    text: &str,
) -> Value {
    client.request(
        "form/update",
        &params(
            context,
            json!({
                "formSessionId":opened["formSessionId"],"revision":opened["revision"],
                "overlays":[{"source":opened["dependencies"][0]["id"],"version":version.to_string(),"text":text}]
            }),
        ),
    )
}

fn input_without_title() -> String {
    let title_start = BOUND_INPUT.find("<Title>\n\t\t\t\t<v8:item>").unwrap();
    let title_end =
        title_start + BOUND_INPUT[title_start..].find("</Title>").unwrap() + "</Title>".len();
    [&BOUND_INPUT[..title_start], &BOUND_INPUT[title_end..]].concat()
}

fn with_form_type(xml: &str, owner: &str, types: &str) -> String {
    let owner = xml.find(owner).unwrap();
    let start = owner + xml[owner..].find("<Type>").unwrap();
    let end = start + xml[start..].find("</Type>").unwrap() + "</Type>".len();
    format!("{}<Type>{types}</Type>{}", &xml[..start], &xml[end..])
}

#[test]
fn form_input_palette_agrees_with_scalar_and_column_contexts_on_dirty_revisions() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    let baseline = input_without_title();
    fs::write(&path, &baseline).unwrap();
    let native: Value = serde_json::from_str(INPUT_PALETTE).unwrap();
    for locale in ["en-US", "ru-RU"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 41);
        let mut opened = client.request("form/open", &params(&context, json!({"node":node})));
        let identity = opened["model"]["root"]["children"][0]["identity"].clone();
        let mut version = 2;
        for (case, raw_type) in [
            ("string", "xs:string"),
            ("number", "xs:decimal"),
            ("date", "xs:dateTime"),
            ("boolean", "xs:boolean"),
        ] {
            if raw_type != "xs:string" {
                let text = with_form_type(
                    &baseline,
                    r#"<Attribute name="Value""#,
                    &format!("<v8:Type>{raw_type}</v8:Type>"),
                );
                opened = update_input(&mut client, &context, &opened, version, &text);
                version += 1;
            }
            let field = &opened["model"]["root"]["children"][0];
            assert_eq!(field["identity"], identity);
            assert_eq!(field["presentation"]["valueType"]["kind"], case);
            let inspected = inspect_input(&mut client, &context, &opened, field, true);
            assert_input_palette(&inspected, &native, case, locale);
        }
        opened = update_input(&mut client, &context, &opened, version, TABLE_INPUTS);
        for (index, case) in ["tableString", "tableNumber"].into_iter().enumerate() {
            let field = &opened["model"]["root"]["children"][0]["children"][index];
            let inspected = inspect_input(&mut client, &context, &opened, field, true);
            assert_input_palette(&inspected, &native, case, locale);
        }
        client.finish();
    }
    assert_eq!(fs::read_to_string(path).unwrap(), baseline);
}

#[test]
fn form_input_palette_agrees_with_reference_list_and_additional_column_contexts() {
    let directory = fixture();
    let src = directory.0.join("src");
    let path = src.join(PATH);
    let baseline = input_without_title();
    fs::write(&path, &baseline).unwrap();
    fs::create_dir(src.join("Catalogs")).unwrap();
    fs::write(src.join("Catalogs/NativeGoods.xml"), NATIVE_GOODS).unwrap();
    let configuration = src.join("Configuration.xml");
    let descriptor = fs::read_to_string(&configuration).unwrap().replace(
        "<ChildObjects>",
        "<ChildObjects><Catalog>NativeGoods</Catalog>",
    );
    fs::write(configuration, descriptor).unwrap();
    let native: Value = serde_json::from_str(INPUT_PALETTE).unwrap();
    let table = with_form_type(
        TABLE_INPUTS,
        r#"<Column name="Text""#,
        "<v8:Type>xs:dateTime</v8:Type><v8:DateQualifiers><v8:DateFractions>DateTime</v8:DateFractions></v8:DateQualifiers>",
    );
    let table = with_form_type(
        &table,
        r#"<Column name="Amount""#,
        "<v8:Type>xs:boolean</v8:Type>",
    );
    for locale in ["en-US", "ru-RU"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 41);
        let mut opened = client.request("form/open", &params(&context, json!({"node":node})));
        let identity = opened["model"]["root"]["children"][0]["identity"].clone();
        let mut version = 2;
        for (case, types) in [
            ("unbound", ""),
            (
                "mixed",
                "<v8:Type>xs:string</v8:Type><v8:Type>xs:decimal</v8:Type>",
            ),
            ("reference", "<v8:Type>cfg:CatalogRef.NativeGoods</v8:Type>"),
            (
                "mixedReference",
                "<v8:Type>xs:string</v8:Type><v8:Type>cfg:CatalogRef.NativeGoods</v8:Type>",
            ),
            ("valueList", "<v8:Type>v8:ValueListType</v8:Type>"),
        ] {
            let text = if types.is_empty() {
                baseline.replace("<DataPath>Value</DataPath>", "")
            } else {
                with_form_type(&baseline, r#"<Attribute name="Value""#, types)
            };
            opened = update_input(&mut client, &context, &opened, version, &text);
            version += 1;
            let field = &opened["model"]["root"]["children"][0];
            assert_eq!(field["identity"], identity);
            let inspected = inspect_input(&mut client, &context, &opened, field, true);
            assert_input_palette(&inspected, &native, case, locale);
        }
        opened = update_input(&mut client, &context, &opened, version, &table);
        for (index, case) in ["tableDate", "tableBoolean"].into_iter().enumerate() {
            let field = &opened["model"]["root"]["children"][0]["children"][index];
            let inspected = inspect_input(&mut client, &context, &opened, field, true);
            assert_input_palette(&inspected, &native, case, locale);
        }
        client.finish();
    }
    assert_eq!(fs::read_to_string(path).unwrap(), baseline);
}

fn assert_input_palette(inspected: &Value, native: &Value, case: &str, locale: &str) {
    let expected = native["rows"].as_array().unwrap().iter().filter(|row| {
        row["contexts"]
            .as_array()
            .is_none_or(|contexts| contexts.iter().any(|context| context == case))
    });
    let rows = inspected["rows"].as_array().unwrap();
    assert_eq!(json!(rows.len()), native["rowCounts"][case], "{case}");
    for (row, expected) in rows.iter().zip(expected) {
        assert_native_row(row, expected, locale);
    }
    let categories = inspected["categories"].as_array().unwrap();
    assert_eq!(categories.len(), 5);
    for (category, id) in categories
        .iter()
        .zip(["main", "usage", "appearance", "layout", "events"])
    {
        assert_eq!(category["id"], id);
        assert_eq!(category["caption"], native["categoryLabels"][id]);
    }
}

fn input_palette_value<'a>(inspection: &'a Value, key: &str) -> &'a Value {
    let id = format!("platform:{key}");
    &inspection["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap()["value"]
}

#[test]
fn form_input_palette_preserves_declared_values_and_size_overrides() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    let changed = BOUND_INPUT.replace(
        "</InputField>",
        r#"<TitleLocation>Top</TitleLocation><MultiLine>true</MultiLine>
<GroupHorizontalAlign>Left</GroupHorizontalAlign><TextColor>web:Blue</TextColor>
<Font ref="sys:DefaultGUIFont" height="14" bold="true" kind="WindowsFont"/>
<OnMainServerUnavalableBehavior>MakeDisable</OnMainServerUnavalableBehavior>
<AutoShowClearButtonMode>Always</AutoShowClearButtonMode><AutoShowOpenButtonMode>Always</AutoShowOpenButtonMode>
<AutoMaxWidth>false</AutoMaxWidth><AutoMaxHeight>false</AutoMaxHeight><MaxHeight>0</MaxHeight>
<ReadOnly>false</ReadOnly><Width>0</Width><InputHint/></InputField>"#,
    );
    fs::write(&path, BOUND_INPUT).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let opened = update_input(&mut client, &context, &opened, 2, &changed);
    let input = &opened["model"]["root"]["children"][0];
    let inspected = inspect_input(&mut client, &context, &opened, input, true);
    let row = |key: &str| input_palette_value(&inspected, key);
    for (id, expected) in [
        ("TitleLocation", json!({"ru-RU":"Верх","en-US":"Top"})),
        ("MultiLine", json!({"ru-RU":"Да","en-US":"Yes"})),
        (
            "OnMainServerUnavailableBehavior",
            json!({"ru-RU":"Отключать доступность","en-US":"Disable availability"}),
        ),
        (
            "AutoShowClearButton",
            json!({"ru-RU":"Всегда","en-US":"Always"}),
        ),
        (
            "AutoShowOpenButton",
            json!({"ru-RU":"Всегда","en-US":"Always"}),
        ),
    ] {
        assert_eq!(row(id)["caption"], expected);
        assert_eq!(row(id)["kind"], "declared");
    }
    let property = |value: &Value| {
        let index = usize::try_from(value["propertyIndex"].as_u64().unwrap()).unwrap();
        &input["properties"][index]
    };
    for (id, key, text) in [
        ("HorizontalAlignInGroup", "GroupHorizontalAlign", "Left"),
        ("ReadOnly", "ReadOnly", "false"),
        ("Width", "Width", "0"),
        ("InputHint", "InputHint", ""),
        ("MaxHeight", "MaxHeight", "0"),
        ("AutoShowClearButton", "AutoShowClearButtonMode", "Always"),
        ("AutoShowOpenButton", "AutoShowOpenButtonMode", "Always"),
        (
            "OnMainServerUnavailableBehavior",
            "OnMainServerUnavalableBehavior",
            "MakeDisable",
        ),
    ] {
        assert_eq!(row(id)["kind"], "declared");
        let property = property(row(id));
        assert_eq!(property["key"]["name"], key);
        assert_eq!(property["value"]["text"], text);
    }
    assert_eq!(
        row("MaxWidth")["default"],
        json!({"kind":"number","value":0})
    );
    assert_eq!(
        inspected["rows"]
            .as_array()
            .unwrap()
            .windows(4)
            .find(|rows| rows[0]["id"] == "platform:AutoMaxWidth")
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "platform:AutoMaxWidth",
            "platform:MaxWidth",
            "platform:AutoMaxHeight",
            "platform:MaxHeight"
        ]
    );
    assert_eq!(property(row("TextColor"))["value"]["text"], "web:Blue");
    assert_eq!(input["presentation"]["foreground"]["value"], "Blue");
    assert_eq!(input["presentation"]["font"]["points"], 14.0);
    assert_eq!(
        property(row("Font"))["qualifiers"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    client.finish();
    assert_eq!(fs::read_to_string(path).unwrap(), BOUND_INPUT);
}

#[test]
fn form_input_palette_projects_only_opted_in_events_with_real_xml_sources() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    let events = concat!(
        "<Events><f:Event xmlns:f=\"urn:foreign\" name=\"OnChange\">Foreign</f:Event>",
        "<Event name=\"OnChange\">HandleChange</Event><Event name=\"Unrecognized\">Other</Event></Events>"
    );
    let changed = BOUND_INPUT.replace("</InputField>", &format!("{events}</InputField>"));
    fs::write(&path, BOUND_INPUT).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let opened = update_input(&mut client, &context, &opened, 2, &changed);
    let input = &opened["model"]["root"]["children"][0];
    let inspected = inspect_input(&mut client, &context, &opened, input, true);
    let event = input_palette_value(&inspected, "OnChange");
    assert_eq!(event["kind"], "declaredEvent");
    assert_eq!(event["entryIndex"], 1);
    assert_eq!(event["sourceKind"], "form");
    let index = usize::try_from(event["propertyIndex"].as_u64().unwrap()).unwrap();
    let wrapper = &input["properties"][index];
    assert_eq!(wrapper["key"]["name"], "Events");
    assert_eq!(
        wrapper["value"]["fields"][1]["value"]["text"],
        "HandleChange"
    );
    assert_eq!(
        input_palette_value(&inspected, "StartChoice")["kind"],
        "absent"
    );
    let location = client.request("form/locate", &params(&context, json!({
        "formSessionId":opened["formSessionId"],"revision":opened["revision"],
        "target":{"kind":"property","identity":input["identity"],"property":wrapper["key"],"occurrence":event["occurrence"]}
    })));
    assert_eq!(xml_at(&changed, &location["range"]), events);

    let previous = inspect_input(&mut client, &context, &opened, input, false);
    assert_eq!(previous["categories"].as_array().unwrap().len(), 4);
    assert!(
        previous["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["category"] != "events")
    );
    assert!(previous["rows"].as_array().unwrap().iter().any(|row| {
        row["category"].is_null() && row["value"]["propertyIndex"] == event["propertyIndex"]
    }));
    let unresolved = changed.replace("<DataPath>Value</DataPath>", "<DataPath>Missing</DataPath>");
    let updated = update_input(&mut client, &context, &opened, 3, &unresolved);
    let unresolved = inspect_input(
        &mut client,
        &context,
        &updated,
        &updated["model"]["root"]["children"][0],
        true,
    );
    assert!(unresolved["rows"].is_null());
    client.finish();
    assert_eq!(fs::read_to_string(path).unwrap(), BOUND_INPUT);
}

#[test]
fn form_palette_agrees_with_the_native_group_catalog_and_round_trip() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    fs::write(&path, GROUP).unwrap();
    let native: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/forms/native-property-palette/usual-group.json"
    ))
    .unwrap();
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 41);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let inspected = inspect_group(&mut client, &context, &opened);
        let rows = inspected["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 35);
        for category in inspected["categories"].as_array().unwrap() {
            assert_eq!(
                category["caption"],
                native["categoryLabels"][category["id"].as_str().unwrap()]
            );
        }
        let group = &opened["model"]["root"]["children"][0];
        assert_eq!(group["presentation"]["united"], true);
        assert_eq!(group["properties"].as_array().unwrap().len(), 2);
        for (row, expected) in rows.iter().zip(native["rows"].as_array().unwrap()) {
            assert_native_row(row, expected, locale);
        }
        let id = &opened["formSessionId"];
        let source = &opened["dependencies"][0]["id"];
        let updated = client.request(
            "form/update",
            &params(
                &context,
                json!({
                    "formSessionId":id,"revision":opened["revision"],
                    "overlays":[{"source":source,"version":"2","text":GROUP_OVERRIDES}]
                }),
            ),
        );
        let properties = updated["model"]["root"]["children"][0]["properties"]
            .as_array()
            .unwrap();
        let updated_inspection = inspect_group(&mut client, &context, &updated);
        assert_eq!(
            updated["model"]["root"]["children"][0]["presentation"]["united"],
            false
        );
        let overrides = updated_inspection["rows"].as_array().unwrap();
        assert_eq!(overrides.len(), rows.len());
        for ((row, updated), expected) in rows
            .iter()
            .zip(overrides)
            .zip(native["rows"].as_array().unwrap())
        {
            assert_eq!(row["id"], updated["id"]);
            if let Some(key) = expected["roundtrip"]["key"].as_str() {
                let value = &updated["value"];
                assert_eq!(value["sourceKind"], "form");
                if expected["roundtrip"]["kind"] == "attribute" {
                    assert_eq!(value["kind"], "qualifier");
                    assert_eq!(value["key"]["name"], key);
                    assert_eq!(value["text"], expected["roundtrip"]["value"]);
                } else {
                    assert_eq!(value["kind"], "declared");
                    let index = usize::try_from(value["propertyIndex"].as_u64().unwrap()).unwrap();
                    let field = &properties[index];
                    assert_eq!(field["key"]["name"], key);
                    assert_eq!(field["value"]["text"], expected["roundtrip"]["value"]);
                }
            }
        }
        let stale = client.response(
            "form/inspect",
            &params(
                &context,
                json!({
                    "formSessionId":id,"revision":opened["revision"],"identity":group["identity"]
                }),
            ),
        );
        assert_eq!(
            stale["error"]["data"]["details"]["reason"],
            "stale_form_revision"
        );
        client.finish();
    }
    assert_eq!(fs::read_to_string(path).unwrap(), GROUP);
}

#[test]
fn form_palette_keeps_qualified_repeated_properties_and_only_navigates_real_xml() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    let source = GROUP
        .replace(
            r#"<UsualGroup name="Group" id="1">"#,
            r#"<UsualGroup xmlns:f="urn:foreign" f:hint="raw" name="Group" id="1">"#,
        )
        .replace(
            "<Group>Vertical</Group>",
            "<Group>Vertical</Group><f:United>false</f:United><f:United/><Width>0</Width><ToolTip/>",
        );
    fs::write(&path, &source).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let inspected = inspect_group(&mut client, &context, &opened);
    let rows = inspected["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 38);
    let row = |id: &str| &rows.iter().find(|row| row["id"] == id).unwrap()["value"];
    assert_eq!(row("platform:United")["default"]["value"], true);
    for id in ["platform:Width", "platform:ToolTip"] {
        assert_eq!(row(id)["kind"], "declared");
    }
    assert_eq!(rows[35]["value"]["occurrence"], 0);
    assert_eq!(rows[36]["value"]["occurrence"], 1);
    let attribute = &rows[37]["value"];
    assert_eq!(attribute["kind"], "qualifier");
    assert_eq!(attribute["text"], "raw");
    assert_eq!(attribute["sourceKind"], "form");
    let identity = &inspected["identity"];
    let location = client.request("form/locate", &params(&context, json!({
        "formSessionId":opened["formSessionId"],"revision":opened["revision"],
        "target":{"kind":"attribute","identity":identity,"attribute":{"namespace":"urn:foreign","name":"hint"}}
    })));
    assert_eq!(location["range"], attribute["origin"]["range"]);
    assert_eq!(xml_at(&source, &location["range"]), r#"f:hint="raw""#);
    let locate = |namespace: &str, occurrence: usize| {
        params(
            &context,
            json!({
                "formSessionId":opened["formSessionId"],"revision":opened["revision"],
                "target":{"kind":"property","identity":identity,"property":{"namespace":namespace,"name":"United"},"occurrence":occurrence}
            }),
        )
    };
    for (occurrence, text) in ["<f:United>false</f:United>", "<f:United/>"]
        .iter()
        .enumerate()
    {
        let location = client.request("form/locate", &locate("urn:foreign", occurrence));
        assert_eq!(xml_at(&source, &location["range"]), *text);
    }
    let absent = client.response("form/locate", &locate("http://v8.1c.ru/8.3/xcf/logform", 0));
    assert_eq!(
        absent["error"]["data"]["details"]["reason"],
        "target_unavailable"
    );
    let other = client.request(
        "form/inspect",
        &params(
            &context,
            json!({
                "formSessionId":opened["formSessionId"],"revision":opened["revision"],
                "identity":opened["model"]["root"]["identity"]
            }),
        ),
    );
    assert!(other["rows"].is_null());
    client.finish();
}

fn xml_at<'a>(source: &'a str, range: &Value) -> &'a str {
    let start = usize::try_from(range["start"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(range["end"].as_u64().unwrap()).unwrap();
    &source[start..end]
}

#[test]
fn form_palette_is_negotiated_and_rejects_stale_models() {
    let directory = fixture();
    for minor in [40, 41] {
        let mut client = Client::new(&directory.0, "en-US");
        let (context, node) = connect(&mut client, &directory.0, "en-US", minor);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let id = &opened["formSessionId"];
        let revision = &opened["revision"];
        let identity = &opened["model"]["root"]["identity"];
        let inspect = params(
            &context,
            json!({"formSessionId":id,"revision":revision,"identity":identity}),
        );
        if minor == 40 {
            let unsupported = client.response("form/inspect", &inspect);
            assert_eq!(unsupported["error"]["code"], -32601);
        } else {
            let source = &opened["dependencies"][0]["id"];
            let invalid = client.response("form/update", &params(&context, json!({
                "formSessionId":id,"revision":revision,"overlays":[{"source":source,"version":"2","text":"<Form"}]
            })));
            assert_eq!(invalid["error"]["data"]["details"]["reason"], "invalid_xml");
            let stale = client.response("form/inspect", &inspect);
            assert_eq!(
                stale["error"]["data"]["details"]["reason"],
                "stale_form_model"
            );
        }
        client.finish();
    }
}

#[test]
fn form_preparation_is_versioned_and_does_not_parse_payloads() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    fs::write(&path, "<broken").unwrap();
    for minor in [37, 38] {
        let mut client = Client::new(&directory.0, "ru-RU");
        let (context, node) = connect(&mut client, &directory.0, "ru-RU", minor);
        let prepared = client.response("form/prepare", &params(&context, json!({"node":node})));
        if minor < 38 {
            assert_eq!(prepared["error"]["code"], -32601);
        } else {
            let prepared = &prepared["result"];
            assert_eq!(prepared["roots"].as_array().unwrap().len(), 1);
            assert_eq!(prepared["forms"].as_array().unwrap().len(), 1);
            assert_eq!(prepared["forms"][0]["path"]["value"], PATH);
            assert_eq!(prepared["forms"][0]["id"].as_str().unwrap().len(), 64);
            let opened = client.response("form/open", &params(&context, json!({"node":node})));
            assert_eq!(opened["error"]["data"]["details"]["reason"], "invalid_xml");
        }
        client.finish();
    }
    assert_eq!(fs::read_to_string(path).unwrap(), "<broken");
}

#[test]
fn handler_navigation_uses_checked_modules_and_the_current_editor_snapshot() {
    let directory = fixture();
    let xml = INPUT.replace(
        "</InputField>",
        "<Events><Event name=\"OnChange\">Обработка</Event></Events></InputField>",
    );
    fs::write(directory.0.join("src").join(PATH), &xml).unwrap();
    let module = directory
        .0
        .join("src/CommonForms/Preview/Ext/Form/Module.bsl");
    fs::create_dir_all(module.parent().unwrap()).unwrap();
    let disk = "// Existing module without the unsaved declaration\r\n";
    fs::write(&module, disk).unwrap();
    let text = "\u{feff}// 😀 Процедура Обработка() КонецПроцедуры\r\n&НаКлиенте\r\nПроцедура ОБРАБОТКА(Элемент)\r\nКонецПроцедуры\r\n";
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 39);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let id = &opened["formSessionId"];
        assert_eq!(opened["model"]["handlers"][0]["name"], "Обработка");
        assert_eq!(
            opened["model"]["handlers"][0]["owner"]["identity"]["key"]["value"],
            "9223372036854775807"
        );
        let target = json!({"formSessionId":id,"revision":"1","index":0});
        let source = client.request("form/handlerSource", &params(&context, target.clone()));
        assert_eq!(
            source["path"]["value"],
            "CommonForms/Preview/Ext/Form/Module.bsl"
        );
        let input = json!({"source":source["source"],"version":"7","text":text});
        let mut locate = target.clone();
        locate["input"] = input.clone();
        let found = client.request("form/handlerLocate", &params(&context, locate.clone()));
        let start = usize::try_from(found["range"]["start"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(found["range"]["end"].as_u64().unwrap()).unwrap();
        assert_eq!(&text[start..end], "ОБРАБОТКА");
        assert_eq!(found["snapshot"], snapshot(text));
        let mut foreign = locate.clone();
        foreign["input"]["source"] = json!("a".repeat(64));
        assert_eq!(
            client.response("form/handlerLocate", &params(&context, foreign))["error"]["data"]["details"]
                ["reason"],
            "foreign_handler_input"
        );
        for (value, reason) in [
            ("Procedure A() EndProcedure", "handler_not_found"),
            ("Procedure A()", "handler_module_invalid"),
            (
                "Procedure Обработка() EndProcedure\nProcedure Обработка() EndProcedure",
                "handler_ambiguous",
            ),
        ] {
            let mut args = locate.clone();
            args["input"]["text"] = json!(value);
            assert_eq!(
                client.response("form/handlerLocate", &params(&context, args))["error"]["data"]["details"]
                    ["reason"],
                reason
            );
        }
        let mut stale = locate.clone();
        stale["revision"] = json!("2");
        assert_eq!(
            client.response("form/handlerLocate", &params(&context, stale))["error"]["data"]["details"]
                ["reason"],
            "stale_form_revision"
        );
        let retained = client.request("form/get", &params(&context, json!({"formSessionId":id})));
        assert_eq!(retained["status"], "ready");
        assert_eq!(retained["model"], opened["model"]);
        client.finish();
    }
    assert_eq!(fs::read_to_string(module).unwrap(), disk);
}

#[test]
fn handler_navigation_is_not_available_to_an_older_protocol_client() {
    let directory = fixture();
    let mut client = Client::new(&directory.0, "en-US");
    let (context, node) = connect(&mut client, &directory.0, "en-US", 38);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    for method in ["form/handlerSource", "form/handlerLocate"] {
        assert_eq!(
            client.response(
                method,
                &params(
                    &context,
                    json!({"formSessionId":opened["formSessionId"],"revision":"1","index":0})
                )
            )["error"]["code"],
            -32601
        );
    }
    client.finish();
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

fn dependency_style(name: &str) -> String {
    format!(
        "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:v8ui=\"http://v8.1c.ru/8.1/data/ui\" xmlns:sys=\"http://v8.1c.ru/8.1/data/ui/fonts/system\"><StyleItem uuid=\"{name}\"><Properties><Name>{name}</Name><Type>Font</Type><Value xsi:type=\"v8ui:Font\" ref=\"sys:DefaultGUIFont\" height=\"12\" kind=\"WindowsFont\"/></Properties></StyleItem></MetaDataObject>"
    )
}

fn dependency_fixture() -> (support::TestDir, &'static str) {
    let directory = fixture();
    let root = directory.0.join("src");
    let path = root.join(PATH);
    let form = "<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\" xmlns:style=\"http://v8.1c.ru/8.1/data/ui/style\"><ChildItems><LabelDecoration name=\"Caption\" id=\"1\"><Title>Caption</Title><Font ref=\"style:FontA\" kind=\"StyleItem\"/></LabelDecoration></ChildItems></Form>";
    fs::write(&path, form).unwrap();
    let configuration = root.join("Configuration.xml");
    fs::write(
        &configuration,
        fs::read_to_string(&configuration).unwrap().replace(
            "<ChildObjects>",
            "<ChildObjects><StyleItem>FontA</StyleItem><StyleItem>FontB</StyleItem>",
        ),
    )
    .unwrap();
    fs::create_dir(root.join("StyleItems")).unwrap();
    for name in ["FontA", "FontB"] {
        fs::write(
            root.join(format!("StyleItems/{name}.xml")),
            dependency_style(name),
        )
        .unwrap();
    }
    (directory, form)
}

fn prepared_input(prepared: &Value, path: &str, text: &str, version: &str) -> Value {
    json!({"root":prepared["roots"][0],"path":{"value":path,"encoding":"utf-8"},"version":version,"text":text})
}

#[test]
fn prepared_inputs_recover_invalid_disk_form_and_metadata_without_writes() {
    let (directory, form) = dependency_fixture();
    let root = directory.0.join("src");
    fs::write(root.join(PATH), "<broken").unwrap();
    fs::write(root.join("StyleItems/FontB.xml"), "<broken").unwrap();
    fs::create_dir_all(root.join("CommonForms/Other/Ext")).unwrap();
    fs::write(root.join("CommonForms/Other/Ext/Form.xml"), form).unwrap();
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 38);
        let prepared = client.request("form/prepare", &params(&context, json!({"node":node})));
        let form_input = prepared_input(
            &prepared,
            PATH,
            &form.replace("style:FontA", "style:FontB"),
            "11",
        );
        let style_input = prepared_input(
            &prepared,
            "StyleItems/FontB.xml",
            &dependency_style("FontB").replace("height=\"12\"", "height=\"16\""),
            "12",
        );
        let unused = prepared_input(&prepared, "StyleItems/FontA.xml", "<broken", "13");
        let opened = client.request(
            "form/open",
            &params(
                &context,
                json!({"node":node,"inputs":[form_input,style_input,unused]}),
            ),
        );
        assert_eq!(opened["status"], "ready");
        assert_eq!(
            opened["model"]["root"]["children"][0]["presentation"]["font"]["points"],
            16.0
        );
        assert_eq!(opened["dependencies"][0]["id"], prepared["forms"][0]["id"]);
        assert!(
            opened["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .all(|input| !input["path"]["value"]
                    .as_str()
                    .unwrap()
                    .ends_with("FontA.xml"))
        );
        for inputs in [
            json!([form_input, form_input]),
            json!([prepared_input(
                &prepared,
                "CommonForms/Other/Ext/Form.xml",
                form,
                "14"
            )]),
            json!([prepared_input(&prepared, "Module.bsl", "text", "14")]),
        ] {
            let rejected = client.response("form/update", &params(&context,json!({"formSessionId":opened["formSessionId"],"revision":"1","overlays":[],"inputs":inputs})));
            assert_eq!(
                rejected["error"]["data"]["details"]["reason"],
                "foreign_form_input"
            );
        }
        let restored = client.request("form/update", &params(&context,json!({"formSessionId":opened["formSessionId"],"revision":"1","overlays":[],"inputs":[form_input,style_input,unused]})));
        assert_eq!(restored["status"], "ready");
        assert_eq!(restored["revision"], "2");
        client.finish();
    }
    assert_eq!(fs::read_to_string(root.join(PATH)).unwrap(), "<broken");
    assert_eq!(
        fs::read_to_string(root.join("StyleItems/FontB.xml")).unwrap(),
        "<broken"
    );
    assert_eq!(
        fs::read_to_string(root.join("StyleItems/FontA.xml")).unwrap(),
        dependency_style("FontA")
    );
}

#[test]
fn prepared_input_paths_reject_foreign_roots_and_traversal() {
    let directory = fixture();
    let outside = directory.0.join("escape.xml");
    fs::write(&outside, "private").unwrap();
    let mut client = Client::new(&directory.0, "en-US");
    let (context, node) = connect(&mut client, &directory.0, "en-US", 38);
    let prepared = client.request("form/prepare", &params(&context, json!({"node":node})));
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let mut foreign = prepared_input(&prepared, PATH, INPUT, "1");
    foreign["root"] = json!({"value":directory.0,"encoding":"utf-8"});
    for (input, reason) in [
        (foreign, "foreign_form_input"),
        (
            prepared_input(&prepared, "../escape.xml", "<MetaDataObject/>", "1"),
            "source_unavailable",
        ),
    ] {
        let rejected = client.response("form/update", &params(&context,json!({"formSessionId":opened["formSessionId"],"revision":"1","overlays":[],"inputs":[input]})));
        assert_eq!(rejected["error"]["data"]["details"]["reason"], reason);
    }
    client.finish();
    assert_eq!(fs::read_to_string(outside).unwrap(), "private");
}

#[test]
fn form_update_retires_dirty_metadata_but_rejects_unknown_and_replayed_inputs() {
    let (directory, form) = dependency_fixture();
    let root = directory.0.join("src");
    let style = dependency_style;
    let dependency = |view: &Value, name: &str| {
        view["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["path"]["value"].as_str().unwrap().ends_with(name))
            .unwrap()["id"]
            .clone()
    };
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 37);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let id = &opened["formSessionId"];
        let source = dependency(&opened, "Form.xml");
        let a = dependency(&opened, "FontA.xml");
        let dirty_a = json!({"source":a,"version":"11","text":style("FontA").replace("height=\"12\"","height=\"14\"")});
        let duplicate = client.response(
            "form/update",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"1","overlays":[dirty_a,dirty_a]}),
            ),
        );
        assert_eq!(
            duplicate["error"]["data"]["details"]["reason"],
            "foreign_form_input"
        );
        let changed_form = json!({"source":source,"version":"12","text":form.replace("style:FontA","style:FontB")});
        let changed = client.request(
            "form/update",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"1","overlays":[changed_form,dirty_a]}),
            ),
        );
        assert_eq!(changed["revision"], "2");
        assert_eq!(
            changed["model"]["root"]["children"][0]["presentation"]["font"]["points"],
            12.0
        );
        assert!(
            changed["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .all(|input| input["id"] != a)
        );
        let replayed = client.response(
            "form/update",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"2","overlays":[dirty_a]}),
            ),
        );
        assert_eq!(
            replayed["error"]["data"]["details"]["reason"],
            "foreign_form_input"
        );
        let b = dependency(&changed, "FontB.xml");
        let dirty_b = json!({"source":b,"version":"13","text":style("FontB").replace("height=\"12\"","height=\"18\"")});
        let updated = client.request(
            "form/update",
            &params(
                &context,
                json!({"formSessionId":id,"revision":"2","overlays":[changed_form,dirty_b]}),
            ),
        );
        assert_eq!(updated["revision"], "3");
        assert_eq!(
            updated["model"]["root"]["children"][0]["presentation"]["font"]["points"],
            18.0
        );
        let unknown = client.response("form/update", &params(&context,json!({"formSessionId":id,"revision":"3","overlays":[{"source":"unrelated","version":"14","text":style("FontB")}]})));
        assert_eq!(
            unknown["error"]["data"]["details"]["reason"],
            "foreign_form_input"
        );
        client.finish();
    }
    assert_eq!(fs::read_to_string(root.join(PATH)).unwrap(), form);
    for name in ["FontA", "FontB"] {
        assert_eq!(
            fs::read_to_string(root.join(format!("StyleItems/{name}.xml"))).unwrap(),
            style(name)
        );
    }
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
