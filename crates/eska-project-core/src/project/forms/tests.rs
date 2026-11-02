use super::*;
use crate::project::metadata_model::MetadataValue;

fn form(body: &str) -> String {
    format!(r#"<Form xmlns="{FORM_NAMESPACE}" version="2.20">{body}</Form>"#)
}

#[test]
fn catalog_matches_installed_platform_inventory() {
    let catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/forms/catalog.json"
    ))
    .unwrap();
    let expected: std::collections::BTreeSet<_> = catalog["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["xmlTag"].as_str().unwrap())
        .collect();
    let actual: std::collections::BTreeSet<_> = ElementKind::ALL
        .iter()
        .map(|kind| kind.xml_name())
        .collect();
    assert_eq!(expected, actual);
    assert_eq!(actual.len(), 38);
    assert_eq!(
        ElementKind::from_xml_name("SpreadSheetDocumentField"),
        Some(ElementKind::SpreadsheetDocumentField)
    );
    assert_eq!(ElementKind::from_xml_name("SpreadsheetDocumentField"), None);
}

#[test]
fn native_exported_field_fixtures_keep_platform_generated_helpers() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/forms/native-fields");
    let mut fields = 0;
    let mut pdf_status = false;
    for path in std::fs::read_dir(folder).unwrap() {
        let path = path.unwrap().path();
        if path.extension().is_none_or(|value| value != "xml") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        let input = std::fs::read_to_string(&path).unwrap();
        let model = parse(name, input.clone()).unwrap();
        assert!(!model.has_issues(), "{name}: {:?}", model.diagnostics);
        assert_eq!(model.input(), input);
        assert_eq!(model.root.children.len(), 1);
        let field = &model.root.children[0];
        assert_eq!(field.kind.unwrap().xml_name(), name);
        if name == "PDFDocumentField" {
            pdf_status = field
                .auxiliary
                .iter()
                .any(|item| item.kind == Some(ElementKind::ViewStatusAddition));
        }
        fields += 1;
    }
    assert_eq!(fields, 20);
    assert!(
        pdf_status,
        "the platform adds a view-status helper to an empty PDF field"
    );
}

#[test]
fn source_ranges_preserve_bom_comments_unicode_and_crlf() {
    let input = format!(
        "\u{feff}<?xml version=\"1.0\"?>\r\n<Form xmlns=\"{FORM_NAMESPACE}\">\r\n<!-- private source comment 😀 -->\r\n<ChildItems><InputField name=\"Имя😀\" id=\"1\"><Title><![CDATA[Текст & 😀]]></Title></InputField></ChildItems></Form>"
    );
    let parsed = parse("context/base/form", input.clone()).unwrap();
    assert_eq!(parsed.input(), input);
    assert!(!parsed.has_issues());
    let field = &parsed.root.children[0];
    assert_eq!(field.name.as_deref(), Some("Имя😀"));
    assert_eq!(field.kind, Some(ElementKind::InputField));
    assert!(input[field.range.clone()].starts_with("<InputField"));
    let title = &field.properties[0];
    assert_eq!(
        &input[title.range.clone()],
        "<Title><![CDATA[Текст & 😀]]></Title>"
    );
    assert_eq!(
        title.property.value,
        MetadataValue::Text("Текст & 😀".into())
    );
    let json = serde_json::to_string(&parsed).unwrap();
    assert!(!json.contains("private source comment"));
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(!json.as_object().unwrap().contains_key("input"));
}

