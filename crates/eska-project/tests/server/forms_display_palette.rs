use super::{
    native_palette::{element, property_row},
    *,
};

#[test]
fn display_fields_and_horizontal_groups_use_catalog_defaults_and_shared_events() {
    let directory = fixture();
    let xml = r#"<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:xs="http://www.w3.org/2001/XMLSchema"><ChildItems>
        <UsualGroup name="Horizontal" id="1"><Group>AlwaysHorizontal</Group><ChildItems>
        <PictureField name="Picture" id="2"><DataPath>Value</DataPath><Events><Event name="Click">PictureClick</Event></Events><ContextMenu name="Menu" id="3"/></PictureField>
        <LabelField name="Label" id="4"><DataPath>Value</DataPath></LabelField><UsualGroup name="Nested" id="5"><Group>Vertical</Group><ChildItems><CheckBoxField name="NestedFlag" id="6"><DataPath>Flag</DataPath></CheckBoxField></ChildItems></UsualGroup>
        </ChildItems></UsualGroup></ChildItems><Attributes><Attribute name="Value" id="1"><Type><v8:Type>xs:decimal</v8:Type></Type></Attribute><Attribute name="Flag" id="2"><Type><v8:Type>xs:boolean</v8:Type></Type></Attribute></Attributes></Form>"#;
    fs::write(directory.0.join("src").join(PATH), xml).unwrap();
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 47);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let root = &opened["model"]["root"];
        for name in ["Horizontal", "Picture", "Label", "Menu", "NestedFlag"] {
            let node = element(root, name).unwrap();
            let palette = inspect_palette(&mut client, &context, &opened, node, true);
            let rows = palette["rows"].as_array().unwrap();
            assert!(!rows.is_empty(), "{name}");
            for row in rows.iter().filter(|row| {
                row["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("platform:"))
            }) {
                assert!(
                    row["caption"][locale]
                        .as_str()
                        .is_some_and(|text| !text.starts_with("form-") && !text.is_empty()),
                    "{name}: {row}"
                );
            }
            if name == "Menu" {
                assert_eq!(rows.len(), 2);
            }
            if name == "Horizontal" {
                assert_eq!(
                    property_row(&palette, "United")["value"]["default"]["value"],
                    true
                );
            }
            if name == "Picture" {
                assert_eq!(
                    property_row(&palette, "ValuesPicture")["value"]["default"]["kind"],
                    "empty"
                );
                assert_eq!(
                    property_row(&palette, "Click")["value"]["kind"],
                    "declaredEvent"
                );
                assert_eq!(
                    property_row(&palette, "OnChange")["value"]["default"]["kind"],
                    "empty"
                );
                assert_eq!(property_row(&palette, "FileDragMode")["available"], false);
            }
            if name == "Label" {
                property_row(&palette, "Format");
                property_row(&palette, "URLProcessing");
                assert!(!rows.iter().any(|row| row["id"] == "platform:ValuesPicture"));
            }
        }
        client.finish();
    }
    assert_eq!(
        fs::read_to_string(directory.0.join("src").join(PATH)).unwrap(),
        xml
    );
}
