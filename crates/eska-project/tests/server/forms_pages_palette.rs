use super::{
    native_palette::{assert_native_palette, element, property_row},
    *,
};

const FORM: &str = include_str!("../../../../tests/fixtures/forms/native-groups/PagesAuto.xml");
const NATIVE: &str =
    include_str!("../../../../tests/fixtures/forms/native-property-palette/pages.json");

#[test]
fn page_palettes_match_designer_order_and_defaults_in_both_locales() {
    let native: Value = serde_json::from_str(NATIVE).unwrap();
    let directory = fixture();
    let file = directory.0.join("src").join(PATH);
    fs::write(&file, FORM).unwrap();
    for locale in ["ru-RU", "en-US"] {
        let mut client = Client::new(&directory.0, locale);
        let (context, node) = connect(&mut client, &directory.0, locale, 41);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let root = &opened["model"]["root"];
        for (name, kind) in [("Pages", "Pages"), ("FirstPage", "Page")] {
            let item = element(root, name).unwrap();
            let palette = inspect_palette(&mut client, &context, &opened, item, true);
            assert_native_palette(&palette, &native["cases"][kind], &native, name);
            for row in palette["rows"].as_array().unwrap() {
                if let Some(editor) = native["editors"].get(row["id"].as_str().unwrap()) {
                    assert_eq!(row["editor"], *editor, "{name}/{row}");
                }
            }
        }
        client.finish();
    }
    assert_eq!(fs::read_to_string(file).unwrap(), FORM);
}

const REPRESENTATION: &str = "<PagesRepresentation>TabsOnLeftHorizontal</PagesRepresentation>";
const ASSOCIATED: &str =
    "<AssociatedTableElementId xsi:type=\"xs:string\">RelatedRows</AssociatedTableElementId>";
const EVENTS: &str = "<Events><Event name=\"OnCurrentPageChange\">OnPageChanged</Event></Events>";
const SCROLL: &str = "<ScrollOnCompress>true</ScrollOnCompress>";
const ALIGN: &str = "<GroupHorizontalAlign>Center</GroupHorizontalAlign>";
const CURRENT_ROW: &str = "<CurrentRowUse>DontUse</CurrentRowUse>";
const GROUP: &str = "<Group>HorizontalIfPossible</Group>";
const CHILD_ALIGN: &str = "<ChildrenAlign>TitlesLeftDataAuto</ChildrenAlign>";

