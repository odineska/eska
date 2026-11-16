use std::{collections::BTreeSet, fs, path::Path};

use roxmltree::{Document, Node};
use serde_json::{Value, json};

use super::{Stand, strings};

const FORM: &str = "http://v8.1c.ru/8.3/xcf/logform";
const CORE: &str = "http://v8.1c.ru/8.1/data/core";
const XS: &str = "http://www.w3.org/2001/XMLSchema";
const ID: &str = "common-form:NativeSemantic";

pub fn run() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/eska-debug/tests/fixtures/posting/source");
    let stand = Stand::from_source(&fixture);
    stand.mutate(
        "form-create",
        &strings(&[
            "create",
            "--kind",
            "common-form",
            "--name",
            "NativeSemantic",
        ]),
    );
    edit(&stand, "form-add", &additions());
    add_list_rule(&stand);
    edit(
        &stand,
        "form-appearance",
        &[json!({
            "kind":"setConditionalAppearance","rules":[{
                "original":null,"enabled":true,"fields":[{"name":"Amount","enabled":true}],
                "conditions":[{"kind":"group","group":"AndGroup","enabled":true,"items":[
                    {"kind":"comparison","enabled":true,"field":"Amount","operator":"Less","operand":{"kind":"number","value":"10"}},
                    {"kind":"comparison","enabled":true,"field":"Amount","operator":"Filled","operand":null}
                ]}],
                "format":[{"parameter":"background","enabled":true,"value":{"kind":"color","value":"#AAEECC"}},
                    {"parameter":"visible","enabled":true,"value":{"kind":"boolean","value":true}}]
            }]
        })],
    );
    let initial = schema(&stand);
    reject_structural_removal(&stand, &initial);
    edit(
        &stand,
        "form-copy-move",
        &[
            json!({"kind":"copyElement","target":element(&initial,"Group"),"parent":{"kind":"root"},"name":"GroupCopy"}),
            json!({"kind":"copyElement","target":element(&initial,"TableGroup"),"parent":{"kind":"root"},"name":"TableGroupCopy"}),
            json!({"kind":"moveElement","target":element(&initial,"Group"),"parent":element(&initial,"PageTwo")}),
            json!({"kind":"removeProperty","target":element(&initial,"Amount"),"key":{"namespace":FORM,"name":"Visible"}}),
        ],
    );
    let copied = schema(&stand);
    edit(
        &stand,
        "form-property-remove",
        &[
            json!({"kind":"setProperty","target":element(&copied,"GroupCopy"),"property":text("United","false")}),
            json!({"kind":"remove","target":{"kind":"parameter","name":"Mode"}}),
        ],
    );
    stand.mutate(
        "form-clone",
        &strings(&["copy", "--source", ID, "--name", "NativeSemanticTwin"]),
    );
    let relative = "CommonForms/NativeSemantic/Ext/Form.xml";
    assert_eq!(
        fs::read(stand.source.join(relative)).unwrap(),
        fs::read(
            stand
                .source
                .join("CommonForms/NativeSemanticTwin/Ext/Form.xml")
        )
        .unwrap(),
        "local form IDs and bindings belong to separate form scopes"
    );
    let export = stand.roundtrip("forms", true);
    assert_export(&fs::read_to_string(export.join(relative)).unwrap());
    assert_export(
        &fs::read_to_string(export.join("CommonForms/NativeSemanticTwin/Ext/Form.xml")).unwrap(),
    );
    let current = schema(&stand);
    edit(
        &stand,
        "form-remove-subtree",
        &[
            json!({"kind":"remove","target":element(&current,"GroupCopy")}),
            json!({"kind":"remove","target":element(&current,"TableGroup")}),
        ],
    );
    for id in [ID, "common-form:NativeSemanticTwin"] {
        stand.mutate(
            &format!("delete-{}", id.replace(':', "-")),
            &strings(&["delete", "--object", id, "--yes"]),
        );
    }
    stand.roundtrip("forms-deleted", false);
}

fn schema(stand: &Stand) -> Value {
    stand.cli(
        "en-US",
        &strings(&["form-schema", "--form", ID, "--format", "json"]),
    )
}

fn edit(stand: &Stand, label: &str, operations: &[Value]) {
    let schema = schema(stand);
    let input = json!({"schemaVersion":1,"request":{"form":ID,"snapshot":schema["result"]["snapshot"],"operations":operations}});
    let path = stand.root.0.join(format!("{label}-request.json"));
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    stand.mutate(
        label,
        &strings(&["form-edit", "--input", path.to_str().unwrap()]),
    );
}

