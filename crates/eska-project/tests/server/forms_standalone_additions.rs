use super::{native_palette::element, *};

fn source(name: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/forms/native-standalone-additions");
    fs::read_to_string(root.join(format!("{name}.xml"))).unwrap()
}

fn names(nodes: &Value) -> Vec<&str> {
    nodes
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["name"].as_str())
        .collect()
}

#[test]
fn separately_placed_additions_keep_child_order_and_legacy_protocol_shape() {
    let directory = fixture();
    let path = directory.0.join("src").join(PATH);
    let input = source("Horizontal");
    fs::write(&path, &input).unwrap();
    for minor in [42, 43] {
        let mut client = Client::new(&directory.0, "en-US");
        let (context, node) = connect(&mut client, &directory.0, "en-US", minor);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let root = &opened["model"]["root"];
        let group = element(root, "Inline").unwrap();
        let expected = if minor == 43 {
            vec![
                "Before",
                "SearchExtra",
                "ManageExtra",
                "StatusExtra",
                "After",
            ]
        } else {
            vec!["Before", "After"]
        };
        assert_eq!(names(&group["children"]), expected);
        let extras = names(&group["auxiliary"])
            .into_iter()
            .filter(|name| name.ends_with("Extra"))
            .collect::<Vec<_>>();
        if minor == 43 {
            assert_eq!(extras, [] as [&str; 0]);
        } else {
            assert_eq!(extras, ["SearchExtra", "ManageExtra", "StatusExtra"]);
        }
        let table = element(root, "Rows").unwrap();
        for name in ["SearchOwned", "StatusOwned", "ManageOwned"] {
            assert!(names(&table["auxiliary"]).contains(&name));
        }
        let search = element(group, "SearchExtra").unwrap();
        assert_eq!(search["presentation"]["displayed"], true);
        let identity = search["identity"].clone();
        let dirty = input.replace("<Item>Rows</Item>", "<Item>Missing</Item>");
        assert_ne!(dirty, input);
        let updated = update_input(&mut client, &context, &opened, 2, &dirty);
        let root = &updated["model"]["root"];
        let search = element(root, "SearchExtra").unwrap();
        assert_eq!(search["identity"], identity);
        assert_eq!(search["presentation"]["displayed"], false);
        assert_eq!(
            element(root, "SearchOwned").unwrap()["presentation"]["displayed"],
            true
        );
        let palette = inspect_palette(&mut client, &context, &updated, search, false);
        assert!(
            palette["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == "platform:Source")
        );
        client.finish();
    }
    assert_eq!(fs::read_to_string(path).unwrap(), input);
}

#[test]
fn placed_addition_visibility_uses_the_effective_source_without_its_visual_placement() {
    for (name, extra_visible, table_visible) in [
        ("HiddenOwner", true, false),
        ("LocationsNone", true, true),
        ("Unbound", false, true),
        ("NoColumns", false, true),
    ] {
        let directory = fixture();
        fs::write(directory.0.join("src").join(PATH), source(name)).unwrap();
        let mut client = Client::new(&directory.0, "ru-RU");
        let (context, node) = connect(&mut client, &directory.0, "ru-RU", 43);
        let opened = client.request("form/open", &params(&context, json!({"node":node})));
        let root = &opened["model"]["root"];
        for name in ["SearchExtra", "StatusExtra", "ManageExtra"] {
            assert_eq!(
                element(root, name).unwrap()["presentation"]["displayed"],
                extra_visible,
                "{name}"
            );
        }
        assert_eq!(
            element(root, "Rows").unwrap()["presentation"]["displayed"],
            table_visible
        );
        client.finish();
    }

    let directory = fixture();
    let input = source("PdfExternal");
    fs::write(directory.0.join("src").join(PATH), &input).unwrap();
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 43);
    let opened = client.request("form/open", &params(&context, json!({"node":node})));
    let root = &opened["model"]["root"];
    assert_eq!(names(&root["children"]), ["StatusExtra", "Control"]);
    assert_eq!(
        element(root, "StatusExtra").unwrap()["presentation"]["displayed"],
        true
    );
    assert!(names(&element(root, "Control").unwrap()["auxiliary"]).contains(&"PdfStatus"));
    client.finish();
}
