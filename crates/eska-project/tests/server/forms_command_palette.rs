use super::{
    native_palette::{assert_native_palette, element, property_row},
    *,
};

const CATALOGUE: &str =
    include_str!("../../../../tests/fixtures/forms/native-command-palette/metrics.json");

fn source(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/forms/native-command-palette")
            .join(name),
    )
    .unwrap()
}

fn configuration(variant: &str, input: &str) -> support::TestDir {
    let directory = fixture();
    let root = directory.0.join("src");
    let config = root.join("Configuration.xml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace(
                "</Properties>",
                &format!("<ScriptVariant>{variant}</ScriptVariant></Properties>"),
            )
            .replace(
                "<ChildObjects>",
                "<ChildObjects><Catalog>NativeGoods</Catalog>",
            ),
    )
    .unwrap();
    fs::create_dir(root.join("Catalogs")).unwrap();
    fs::write(root.join("Catalogs/NativeGoods.xml"), NATIVE_GOODS).unwrap();
    fs::write(root.join(PATH), input).unwrap();
    directory
}

#[test]
fn command_and_addition_palettes_follow_native_order_defaults_and_sources() {
    let native: Value = serde_json::from_str(CATALOGUE).unwrap();
    for case in native["rows"].as_array().unwrap() {
        let input = source(case["file"].as_str().unwrap());
        for (locale, variant) in [("en-US", "English"), ("ru-RU", "Russian")] {
            let (variant, source_language) = if case["name"] == "Tuned" {
                ("English", "en-US")
            } else {
                (variant, locale)
            };
            let directory = configuration(variant, &input);
            let mut client = Client::new(&directory.0, locale);
            let (context, node) = connect(&mut client, &directory.0, locale, 41);
            let snapshot = client.request("form/open", &params(&context, json!({"node":node})));
            assert_eq!(snapshot["model"]["scriptVariant"], variant.to_lowercase());
            for selection in case["selections"].as_array().unwrap() {
                let name = selection["name"].as_str().unwrap();
                let element = element(&snapshot["model"]["root"], name).unwrap();
                let palette = inspect_palette(&mut client, &context, &snapshot, element, true);
                let description = format!("{}/{name}/{locale}", case["name"].as_str().unwrap());
                assert_native_palette(&palette, selection, &native, &description);
                if let Some(member) =
                    native["additionSourceMembers"][name][source_language].as_str()
                {
                    let value = &property_row(&palette, "Source")["value"];
                    assert_eq!(
                        value["caption"],
                        json!({
                            "ru-RU":format!("Rows.{member}"),
                            "en-US":format!("Rows.{member}"),
                        })
                    );
                    let field = declared_property(element, value, "AdditionSource");
                    assert_eq!(field["value"]["kind"], "record");
                }
            }
            if case["name"] == "PlainCommands" {
                let edited = input.replace(
                    "<ExtendedTooltip name=\"BGControlTooltip\"",
                    "<Representation>Compact</Representation><ExtendedTooltip name=\"BGControlTooltip\"",
                );
                let snapshot = update_input(&mut client, &context, &snapshot, 2, &edited);
                let group = element(&snapshot["model"]["root"], "BarGroup").unwrap();
                let palette = inspect_palette(&mut client, &context, &snapshot, group, false);
                let value = &property_row(&palette, "View")["value"];
                let field = declared_property(group, value, "Representation");
                assert_eq!(field["value"]["text"], "Compact");
                assert_eq!(
                    value["caption"],
                    json!({"en-US":"Compact","ru-RU":"Компактное"})
                );
            }
            client.finish();
            assert_eq!(
                fs::read_to_string(directory.0.join("src").join(PATH)).unwrap(),
                input
            );
        }
    }
}

fn declared_property<'a>(element: &'a Value, value: &Value, name: &str) -> &'a Value {
    assert_eq!(value["kind"], "declared");
    let index = usize::try_from(value["propertyIndex"].as_u64().unwrap()).unwrap();
    let field = &element["properties"][index];
    assert_eq!(field["key"]["namespace"], "http://v8.1c.ru/8.3/xcf/logform");
    assert_eq!(field["key"]["name"], name);
    field
}

#[test]
fn command_and_addition_palette_rows_follow_designer_xml_aliases_after_dirty_updates() {
    let input = source("External.xml");
    let directory = configuration("English", &input);
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let snapshot = client.request("form/open", &params(&context, json!({"node":node})));
    let root = &snapshot["model"]["root"];
    let before = element(root, "StatusExtra").unwrap()["identity"].clone();
    let changed = input
        .replace("<CommandSource>Item.Rows</CommandSource>", "<CommandSource>Item.Rows</CommandSource><HorizontalLocation>Right</HorizontalLocation>")
        .replace("<AutoCommandBar name=\"RowsBar\" id=\"5\"/>", "<AutoCommandBar name=\"RowsBar\" id=\"5\"><Autofill>false</Autofill><HorizontalAlign>Right</HorizontalAlign></AutoCommandBar>")
        .replace("<ViewStatusAddition name=\"StatusExtra\" id=\"28\">",
            "<ViewStatusAddition name=\"StatusExtra\" id=\"28\"><AutoMaxWidth>false</AutoMaxWidth><MaxWidth>15</MaxWidth><HorizontalLocation>Left</HorizontalLocation>");
    let snapshot = update_input(&mut client, &context, &snapshot, 2, &changed);
    let root = &snapshot["model"]["root"];
    for (name, row, key, expected) in [
        ("BoundBar", "HorizontalAlign", "HorizontalLocation", "Right"),
        ("RowsBar", "AutoFill", "Autofill", "false"),
        ("RowsBar", "HorizontalAlign", "HorizontalAlign", "Right"),
        (
            "StatusExtra",
            "HorizontalAlign",
            "HorizontalLocation",
            "Left",
        ),
        ("StatusExtra", "MaxWidth", "MaxWidth", "15"),
    ] {
        let item = element(root, name).unwrap();
        let palette = inspect_palette(&mut client, &context, &snapshot, item, false);
        let field = declared_property(item, &property_row(&palette, row)["value"], key);
        assert_eq!(field["value"]["text"], expected, "{name}/{row}");
    }
    let status = element(root, "StatusExtra").unwrap();
    assert_eq!(status["identity"], before);
    let palette = inspect_palette(&mut client, &context, &snapshot, status, false);
    let field = declared_property(
        status,
        &property_row(&palette, "Source")["value"],
        "AdditionSource",
    );
    let located = client.request(
        "form/locate",
        &params(
            &context,
            json!({
                "formSessionId":snapshot["formSessionId"],
                "revision":snapshot["revision"],
                "target":{"kind":"property","identity":before,"property":field["key"]}
            }),
        ),
    );
    assert_eq!(located["range"], field["origin"]["range"]);
    client.finish();
    assert_eq!(
        fs::read_to_string(directory.0.join("src").join(PATH)).unwrap(),
        input
    );
}
