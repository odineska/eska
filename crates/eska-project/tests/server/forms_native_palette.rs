use super::*;

pub(super) fn element<'a>(root: &'a Value, name: &str) -> Option<&'a Value> {
    if root["name"] == name {
        return Some(root);
    }
    ["children", "auxiliary"]
        .into_iter()
        .flat_map(|slot| root[slot].as_array().into_iter().flatten())
        .find_map(|child| element(child, name))
}

pub(super) fn property_row<'a>(palette: &'a Value, id: &str) -> &'a Value {
    let key = format!("platform:{id}");
    palette["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == key)
        .unwrap_or_else(|| panic!("missing platform palette row {id}"))
}

pub(super) fn assert_native_palette(palette: &Value, expected: &Value, native: &Value, case: &str) {
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
