use super::{
    native_palette::{assert_native_palette, element, property_row},
    *,
};

const FORM: &str =
    include_str!("../../../../tests/fixtures/forms/native-groups/GroupCollapsible.xml");
const NATIVE: &str =
    include_str!("../../../../tests/fixtures/forms/native-property-palette/collapsible-group.json");

#[test]
fn collapsible_group_palette_matches_designer_in_both_locales() {
    let native: Value = serde_json::from_str(NATIVE).unwrap();
    let directory = fixture();
    let file = directory.0.join("src").join(PATH);
    let input = FORM
        .replace("<Collapsed>true</Collapsed>", "")
        .replace("<Group>Vertical</Group>", "");
    fs::write(&file, &input).unwrap();
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 41);
        let snapshot = client.request("form/open", &params(&context, json!({"node":node})));
        let group = element(&snapshot["model"]["root"], "Group").unwrap();
        let palette = inspect_palette(&mut client, &context, &snapshot, group, true);
        assert_native_palette(&palette, &native["default"], &native, locale);
        client.finish();
    }
    assert_eq!(fs::read_to_string(file).unwrap(), input);
}

#[test]
fn group_behavior_updates_palette_and_preserves_inapplicable_xml() {
    const COLOR: &str =
        "<HiddenStateTitleBackColor>style:FormTextColor</HiddenStateTitleBackColor>";
    let directory = fixture();
    let file = directory.0.join("src").join(PATH);
    fs::write(&file, FORM).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let snapshot = client.request("form/open", &params(&context, json!({"node":node})));
    let input = FORM.replace(
        "<Collapsed>true</Collapsed>",
        &format!(
            "<Collapsed>true</Collapsed><ControlRepresentation>Picture</ControlRepresentation>{COLOR}<ThroughAlign>Use</ThroughAlign>"
        ),
    );
    let snapshot = update_input(&mut client, &context, &snapshot, 2, &input);
    let group = element(&snapshot["model"]["root"], "Group").unwrap();
    let identity = group["identity"].clone();
    let palette = inspect_palette(&mut client, &context, &snapshot, group, true);
    let display = &property_row(&palette, "ControlRepresentation")["value"];
    assert_eq!(
        display["caption"],
        json!({"ru-RU":"Картинка","en-US":"Picture"})
    );
    assert!(has_extra(&palette, "ThroughAlign"));
    let color = &property_row(&palette, "HiddenRepresentationTitleBackColor")["value"];
    let index = usize::try_from(color["propertyIndex"].as_u64().unwrap()).unwrap();
    let property = &group["properties"][index];
    assert_eq!(property["key"]["name"], "HiddenStateTitleBackColor");
    let located = client.request(
        "form/locate",
        &params(
            &context,
            json!({
                "formSessionId":snapshot["formSessionId"],"revision":snapshot["revision"],
                "target":{"kind":"property","identity":identity,"property":property["key"],"occurrence":color["occurrence"]}
            }),
        ),
    );
    assert_eq!(xml_at(&input, &located["range"]), COLOR);
    let ordinary = input.replace(
        "<Behavior>Collapsible</Behavior>",
        "<Behavior>Usual</Behavior>",
    );
    let snapshot = update_input(&mut client, &context, &snapshot, 3, &ordinary);
    let group = element(&snapshot["model"]["root"], "Group").unwrap();
    assert_eq!(group["identity"], identity);
    let palette = inspect_palette(&mut client, &context, &snapshot, group, true);
    assert_eq!(
        property_row(&palette, "ThroughAlign")["value"]["kind"],
        "declared"
    );
    assert!(has_extra(&palette, "HiddenStateTitleBackColor"));
    assert!(has_extra(&palette, "Collapsed"));
    assert!(
        palette["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["id"] != "platform:Collapsed")
    );
    client.finish();
    assert_eq!(fs::read_to_string(file).unwrap(), FORM);
}

fn has_extra(palette: &Value, key: &str) -> bool {
    let id = format!("xml:{}", json!(["http://v8.1c.ru/8.3/xcf/logform", key, 0]));
    palette["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == id)
}