#[test]
fn page_palettes_follow_xml_overrides_events_and_removal_in_dirty_buffers() {
    let directory = fixture();
    let file = directory.0.join("src").join(PATH);
    fs::write(&file, FORM).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let edited = edited_form();
    let updated = update_input(&mut client, &context, &opened, 2, &edited);
    let root = &updated["model"]["root"];
    let pages = element(root, "Pages").unwrap();
    let page = element(root, "FirstPage").unwrap();
    let palette = inspect_palette(&mut client, &context, &updated, pages, true);
    let page_palette = inspect_palette(&mut client, &context, &updated, page, true);
    assert_value_captions(&palette, &page_palette);
    assert_sources(
        &mut client,
        &context,
        &updated,
        &edited,
        pages,
        &palette,
        &[
            ("PagesRepresentation", "PagesRepresentation", REPRESENTATION),
            ("AssociatedTable", "AssociatedTableElementId", ASSOCIATED),
            ("HorizontalAlignInGroup", "GroupHorizontalAlign", ALIGN),
            ("OnCurrentPageChange", "Events", EVENTS),
        ],
    );
    assert_sources(
        &mut client,
        &context,
        &updated,
        &edited,
        page,
        &page_palette,
        &[
            ("VerticalScrollOnReduceSize", "ScrollOnCompress", SCROLL),
            ("ItemsAndTitlesAlign", "ChildrenAlign", CHILD_ALIGN),
        ],
    );
    assert!(
        page_palette["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["category"].is_null()
                && row["id"].as_str().unwrap().contains("ChildItemsWidth"))
    );
    let without_events = inspect_palette(&mut client, &context, &updated, pages, false);
    assert!(
        without_events["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["category"] != "events")
    );
    let restored = update_input(&mut client, &context, &updated, 3, FORM);
    for (name, before) in [("Pages", pages), ("FirstPage", page)] {
        let item = element(&restored["model"]["root"], name).unwrap();
        assert_eq!(item["identity"], before["identity"]);
        let initial = element(&opened["model"]["root"], name).unwrap();
        let initial_palette = inspect_palette(&mut client, &context, &restored, item, true);
        assert_eq!(item["properties"], initial["properties"]);
        for id in if name == "Pages" {
            &[
                "PagesRepresentation",
                "AssociatedTable",
                "OnCurrentPageChange",
            ][..]
        } else {
            &["Group", "VerticalScrollOnReduceSize"][..]
        } {
            let value = &property_row(&initial_palette, id)["value"];
            assert_eq!(value["kind"], "absent", "{name}/{id}");
            assert!(value.get("propertyIndex").is_none());
        }
    }
    client.finish();
    assert_eq!(fs::read_to_string(file).unwrap(), FORM);
}

fn edited_form() -> String {
    FORM
        .replace(
            r#"<Pages name="Pages" id="1">"#,
            &format!(
                r#"<Pages name="Pages" id="1">{REPRESENTATION}{ASSOCIATED}{ALIGN}{CURRENT_ROW}{EVENTS}<ToolTipRepresentation>ShowAuto</ToolTipRepresentation>"#
            ),
        )
        .replace(
            r#"<Page name="FirstPage" id="3">"#,
            &format!(
                r#"<Page name="FirstPage" id="3">{SCROLL}{GROUP}{CHILD_ALIGN}<HorizontalSpacing>OneAndHalf</HorizontalSpacing><ChildItemsWidth>LeftNarrowest</ChildItemsWidth>"#
            ),
        )
}

fn assert_value_captions(palette: &Value, page_palette: &Value) {
    for (selection, id, ru, en) in [
        (
            palette,
            "PagesRepresentation",
            "Закладки слева (горизонтально)",
            "Tabs on the left (horizontally)",
        ),
        (palette, "CurrentRowUse", "Не использует", "Do not use"),
        (
            palette,
            "ToolTipRepresentation",
            "Отображать авто",
            "Display automatically",
        ),
        (
            page_palette,
            "Group",
            "Горизонтальная если возможно",
            "Horizontal if possible",
        ),
        (
            page_palette,
            "ItemsAndTitlesAlign",
            "Элементы авто, заголовки лево",
            "Items auto, titles left",
        ),
        (
            page_palette,
            "HorizontalSpacing",
            "Полуторный",
            "One and half",
        ),
    ] {
        assert_eq!(
            property_row(selection, id)["value"]["caption"],
            json!({"ru-RU":ru,"en-US":en})
        );
    }
}

fn assert_sources(
    client: &mut Client,
    context: &Value,
    updated: &Value,
    edited: &str,
    item: &Value,
    palette: &Value,
    sources: &[(&str, &str, &str)],
) {
    for &(id, key, xml) in sources {
        let kind = if key == "Events" {
            "declaredEvent"
        } else {
            "declared"
        };
        let value = &property_row(palette, id)["value"];
        assert_eq!(value["kind"], kind);
        assert_eq!(value["sourceKind"], "form");
        let index = usize::try_from(value["propertyIndex"].as_u64().unwrap()).unwrap();
        let property = &item["properties"][index];
        assert_eq!(property["key"]["name"], key);
        let located = client.request(
            "form/locate",
            &params(
                context,
                json!({
                    "formSessionId":updated["formSessionId"],"revision":updated["revision"],
                    "target":{"kind":"property","identity":item["identity"],"property":property["key"],"occurrence":value["occurrence"]}
                }),
            ),
        );
        assert_eq!(xml_at(edited, &located["range"]), xml);
        if kind == "declaredEvent" {
            let index = usize::try_from(value["entryIndex"].as_u64().unwrap()).unwrap();
            assert_eq!(
                property["value"]["fields"][index]["value"]["text"],
                "OnPageChanged"
            );
        }
    }
}
