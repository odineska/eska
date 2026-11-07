use super::*;
use crate::project::{
    forms::{ElementKey, ElementPresentation, compose, parse, present},
    metadata_parser::{self, PropertiesMode},
};

fn resource(kind: &str, name: &str, properties: &str) -> FormResource {
    let xml = format!(
        r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:xr="http://v8.1c.ru/8.3/xcf/readable"><{kind} uuid="{name}"><Properties><Name>{name}</Name>{properties}</Properties></{kind}></MetaDataObject>"#
    );
    FormResource {
        layer_source: "base".into(),
        source: format!("{kind}.{name}"),
        style: None,
        picture: None,
        retained_bytes: xml.len(),
        references: Vec::new(),
        descriptor: metadata_parser::parse(&xml, None, PropertiesMode::All).unwrap(),
    }
}

fn presentations(
    main_table: &str,
    properties: &str,
    resources: &[FormResource],
) -> Vec<ElementPresentation> {
    let xml = format!(
        r#"<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config">
        <ChildItems><Table name="Rows" id="11"><DataPath>Rows</DataPath>{properties}<ChildItems>
        <InputField name="Code" id="12"><DataPath>Rows.Code</DataPath></InputField></ChildItems></Table>
        <CommandBar name="External" id="13"><CommandSource>Item.Rows</CommandSource></CommandBar></ChildItems>
        <Attributes><Attribute name="Rows" id="1"><Type><v8:Type>cfg:DynamicList</v8:Type></Type>
        <Settings><MainTable>{main_table}</MainTable></Settings></Attribute></Attributes></Form>"#
    );
    let document = parse("form", xml).unwrap();
    present(&compose(&document, &[]), &[document], resources).elements
}