fn element(schema: &Value, name: &str) -> Value {
    schema["result"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] == name && node["target"]["kind"] == "element")
        .unwrap_or_else(|| panic!("missing element {name}"))["target"]
        .clone()
}

fn reject_structural_removal(stand: &Stand, schema: &Value) {
    let path = stand.source.join("CommonForms/NativeSemantic/Ext/Form.xml");
    let original = fs::read(&path).unwrap();
    for (target, key) in [
        (json!({"kind":"root"}), "AutoCommandBar"),
        (element(schema, "Amount"), "ContextMenu"),
        (element(schema, "Amount"), "ExtendedTooltip"),
    ] {
        let input = json!({"schemaVersion":1,"request":{"form":ID,"snapshot":schema["result"]["snapshot"],"operations":[{"kind":"removeProperty","target":target,"key":{"namespace":FORM,"name":key}}]}});
        let request = stand.root.0.join("structural-property-request.json");
        fs::write(&request, serde_json::to_vec(&input).unwrap()).unwrap();
        let args = strings(&[
            "form-edit",
            "--input",
            request.to_str().unwrap(),
            "--no-input",
            "--format",
            "json",
        ]);
        let error = stand.cli_error("en-US", &args);
        assert_eq!(error, stand.cli_error("ru-RU", &args));
        assert_eq!(error["error"]["kind"], "form_invalid");
        assert_eq!(error["error"]["details"]["reason"], "structural_property");
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}

fn property(namespace: &str, name: &str, value: &Value) -> Value {
    json!({"key":{"namespace":namespace,"name":name},"qualifiers":[],"value":value})
}

fn text(name: &str, value: &str) -> Value {
    property(FORM, name, &json!({"Text":value}))
}

fn data_type(namespace: &str, prefix: &str, name: &str) -> Value {
    property(
        FORM,
        "Type",
        &json!({"Record":[property(CORE,"Type",&json!({"QualifiedText":{"text":format!("{prefix}:{name}"),"key":{"namespace":namespace,"name":name}}}))]}),
    )
}

fn additions() -> Vec<Value> {
    vec![
        json!({"kind":"addDefinition","section":"attribute","name":"Amount","properties":[data_type(XS,"xs","decimal")]}),
        json!({"kind":"addDefinition","section":"attribute","name":"Rows","properties":[data_type(CORE,"v8","ValueTable")]}),
        json!({"kind":"addDefinition","section":"attribute","parent":{"kind":"attribute","name":"Rows"},"name":"Quantity","properties":[data_type(XS,"xs","decimal")]}),
        json!({"kind":"addDefinition","section":"command","name":"Run"}),
        json!({"kind":"addDefinition","section":"parameter","name":"Mode","properties":[data_type(XS,"xs","string")]}),
        json!({"kind":"addElement","parent":{"kind":"root"},"element":{"kind":"UsualGroup","name":"Group","properties":[text("United","true")],"children":[{"kind":"InputField","name":"Amount","properties":[text("DataPath","Amount"),text("Visible","false")],"auxiliary":[{"kind":"ContextMenu","name":"AmountContext"},{"kind":"ExtendedTooltip","name":"AmountTooltip"}]}]}}),
        json!({"kind":"addElement","parent":{"kind":"root"},"element":{"kind":"Pages","name":"Pages","children":[{"kind":"Page","name":"PageOne","children":[{"kind":"Table","name":"Rows","properties":[text("DataPath","Rows")],"children":[{"kind":"InputField","name":"Quantity","properties":[text("DataPath","Rows.Quantity")]}]}]},{"kind":"Page","name":"PageTwo"}]}}),
        json!({"kind":"addElement","parent":{"kind":"root"},"element":{"kind":"UsualGroup","name":"TableGroup","children":[{"kind":"Table","name":"CopyRows","properties":[text("DataPath","Rows")],"auxiliary":[{"kind":"AutoCommandBar","name":"CopyRowsBar","children":[{"kind":"Button","name":"AddRow","properties":[text("CommandName","Form.Item.CopyRows.StandardCommand.Add")]}]}]},{"kind":"CommandBar","name":"ExplicitBar","properties":[text("CommandSource","Item.CopyRows")]},{"kind":"Button","name":"External","properties":[text("CommandName","Form.Item.Outside.StandardCommand.Add")]}]}}),
        json!({"kind":"addElement","parent":{"kind":"root"},"element":{"kind":"Table","name":"Outside","properties":[text("DataPath","Rows")]}}),
        json!({"kind":"setProperty","target":{"kind":"root"},"property":text("Width","80")}),
        json!({"kind":"setEvent","target":{"kind":"root"},"event":"OnOpen","handler":"NativeOnOpen"}),
    ]
}

fn add_list_rule(stand: &Stand) {
    let settings = json!({
        "key":{"namespace":FORM,"name":"Settings"},
        "qualifiers":[[{"namespace":"http://www.w3.org/2001/XMLSchema-instance","name":"type"},"DynamicList"]],
        "value":{"Record":[text("ManualQuery","false"),text("DynamicDataRead","true"),text("MainTable","Document.PostingProbe")]}
    });
    edit(
        stand,
        "form-list-appearance",
        &[
            json!({"kind":"addDefinition","section":"attribute","name":"PostingRows","properties":[
                data_type("http://v8.1c.ru/8.1/data/enterprise/current-config","cfg","DynamicList"),settings
            ]}),
            json!({"kind":"setListConditionalAppearance","attribute":"PostingRows","rules":[{
                "fields":[{"name":"Number","enabled":true}],"conditions":[{
                    "kind":"comparison","enabled":true,"field":"Number","operator":"InList",
                    "operand":{"kind":"values","value":[
                        {"kind":"string","value":"A001"},
                        {"kind":"string","value":"A002"}
                    ]}
                }],"format":[{"parameter":"background","enabled":true,"value":{"kind":"color","value":"#112233"}}]
            }]}),
        ],
    );
}

fn assert_export(xml: &str) {
    assert!(xml.contains("ConditionalAppearance"));
    assert!(xml.contains("#AAEECC"));
    assert!(xml.contains("FilterItemGroup"));
    assert!(xml.contains(">Amount</dcsset:field>"));
    let document = Document::parse(xml).unwrap();
    let root = document.root_element();
    let dynamic = root
        .descendants()
        .find(|node| node.attribute("name") == Some("PostingRows"))
        .unwrap();
    let dynamic = &xml[dynamic.range()];
    for value in ["InList", "A001", "A002", "#112233"] {
        assert!(
            dynamic.contains(value),
            "list appearance lost {value}: {dynamic}"
        );
    }
    assert_eq!(
        root.children()
            .find(|node| node.has_tag_name((FORM, "Width")))
            .unwrap()
            .text(),
        Some("80")
    );
    let nodes: Vec<_> = document.descendants().filter(Node::is_element).collect();
    assert_table_copy(&nodes);
    let group = nodes
        .iter()
        .find(|node| node.attribute("name") == Some("Group"))
        .unwrap();
    assert_eq!(
        group
            .parent_element()
            .unwrap()
            .parent_element()
            .unwrap()
            .attribute("name"),
        Some("PageTwo")
    );
    let copy = nodes
        .iter()
        .find(|node| node.attribute("name") == Some("GroupCopy"))
        .unwrap();
    assert_eq!(
        copy.children()
            .find(|node| node.has_tag_name((FORM, "United")))
            .unwrap()
            .text(),
        Some("false")
    );
    assert!(
        nodes
            .iter()
            .any(|node| node.has_tag_name((FORM, "DataPath"))
                && node.text() == Some("Rows.Quantity"))
    );
    assert!(
        nodes.iter().any(|node| node.has_tag_name((FORM, "Column"))
            && node.attribute("name") == Some("Quantity"))
    );
    assert!(nodes.iter().any(|node| node.has_tag_name((FORM, "Event"))
        && node.attribute("name") == Some("OnOpen")
        && node.text() == Some("NativeOnOpen")));
    assert!(!nodes.iter().any(
        |node| node.has_tag_name((FORM, "Parameter")) && node.attribute("name") == Some("Mode")
    ));
    let elements = root
        .children()
        .find(|node| node.has_tag_name((FORM, "ChildItems")))
        .unwrap();
    let mut ids = BTreeSet::new();
    for node in elements.descendants().filter(Node::is_element) {
        if let Some(id) = node.attribute("id").and_then(|id| id.parse::<u64>().ok()) {
            assert!(ids.insert(id), "duplicate positive form element ID {id}");
        }
    }
}

fn assert_table_copy(nodes: &[Node<'_, '_>]) {
    let copy = nodes
        .iter()
        .find(|node| node.attribute("name") == Some("TableGroupCopy"))
        .unwrap();
    for (tag, value) in [
        ("CommandSource", "Item.CopyRows_Copy1"),
        (
            "CommandName",
            "Form.Item.CopyRows_Copy1.StandardCommand.Add",
        ),
        ("CommandName", "Form.Item.Outside.StandardCommand.Add"),
    ] {
        assert!(
            copy.descendants()
                .any(|node| node.has_tag_name((FORM, tag)) && node.text() == Some(value)),
            "copied table command binding {tag}={value} survives native export"
        );
    }
}