#[test]
fn layout_children_and_auxiliary_elements_keep_separate_roles() {
    let parsed = parse(
        "form",
        form(
            r#"<AutoCommandBar name="Bar" id="-1"/>
        <ChildItems><Table name="Rows" id="1"><AutoCommandBar name="RowsBar" id="2"/>
        <ContextMenu name="Menu" id="3"/><SearchStringAddition name="Search" id="4"/>
        <ExtendedTooltip name="Tip" id="5"/><ChildItems><ColumnGroup name="Columns" id="6">
        <ChildItems><InputField name="Value" id="7"><DataPath>Rows.Value</DataPath></InputField>
        </ChildItems></ColumnGroup></ChildItems></Table></ChildItems>"#,
        ),
    )
    .unwrap();
    assert!(!parsed.has_issues());
    assert_eq!(
        parsed.root.auxiliary[0].kind,
        Some(ElementKind::AutoCommandBar)
    );
    let table = &parsed.root.children[0];
    assert_eq!(table.children.len(), 1);
    assert_eq!(table.auxiliary.len(), 4);
    assert_eq!(table.children[0].kind, Some(ElementKind::ColumnGroup));
    assert_eq!(table.children[0].children[0].name.as_deref(), Some("Value"));
}

#[test]
fn definition_ids_do_not_collide_with_element_ids() {
    let parsed = parse(
        "form",
        form(
            r#"<Attributes><Attribute name="Rows" id="1">
        <Columns><Column name="N" id="1"><Type xmlns:v="http://v8.1c.ru/8.1/data/core"
        xmlns:s="http://www.w3.org/2001/XMLSchema"><v:Type>s:string</v:Type></Type>
        </Column></Columns></Attribute></Attributes>
        <Commands><Command name="Run" id="1"><Action>Handle</Action></Command></Commands>
        <Parameters><Parameter name="P"><KeyParameter>true</KeyParameter></Parameter></Parameters>
        <ChildItems><InputField name="Field" id="1"/></ChildItems>"#,
        ),
    )
    .unwrap();
    assert!(!parsed.has_issues());
    assert_eq!(parsed.root.children.len(), 1);
    assert_eq!(parsed.attributes[0].name.as_deref(), Some("Rows"));
    assert_eq!(parsed.commands[0].name.as_deref(), Some("Run"));
    assert_eq!(parsed.parameters[0].name.as_deref(), Some("P"));
    let MetadataValue::Record(columns) = &parsed.attributes[0].properties[0].property.value else {
        panic!("column definitions must remain structured metadata");
    };
    assert_eq!(columns[0].key.name, "Column");
}

#[test]
fn identities_are_source_qualified_and_root_is_not_an_xml_id() {
    let input = form(r#"<ChildItems><InputField name="Field" id="0"/></ChildItems>"#);
    let base = parse("application/base/form", input.clone()).unwrap();
    let extension = parse("application/extension/form", input).unwrap();
    let root = base.root.identity.as_ref().unwrap();
    let id = base.root.children[0].identity.as_ref().unwrap();
    let foreign_id = extension.root.children[0].identity.as_ref().unwrap();
    assert_ne!(root, id);
    assert_ne!(id, foreign_id);
    assert!(base.element(id).is_some());
    assert!(base.element(foreign_id).is_none());
}

#[test]
fn namespace_collisions_and_unknown_nodes_remain_explicit() {
    let parsed = parse(
        "form",
        form(
            r#"<ChildItems><InputField name="Field" id="1" xmlns:f="urn:foreign">
        <f:Width>900</f:Width><Width>12</Width><Custom>keep</Custom></InputField>
        <x:InputField xmlns:x="urn:foreign" name="Foreign" id="2"/>
        <FutureField name="Future" id="3"><Unknown enabled="true">opaque</Unknown></FutureField>
        </ChildItems>"#,
        ),
    )
    .unwrap();
    assert_eq!(parsed.root.children.len(), 3);
    assert_eq!(parsed.root.children[1].kind, None);
    assert_eq!(parsed.root.children[2].kind, None);
    assert_eq!(
        parsed.root.children[0].property("Width").unwrap().value,
        MetadataValue::Text("12".into())
    );
    assert_eq!(parsed.diagnostics.len(), 2);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|row| row.issue == FormIssue::UnknownElement)
    );
    assert!(parsed.input().contains("opaque"));
}

