use super::*;
use crate::project::{
    forms::{FORM_NAMESPACE, compose, parse, present},
    metadata_parser::{self, PropertiesMode},
};

fn resource(name: &str, value: &str) -> FormResource {
    let text = format!(
        "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" xmlns:s=\"{}\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:u=\"http://v8.1c.ru/8.1/data/ui\"><StyleItem uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>{name}</Name>{value}</Properties></StyleItem></MetaDataObject>",
        references::STYLE
    );
    let document = roxmltree::Document::parse(&text).unwrap();
    FormResource {
        picture: None,
        retained_bytes: 0,
        source: name.into(),
        descriptor: metadata_parser::parse(&text, None, PropertiesMode::All).unwrap(),
        references: references::collect(document.root_element()),
    }
}
fn view(body: &str, resources: &[FormResource]) -> super::super::FormPresentation {
    let document = parse("form", format!("<Form xmlns=\"{FORM_NAMESPACE}\" xmlns:s=\"{}\"><ChildItems><InputField name=\"A\" id=\"1\">{body}</InputField></ChildItems></Form>", references::STYLE)).unwrap();
    present(&compose(&document, &[]), &[document], resources)
}
#[test]
fn custom_style_item_partial_font_and_explicit_color_resolve_with_provenance() {
    let resources = [
        resource(
            "Important",
            "<Value xsi:type=\"u:Font\" ref=\"s:NormalTextFont\" bold=\"true\" scale=\"120\"/>",
        ),
        resource("Positive", "<Value xsi:type=\"u:Color\">#009646</Value>"),
    ];
    let result = view(
        "<Font ref=\"s:Important\" italic=\"true\"/><TextColor>s:Positive</TextColor><BackColor>#ff0000</BackColor>",
        &resources,
    );
    assert_eq!(result.diagnostics.len(), 0);
    let element = &result.elements[1];
    assert!((element.font.points - 12.0).abs() < f64::EPSILON);
    assert_eq!(element.font.weight, 700);
    assert!(element.font.italic);
    assert_eq!(
        element
            .font
            .origins
            .iter()
            .map(|origin| origin.source.as_str())
            .collect::<Vec<_>>(),
        ["Important", "form"]
    );
    assert_eq!(element.foreground, ResolvedColor::Rgb("#009646".into()));
    assert_eq!(element.background, ResolvedColor::Rgb("#ff0000".into()));
}
#[test]
fn style_cycles_and_missing_values_are_explicit_and_do_not_recurse_unbounded() {
    let resources = [
        resource("A", "<Value xsi:type=\"u:Font\" ref=\"s:B\"/>"),
        resource("B", "<Value xsi:type=\"u:Font\" ref=\"s:A\"/>"),
    ];
    let result = view(
        "<Font ref=\"s:A\"/><TextColor>s:Missing</TextColor>",
        &resources,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|value| value.code == "style_cycle")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|value| value.code == "style_missing")
    );
}
#[test]
fn rebound_prefix_is_resolved_at_its_original_scope() {
    let result = view(
        "<TextColor xmlns:s=\"http://v8.1c.ru/8.1/data/ui/colors/web\">s:Red</TextColor><Font ref=\"s:NormalTextFont\" height=\"14\" bold=\"false\"/>",
        &[],
    );
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Web("Red".into())
    );
    assert!((result.elements[1].font.points - 14.0).abs() < f64::EPSILON);
    assert_eq!(result.elements[1].font.weight, 400);
}

fn descriptor(kind: &str, name: &str, properties: &str) -> FormResource {
    let xml = format!(
        "<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\"><{kind} uuid=\"11111111-1111-1111-1111-111111111111\"><Properties><Name>{name}</Name>{properties}</Properties></{kind}></MetaDataObject>"
    );
    FormResource {
        picture: None,
        retained_bytes: 0,
        source: name.into(),
        descriptor: metadata_parser::parse(&xml, None, PropertiesMode::All).unwrap(),
        references: Vec::new(),
    }
}

#[test]
fn default_style_selects_named_style_instead_of_last_loaded_resource() {
    let mut resources = vec![
        descriptor(
            "Configuration",
            "Config",
            "<DefaultStyle>Style.Selected</DefaultStyle>",
        ),
        descriptor(
            "Style",
            "Selected",
            "<FormTextColor>#112233</FormTextColor><FieldTextColor>#223344</FieldTextColor><NormalTextFont height=\"13\"/>",
        ),
        descriptor(
            "Style",
            "Unrelated",
            "<FormTextColor>#445566</FormTextColor><FieldTextColor>#556677</FieldTextColor><NormalTextFont height=\"21\"/>",
        ),
    ];
    let result = view("", &resources);
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Rgb("#223344".into())
    );
    assert_eq!(
        result.elements[1].title_color,
        ResolvedColor::Rgb("#112233".into())
    );
    assert!((result.elements[1].font.points - 13.0).abs() < f64::EPSILON);
    resources.push(descriptor(
        "Configuration",
        "Extension",
        "<DefaultStyle>Style.Unrelated</DefaultStyle>",
    ));
    let result = view("", &resources);
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Rgb("#556677".into())
    );
    assert_eq!(
        result.elements[1].title_color,
        ResolvedColor::Rgb("#445566".into())
    );
    assert!((result.elements[1].font.points - 21.0).abs() < f64::EPSILON);
}
