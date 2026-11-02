use super::*;
use crate::project::{
    forms::{compose, parse},
    metadata_parser::{self, PropertiesMode},
};
#[test]
fn native_basic_fixture_uses_horizontal_group_and_vertical_form_defaults() {
    let document = parse(
        "basic",
        include_str!("../../../../../../tests/fixtures/forms/native-basic.xml").into(),
    )
    .unwrap();
    let view = present(&compose(&document, &[]), &[document], &[]);
    assert_eq!(view.elements[0].grouping, FormGrouping::Vertical);
    assert_eq!(view.elements[1].grouping, FormGrouping::Horizontal);
    assert_eq!(
        view.elements[2].foreground,
        ResolvedColor::Role("TextColor".into())
    );
    assert_eq!(
        view.elements[3].title_color,
        ResolvedColor::Role("FormTextColor".into())
    );
}
#[test]
fn borrowed_standard_attribute_uses_base_properties_omitted_by_extension() {
    let xml = |properties: &str| {
        format!(
            "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><Catalog uuid=\"catalog\"><Properties><Name>Goods</Name>{properties}</Properties></Catalog></MetaDataObject>"
        )
    };
    let resource = |source: &str, properties: &str| FormResource {
        picture: None,
        retained_bytes: 0,
        source: source.into(),
        references: Vec::new(),
        descriptor: metadata_parser::parse(&xml(properties), None, PropertiesMode::All).unwrap(),
    };
    let resources = [
        resource(
            "base",
            "<HierarchyType>HierarchyFoldersAndItems</HierarchyType><CodeLength>9</CodeLength><CodeType>Number</CodeType>",
        ),
        resource("ext", ""),
    ];
    let owner = &resources[1].descriptor.objects[0];
    assert!(
        matches!(standard_caption("Parent",owner,&resources), PresentationText::Standard { key } if key == "attribute.Folder")
    );
    let value = standard_type("Code", owner, &resources);
    assert_eq!(value.kind, FieldValueKind::Number);
    assert_eq!(value.length, Some(9));
}

#[test]
fn unbound_fields_are_retained_in_the_tree_but_not_displayed() {
    use std::fmt::Write;
    let child_items = super::super::ElementKind::ALL
        .iter()
        .filter(|kind| kind.family() == super::super::ElementFamily::Field)
        .enumerate()
        .fold(String::new(), |mut text, (index, kind)| {
            write!(
                text,
                "<{} name=\"Control{index}\" id=\"{index}\"/>",
                kind.xml_name()
            )
            .unwrap();
            text
        });
    let xml = format!(
        "<Form xmlns=\"{}\"><ChildItems>{child_items}<InputField name=\"Bound\" id=\"99\"><DataPath>Value</DataPath></InputField><InputField name=\"Missing\" id=\"100\"><DataPath>NoSuchAttribute</DataPath></InputField></ChildItems><Attributes><Attribute name=\"Value\" id=\"1\"/></Attributes></Form>",
        super::super::FORM_NAMESPACE
    );
    let document = super::super::parse("own", xml).unwrap();
    let form = super::super::compose(&document, &[]);
    let presentation = super::super::present(&form, &[document], &[]);
    assert_eq!(form.root.children.len(), 22);
    assert!(
        presentation.elements[1..21]
            .iter()
            .all(|element| !element.displayed)
    );
    assert!(presentation.elements[21].displayed);
    assert!(!presentation.elements[22].displayed);
    assert_eq!(presentation.diagnostics.len(), 1);
    assert_eq!(presentation.diagnostics[0].code, "binding_missing");
}

#[test]
fn native_bound_field_fixtures_resolve_their_attributes_and_title_defaults() {
    use super::FieldTitleLocation;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/forms/native-bound-fields");
    let mut count = 0;
    for path in std::fs::read_dir(root).unwrap() {
        let path = path.unwrap().path();
        let document = super::super::parse("own", std::fs::read_to_string(&path).unwrap()).unwrap();
        let form = super::super::compose(&document, &[]);
        let presentation = super::super::present(&form, &[document], &[]);
        assert!(presentation.diagnostics.is_empty(), "{path:?}");
        let field = &presentation.elements[1];
        assert!(field.binding.is_some(), "{path:?}");
        assert!(field.displayed, "{path:?}");
        let expected = if matches!(
            path.file_stem().unwrap().to_str().unwrap(),
            "InputField"
                | "CheckBoxField"
                | "LabelField"
                | "RadioButtonField"
                | "ProgressBarField"
                | "TrackBarField"
        ) {
            FieldTitleLocation::Left
        } else {
            FieldTitleLocation::Top
        };
        assert_eq!(field.title_location, expected, "{path:?}");
        count += 1;
    }
    assert_eq!(count, 13);
}