#[test]
fn duplicate_or_invalid_identities_never_create_synthetic_ids() {
    let parsed = parse(
        "form",
        form(
            r#"<AutoCommandBar name="Bar" id="-1"/><ChildItems>
        <Button name="Duplicate" id="-1"/><InputField name="Missing"/>
        <InputField name="Invalid" id="not-an-integer"/><InputField id="7"/></ChildItems>"#,
        ),
    )
    .unwrap();
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|row| row.issue == FormIssue::DuplicateIdentity)
    );
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .filter(|row| row.issue == FormIssue::InvalidIdentity)
            .count(),
        2
    );
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|row| row.issue == FormIssue::MissingName)
    );
    assert!(parsed.root.children[1].identity.is_none());
    assert!(parsed.root.children[2].identity.is_none());
}

#[test]
fn unsupported_content_and_repeated_sections_are_not_silently_lost() {
    let parsed = parse(
        "form",
        form(
            r#"<Custom>before<Unknown/>after</Custom>
        <ChildItems>unexpected text</ChildItems><ChildItems/>
        <Attributes><x:Attribute xmlns:x="urn:foreign" name="Foreign"/></Attributes>"#,
        ),
    )
    .unwrap();
    for issue in [
        FormIssue::UnsupportedProperty,
        FormIssue::UnsupportedStructure,
        FormIssue::RepeatedSection,
        FormIssue::UnknownDefinition,
    ] {
        assert!(
            parsed.diagnostics.iter().any(|row| row.issue == issue),
            "{issue:?}"
        );
    }
    assert_eq!(
        parsed.attributes[0].xml_name.namespace.as_deref(),
        Some("urn:foreign")
    );
}

#[test]
fn malformed_deep_and_external_entity_inputs_are_rejected() {
    assert_eq!(
        parse("", form("")).unwrap_err(),
        FormParseError::InvalidSource
    );
    assert_eq!(
        parse("form", "<Form>".into()).unwrap_err(),
        FormParseError::InvalidXml
    );
    assert_eq!(
        parse("form", "<Form/>".into()).unwrap_err(),
        FormParseError::UnsupportedRoot
    );
    let deep = form(&format!("{}{}", "<X>".repeat(70), "</X>".repeat(70)));
    assert_eq!(parse("form", deep).unwrap_err(), FormParseError::TooDeep);
    assert_eq!(
        parse("form", form(&" ".repeat(MAX_FORM_BYTES))).unwrap_err(),
        FormParseError::TooLarge
    );
    let external = format!(
        r#"<!DOCTYPE Form [<!ENTITY external SYSTEM "file:///etc/passwd">]><Form xmlns="{FORM_NAMESPACE}">&external;</Form>"#
    );
    assert_eq!(
        parse("form", external).unwrap_err(),
        FormParseError::InvalidXml
    );
}

#[test]
fn content_revisions_do_not_change_xml_identity() {
    let first = parse(
        "form",
        form(r#"<ChildItems><Button name="Run" id="1"><Title>A</Title></Button></ChildItems>"#),
    )
    .unwrap();
    let second = parse("form", first.input().replace("<Title>A", "<Title>B")).unwrap();
    assert_ne!(first.revision, second.revision);
    assert_eq!(
        first.root.children[0].identity,
        second.root.children[0].identity
    );
}

#[test]
#[ignore = "requires an explicitly supplied read-only Designer XML corpus"]
fn installed_designer_form_corpus() {
    let root = std::path::PathBuf::from(std::env::var_os("ESKA_FORM_CORPUS").unwrap());
    let mut pending = vec![root];
    let mut forms = 0;
    let mut issues = std::collections::BTreeMap::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        } else if path.file_name().is_some_and(|name| name == "Form.xml") {
            let input = std::fs::read_to_string(&path).unwrap();
            let source = path.to_string_lossy();
            let result =
                parse(&source, input).unwrap_or_else(|error| panic!("{path:?}: {error:?}"));
            for row in &result.diagnostics {
                *issues.entry(format!("{:?}", row.issue)).or_insert(0) += 1;
            }
            forms += 1;
        }
    }
    assert!(forms > 0, "the supplied corpus must contain forms");
    println!("parsed {forms} forms; diagnostics: {issues:?}");
    assert!(
        issues.is_empty(),
        "known Designer forms must have no structural diagnostics"
    );
}
