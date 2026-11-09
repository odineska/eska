use super::*;

const CATALOGUE: &str =
    include_str!("../../../../tests/fixtures/forms/native-table-palette/metrics.json");
const FIXTURES: &str = "../../../../tests/fixtures/forms/native-table-palette/";
const NS: &str = "http://v8.1c.ru/8.3/xcf/logform";

fn source(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/server")
            .join(FIXTURES)
            .join(name),
    )
    .unwrap()
}

fn element<'a>(root: &'a Value, name: &str) -> Option<&'a Value> {
    if root["name"] == name {
        return Some(root);
    }
    root["children"]
        .as_array()?
        .iter()
        .find_map(|child| element(child, name))
}

fn fixture_with_resources(hierarchical: bool) -> support::TestDir {
    let directory = fixture();
    let project_source = directory.0.join("src");
    let configuration = project_source.join("Configuration.xml");
    fs::write(
        &configuration,
        fs::read_to_string(&configuration).unwrap().replace(
            "<ChildObjects>",
            "<ChildObjects><Catalog>NativeGoods</Catalog><Document>NativeSale</Document>",
        ),
    )
    .unwrap();
    fs::create_dir(project_source.join("Catalogs")).unwrap();
    fs::write(
        project_source.join("Catalogs/NativeGoods.xml"),
        if hierarchical {
            source("HierarchicalCatalog.xml")
        } else {
            NATIVE_GOODS.to_owned()
        },
    )
    .unwrap();
    fs::create_dir(project_source.join("Documents")).unwrap();
    fs::write(
        project_source.join("Documents/NativeSale.xml"),
        source("NativeSale.xml"),
    )
    .unwrap();
    fs::write(project_source.join(PATH), source("ValueTable.xml")).unwrap();
    directory
}

fn property_row<'a>(palette: &'a Value, id: &str) -> &'a Value {
    let key = format!("platform:{id}");
    palette["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == key)
        .unwrap_or_else(|| panic!("missing table palette row {id}"))
}

fn assert_native_palette(palette: &Value, expected: &Value, native: &Value, case: &str) {
    let rows = palette["rows"].as_array().unwrap();
    let native_rows = &native["profiles"][expected["profile"].as_str().unwrap()];
    let actual = rows
        .iter()
        .filter_map(|row| row["id"].as_str()?.strip_prefix("platform:"))
        .collect::<Vec<_>>();
    assert_eq!(json!(actual), *native_rows, "{case}");
    let unavailable = rows
        .iter()
        .filter(|row| row["available"] == false)
        .map(|row| {
            row["id"]
                .as_str()
                .unwrap()
                .strip_prefix("platform:")
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(unavailable), expected["unavailable"], "{case}");
    for (id, values) in expected["values"].as_object().unwrap() {
        let row = property_row(palette, id);
        for (key, value) in values.as_object().unwrap() {
            assert_eq!(row["value"][key], *value, "{case}/{id}/{key}");
        }
    }
    for row in rows {
        let Some(id) = row["id"]
            .as_str()
            .and_then(|id| id.strip_prefix("platform:"))
        else {
            continue;
        };
        if let Some(label) = native["captions"].get(id) {
            assert_eq!(row["caption"], *label, "{case}/{id}");
        }
    }
    let categories = palette["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|category| category["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(json!(categories), expected["categories"], "{case}");
}

#[test]
fn table_and_column_palettes_follow_the_native_order_and_effective_binding() {
    let native: Value = serde_json::from_str(CATALOGUE).unwrap();
    let cases = native["rows"].as_array().unwrap();
    for hierarchical in [false, true] {
        let directory = fixture_with_resources(hierarchical);
        let disk_path = directory.0.join("src").join(PATH);
        let before = fs::read(&disk_path).unwrap();
        for locale in ["ru-RU", "en-US"] {
            let mut client = Client::new(&directory.0, locale);
            let (context, node) = connect(&mut client, &directory.0, locale, 42);
            let mut snapshot = client.request("form/open", &params(&context, json!({"node":node})));
            let filtered = cases
                .iter()
                .filter(|row| row["hierarchical"] == hierarchical);
            for (revision, case) in (2..).zip(filtered) {
                let name = case["name"].as_str().unwrap();
                let xml = source(&format!("{name}.xml"));
                snapshot = update_input(&mut client, &context, &snapshot, revision, &xml);
                for expected in case["selections"].as_array().unwrap() {
                    let target = expected["name"].as_str().unwrap();
                    let node = element(&snapshot["model"]["root"], target).unwrap();
                    let palette = inspect_palette(&mut client, &context, &snapshot, node, true);
                    assert_native_palette(&palette, expected, &native, name);
                }
            }
            client.finish();
        }
        assert_eq!(fs::read(&disk_path).unwrap(), before);
    }
}

#[test]
fn table_palette_preserves_authored_values_and_keeps_events_opt_in() {
    let directory = fixture_with_resources(false);
    let mut client = Client::new(&directory.0, "ru-RU");
    let (context, node) = connect(&mut client, &directory.0, "ru-RU", 41);
    let initial = client.request("form/open", &params(&context, json!({"node":node})));
    let baseline = source("ValueTable.xml");
    let changed = baseline.replacen(
        "<DataPath>Rows</DataPath>",
        concat!(
            "<DataPath>Rows</DataPath>",
            "<Width>0</Width><Enabled>false</Enabled><Title/>",
            "<Events><Event name=\"OnActivateRow\">RowsActivated</Event></Events>"
        ),
        1,
    );
    assert_ne!(changed, baseline);
    let snapshot = update_input(&mut client, &context, &initial, 2, &changed);
    let table = element(&snapshot["model"]["root"], "Rows").unwrap();
    let without_events = inspect_palette(&mut client, &context, &snapshot, table, false);
    assert!(
        !without_events["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["category"] == "events")
    );
    let palette = inspect_palette(&mut client, &context, &snapshot, table, true);
    for (id, raw) in [("Width", "0"), ("Enabled", "false"), ("Title", "")] {
        let value = &property_row(&palette, id)["value"];
        assert_eq!(value["kind"], "declared");
        let index = usize::try_from(value["propertyIndex"].as_u64().unwrap()).unwrap();
        assert_eq!(table["properties"][index]["key"]["name"], id);
        if !raw.is_empty() {
            assert_eq!(table["properties"][index]["value"]["text"], raw);
        }
    }
    let event = &property_row(&palette, "OnActivateRow")["value"];
    assert_eq!(event["kind"], "declaredEvent");
    assert_eq!(event["entryIndex"], 0);
    let locate = client.request(
        "form/locate",
        &params(&context, json!({
            "formSessionId":snapshot["formSessionId"],
            "revision":snapshot["revision"],
            "target":{"kind":"property","identity":table["identity"],"property":{"namespace":NS,"name":"Events"}}
        })),
    );
    assert!(locate["range"].is_object());
    assert_eq!(
        property_row(&palette, "Footer")["value"]["default"]["value"],
        false
    );
    let unknown = baseline.replacen(
        "<DataPath>Rows</DataPath>",
        "<DataPath>Missing</DataPath>",
        1,
    );
    let snapshot = update_input(&mut client, &context, &snapshot, 3, &unknown);
    let table = element(&snapshot["model"]["root"], "Rows").unwrap();
    assert!(inspect_palette(&mut client, &context, &snapshot, table, true)["rows"].is_null());
    client.finish();
    assert_eq!(
        fs::read_to_string(directory.0.join("src").join(PATH)).unwrap(),
        baseline
    );
}
