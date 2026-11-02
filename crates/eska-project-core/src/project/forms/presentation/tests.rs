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
            "InputField" | "CheckBoxField" | "LabelField" | "RadioButtonField" | "ProgressBarField"
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

fn binding_resource(source: &str, properties: &str, children: &str) -> FormResource {
    let xml = format!(
        "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" xmlns:xr=\"http://v8.1c.ru/8.3/xcf/readable\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><Catalog uuid=\"{source}\"><Properties><Name>Goods</Name>{properties}</Properties><ChildObjects>{children}</ChildObjects></Catalog></MetaDataObject>"
    );
    FormResource {
        source: source.into(),
        descriptor: metadata_parser::parse(&xml, None, PropertiesMode::All).unwrap(),
        references: Vec::new(),
        picture: None,
        retained_bytes: 0,
    }
}
fn binding_view(resources: &[FormResource], fields: &str) -> FormPresentation {
    let xml = format!(
        "<Form xmlns=\"{}\" xmlns:v8=\"http://v8.1c.ru/8.1/data/core\" xmlns:cfg=\"{}\" xmlns:foreign=\"urn:foreign\"><ChildItems>{fields}</ChildItems><Attributes><Attribute name=\"Object\" id=\"1\"><Type><v8:Type>cfg:CatalogObject.Goods</v8:Type></Type></Attribute></Attributes></Form>",
        super::super::FORM_NAMESPACE,
        super::super::references::CONFIG
    );
    let document = parse("own", xml).unwrap();
    present(&compose(&document, &[]), &[document], resources)
}

#[test]
fn borrowed_attribute_inherits_type_and_fill_check_but_overrides_its_caption() {
    let resources = [
        binding_resource(
            "base",
            "",
            "<Attribute uuid=\"base-value\"><Properties><Name>Value</Name><Type><v8:Type>xs:string</v8:Type><v8:StringQualifiers><v8:Length>35</v8:Length></v8:StringQualifiers></Type><FillChecking>ShowError</FillChecking><Synonym><v8:item><v8:lang>ru</v8:lang><v8:content>Исходное</v8:content></v8:item></Synonym></Properties></Attribute>",
        ),
        binding_resource(
            "extension",
            "",
            "<Attribute uuid=\"borrowed-value\"><Properties><Name>Value</Name><Synonym><v8:item><v8:lang>ru</v8:lang><v8:content>Изменённое</v8:content></v8:item></Synonym></Properties></Attribute>",
        ),
    ];
    let view = binding_view(
        &resources,
        "<InputField name=\"Field\" id=\"1\"><DataPath>Object.Value</DataPath></InputField>",
    );
    let field = &view.elements[1];
    assert_eq!(field.value_type.kind, FieldValueKind::String);
    assert_eq!(field.value_type.length, Some(35));
    assert!(
        matches!(&field.caption, PresentationText::Literal { items } if items[0].content=="Изменённое")
    );
    assert_eq!(field.binding.as_ref().unwrap().source, "extension");
    assert!(field.mark_incomplete);
}

#[test]
fn standard_attribute_properties_merge_individually_across_partial_extension_records() {
    let resources = [
        binding_resource(
            "base",
            "<StandardAttributes><xr:StandardAttribute name=\"Description\"><xr:FillChecking>ShowError</xr:FillChecking><xr:Synonym><v8:item><v8:lang>ru</v8:lang><v8:content>Описание базы</v8:content></v8:item></xr:Synonym></xr:StandardAttribute><xr:StandardAttribute name=\"Code\"><xr:FillChecking>ShowError</xr:FillChecking></xr:StandardAttribute></StandardAttributes>",
            "",
        ),
        binding_resource(
            "extension",
            "<StandardAttributes><xr:StandardAttribute name=\"Description\"><xr:FillChecking>DontCheck</xr:FillChecking></xr:StandardAttribute></StandardAttributes>",
            "",
        ),
    ];
    let view = binding_view(
        &resources,
        "<InputField name=\"Name\" id=\"1\"><DataPath>Object.Description</DataPath></InputField><InputField name=\"Code\" id=\"2\"><DataPath>Object.Code</DataPath></InputField>",
    );
    assert!(!view.elements[1].mark_incomplete);
    assert!(
        matches!(&view.elements[1].caption, PresentationText::Literal {items} if items[0].content=="Описание базы")
    );
    assert!(
        view.elements[2].mark_incomplete,
        "an omitted standard attribute keeps its base check"
    );
}

#[test]
fn incomplete_marks_follow_field_overrides_and_ignore_foreign_namespace_properties() {
    let resources = [binding_resource(
        "base",
        "<StandardAttributes><xr:StandardAttribute name=\"Description\"><xr:FillChecking>ShowError</xr:FillChecking></xr:StandardAttribute></StandardAttributes>",
        "",
    )];
    let view = binding_view(
        &resources,
        "<InputField name=\"Inherited\" id=\"1\"><DataPath>Object.Description</DataPath><foreign:AutoMarkIncomplete>false</foreign:AutoMarkIncomplete><foreign:TitleLocation>Top</foreign:TitleLocation></InputField><InputField name=\"Off\" id=\"2\"><DataPath>Object.Description</DataPath><AutoMarkIncomplete>false</AutoMarkIncomplete></InputField><InputField name=\"On\" id=\"3\"><DataPath>Object.Code</DataPath><AutoMarkIncomplete>true</AutoMarkIncomplete></InputField>",
    );
    assert!(view.elements[1].mark_incomplete);
    assert_eq!(view.elements[1].title_location, FieldTitleLocation::Left);
    assert!(!view.elements[2].mark_incomplete);
    assert!(view.elements[3].mark_incomplete);
    for (xml, marked) in [
        (
            include_str!(
                "../../../../../../tests/fixtures/forms/native-field-overrides/InputMarked.xml"
            ),
            true,
        ),
        (
            include_str!(
                "../../../../../../tests/fixtures/forms/native-field-overrides/InputUnmarked.xml"
            ),
            false,
        ),
    ] {
        let document = parse("native", xml.into()).unwrap();
        let view = present(&compose(&document, &[]), &[document], &[]);
        assert_eq!(view.elements[1].mark_incomplete, marked);
        assert!(view.elements[1].displayed);
    }
}