fn keys(groups: &[Vec<AutoCommandItem>]) -> Vec<Vec<&str>> {
    groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|item| item.key.strip_prefix("Table.StandardCommand.").unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn catalog_menu_contains_native_actions_sections_shortcuts_and_search_submenu() {
    let resources = [resource(
        "Catalog",
        "Items",
        "<DataHistory>DontUse</DataHistory>",
    )];
    let views = presentations("Catalog.Items", "", &resources);
    let table = &views[1];
    assert_eq!(table.identity.key, ElementKey::Xml(11));
    assert_eq!(
        keys(&table.table_command_menu),
        [
            vec![
                "Create",
                "Copy",
                "Change",
                "SetDeletionMark",
                "Delete",
                "Refresh",
                "FindByCurrentValue",
                "Find",
                "CancelSearch",
                "ChangeHistory",
                "GetCurrentRowURL"
            ],
            vec![
                "ListSettings",
                "SelectSettings",
                "SaveSettings",
                "RestoreDefaultSettings"
            ],
            vec!["OutputList", "SearchControl"],
        ]
    );
    let primary = &table.table_command_menu[0];
    assert_eq!(primary[0].shortcut, Some("Ins"));
    assert_eq!(primary[4].shortcut, Some("Shift+Del"));
    assert!(
        matches!(&primary[7].caption, PresentationText::Standard {key} if key == "command.AdvancedSearch")
    );
    let search = &table.table_command_menu[2][1].items;
    assert_eq!(search.len(), 4);
    assert_eq!(search[2].enabled, Some(false));
    assert_eq!(search[2].begin_group, Some(true));
    assert_eq!(search[3].begin_group, Some(true));
    assert_eq!(
        keys(
            &views
                .iter()
                .find(|v| v.identity.key == ElementKey::Xml(13))
                .unwrap()
                .table_command_menu
        ),
        keys(&table.table_command_menu)
    );
    let read_only = presentations("Catalog.Items", "<ReadOnly>true</ReadOnly>", &resources);
    assert_eq!(
        keys(&read_only[1].table_command_menu),
        keys(&table.table_command_menu)
    );
}

#[test]
fn hierarchy_document_register_and_query_menus_follow_their_sources() {
    let hierarchy = presentations(
        "Catalog.Items",
        "",
        &[resource(
            "Catalog",
            "Items",
            "<Hierarchical>true</Hierarchical><HierarchyType>HierarchyFoldersAndItems</HierarchyType>",
        )],
    );
    assert_eq!(
        &keys(&hierarchy[1].table_command_menu)[0][..11],
        [
            "Create",
            "CreateFolder",
            "Copy",
            "Change",
            "SetDeletionMark",
            "Delete",
            "Refresh",
            "ViewMode",
            "MoveItem",
            "LevelDown",
            "LevelUp"
        ]
    );
    assert_eq!(hierarchy[1].table_command_menu[0][7].items.len(), 3);
    let without_hierarchy_actions = presentations(
        "Catalog.Items",
        "<CommandSet><ExcludedCommand>MoveItem</ExcludedCommand><ExcludedCommand>HierarchicalList</ExcludedCommand><ExcludedCommand>List</ExcludedCommand><ExcludedCommand>Tree</ExcludedCommand></CommandSet>",
        &[resource(
            "Catalog",
            "Items",
            "<Hierarchical>true</Hierarchical>",
        )],
    );
    assert!(
        !keys(&without_hierarchy_actions[1].table_command_menu)[0]
            .iter()
            .any(|key| matches!(*key, "ViewMode" | "MoveItem"))
    );
    let document = presentations(
        "Document.Invoice",
        "",
        &[resource("Document", "Invoice", "<Posting>Allow</Posting>")],
    );
    assert_eq!(
        keys(&document[1].table_command_menu)[1..3],
        [vec!["SetPeriod"], vec!["Post", "UndoPosting"]]
    );
    let unposted = presentations(
        "Document.Invoice",
        "",
        &[resource("Document", "Invoice", "<Posting>Deny</Posting>")],
    );
    assert!(
        !keys(&unposted[1].table_command_menu)
            .iter()
            .flatten()
            .any(|key| *key == "Post")
    );
    let register = presentations(
        "InformationRegister.Rates",
        "",
        &[resource(
            "InformationRegister",
            "Rates",
            "<InformationRegisterPeriodicity>Day</InformationRegisterPeriodicity>",
        )],
    );
    assert_eq!(
        keys(&register[1].table_command_menu)[0],
        ["Create", "Copy", "Change", "Delete", "Refresh"]
    );
    assert_eq!(register[1].table_command_menu[0][3].shortcut, Some("Del"));
    assert_eq!(keys(&register[1].table_command_menu)[1], ["SetPeriod"]);
    let query = presentations("", "", &[]);
    assert_eq!(
        keys(&query[1].table_command_menu)[0],
        ["Refresh", "FindByCurrentValue", "Find", "CancelSearch"]
    );
}

#[test]
fn excluded_actions_search_placement_and_autofill_apply_to_both_menu_levels() {
    let resources = [resource("Catalog", "Items", "")];
    let excluded = [
        "Create",
        "Find",
        "CancelSearch",
        "Refresh",
        "ListSettings",
        "OutputList",
        "SetDeletionMark",
        "ChangeHistory",
    ]
    .map(|name| format!("<ExcludedCommand>{name}</ExcludedCommand>"))
    .join("");
    let views = presentations(
        "Catalog.Items",
        &format!(
            "<CommandSet>{excluded}</CommandSet><AllowGettingCurrentRowURL>false</AllowGettingCurrentRowURL>"
        ),
        &resources,
    );
    assert_eq!(
        keys(&views[1].table_command_menu),
        [
            vec!["Copy", "Change", "Delete", "FindByCurrentValue"],
            vec!["SelectSettings", "SaveSettings", "RestoreDefaultSettings"],
            vec!["SearchControl"],
        ]
    );
    assert_eq!(views[1].table_command_menu[2][0].items.len(), 2);
    let search = presentations(
        "Catalog.Items",
        "<SearchStringLocation>None</SearchStringLocation><SearchControlLocation>None</SearchControlLocation>",
        &resources,
    );
    let find = &search[1].table_command_menu[0][7];
    assert!(matches!(&find.caption, PresentationText::Standard { key } if key == "command.Find"));
    assert_eq!(find.shortcut, Some("Ctrl+F"));
    assert_eq!(keys(&search[1].table_command_menu)[2], ["OutputList"]);
    let manual = presentations(
        "Catalog.Items",
        "<AutoCommandBar name=\"Bar\" id=\"14\"><Autofill>false</Autofill></AutoCommandBar>",
        &resources,
    );
    assert!(manual[1].table_command_menu.is_empty());
    assert!(
        !manual
            .iter()
            .find(|view| view.identity.key == ElementKey::Xml(13))
            .unwrap()
            .table_command_menu
            .is_empty()
    );
}

#[test]
fn journal_creation_entries_use_effective_registered_documents() {
    let resources = [
        resource(
            "DocumentJournal",
            "Sales",
            "<RegisteredDocuments><xr:Item>Document.Invoice</xr:Item><xr:Item>Document.Missing</xr:Item></RegisteredDocuments>",
        ),
        resource(
            "Document",
            "Invoice",
            "<ObjectPresentation>Invoice entry</ObjectPresentation>",
        ),
    ];
    let views = presentations("DocumentJournal.Sales", "", &resources);
    let create = &views[1].table_command_menu[0][0];
    assert_eq!(create.shortcut, None);
    assert_eq!(create.items.len(), 1);
    assert!(
        matches!(&create.items[0].caption, PresentationText::Name { text } if text == "Invoice entry")
    );
    assert_eq!(
        create.items[0].origin.as_ref().unwrap().source,
        "Document.Invoice"
    );
}
