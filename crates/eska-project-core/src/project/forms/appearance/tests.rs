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
        style: None,
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
        style: None,
        picture: None,
        retained_bytes: 0,
        source: name.into(),
        descriptor: metadata_parser::parse(&xml, None, PropertiesMode::All).unwrap(),
        references: Vec::new(),
    }
}

fn styled_resource(name: &str, source: &str, body: &str) -> FormResource {
    let mut result = descriptor("Style", name, "");
    let document = roxmltree::Document::parse(body).unwrap();
    result.style = Some(super::super::style_data::parse(source, &document).unwrap());
    result
}
const NATIVE_STYLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/forms/native-style-values/Styles/ProbeStyleValues/Ext/Style.xml"
));

#[test]
fn selected_native_style_overrides_named_items_and_retains_font_provenance() {
    let mut resources = vec![
        descriptor(
            "Configuration",
            "Config",
            "<DefaultStyle>Style.ProbeStyleValues</DefaultStyle>",
        ),
        resource("ProbeColor", "<Value xsi:type=\"u:Color\">#ff8c00</Value>"),
        resource(
            "ProbeFont",
            "<Value xsi:type=\"u:Font\" height=\"12\" bold=\"true\"/>",
        ),
        styled_resource("ProbeStyleValues", "selected-style-data", NATIVE_STYLE),
        styled_resource(
            "Other",
            "other-style-data",
            &NATIVE_STYLE
                .replace("#9800FF", "#224466")
                .replace("height=\"18\"", "height=\"21\""),
        ),
    ];
    let body = "<Font ref=\"s:ProbeFont\" italic=\"true\"/><TitleFont ref=\"s:ProbeFont\" height=\"10\"/><TextColor>s:ProbeColor</TextColor>";
    let result = view(body, &resources);
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Rgb("#9800ff".into())
    );
    let font = &result.elements[1].font;
    assert!((font.points - 18.0).abs() < f64::EPSILON);
    assert!(font.italic);
    assert_eq!(font.weight, 700);
    assert_eq!(
        font.origins
            .iter()
            .map(|value| value.source.as_str())
            .collect::<Vec<_>>(),
        ["selected-style-data", "form"]
    );
    assert!((result.elements[1].title_font.points - 10.0).abs() < f64::EPSILON);
    resources.push(descriptor(
        "Configuration",
        "Extension",
        "<DefaultStyle>Style.Other</DefaultStyle>",
    ));
    let result = view(body, &resources);
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Rgb("#224466".into())
    );
    assert!((result.elements[1].font.points - 21.0).abs() < f64::EPSILON);
}

#[test]
fn borrowed_style_payload_falls_back_per_named_item_and_cycles_are_bounded() {
    let mut resources = vec![
        descriptor(
            "Configuration",
            "Config",
            "<DefaultStyle>Style.ProbeStyleValues</DefaultStyle>",
        ),
        resource("ProbeColor", "<Value xsi:type=\"u:Color\">#ff8c00</Value>"),
        resource(
            "ProbeFont",
            "<Value xsi:type=\"u:Font\" height=\"12\" bold=\"true\"/>",
        ),
        styled_resource("ProbeStyleValues", "base-style-data", NATIVE_STYLE),
        descriptor("Style", "ProbeStyleValues", ""),
    ];
    assert_eq!(
        view("<TextColor>s:ProbeColor</TextColor>", &resources).elements[1].foreground,
        ResolvedColor::Rgb("#9800ff".into())
    );
    let partial = "<Style xmlns=\"http://v8.1c.ru/8.3/xcf/extrnprops\"><Item name=\"StyleItem.ProbeColor\"><Color>#008000</Color></Item></Style>";
    resources.push(styled_resource(
        "ProbeStyleValues",
        "extension-style-data",
        partial,
    ));
    let result = view(
        "<TextColor>s:ProbeColor</TextColor><Font ref=\"s:ProbeFont\"/>",
        &resources,
    );
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Rgb("#008000".into())
    );
    assert!((result.elements[1].font.points - 18.0).abs() < f64::EPSILON);
    let cycle = partial.replace("#008000", "s:ProbeColor").replace(
        "<Style xmlns=",
        "<Style xmlns:s=\"http://v8.1c.ru/8.1/data/ui/style\" xmlns=",
    );
    resources.push(styled_resource(
        "ProbeStyleValues",
        "cycle-style-data",
        &cycle,
    ));
    let result = view("<TextColor>s:ProbeColor</TextColor>", &resources);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|value| value.code == "style_cycle" && value.origin.source == "cycle-style-data")
    );
}

#[test]
fn native_taxi_style_keeps_automatic_role_defaults_and_requires_item_definitions() {
    let resources = [
        descriptor(
            "Configuration",
            "Config",
            "<DefaultStyle>Style.ProbeStyleValues</DefaultStyle>",
        ),
        styled_resource("ProbeStyleValues", "selected-style-data", NATIVE_STYLE),
    ];
    let result = view("", &resources);
    assert_eq!(
        result.elements[1].foreground,
        ResolvedColor::Role("FieldTextColor".into())
    );
    assert_eq!(
        result.elements[1].title_color,
        ResolvedColor::Role("FormTextColor".into())
    );
    assert!((result.elements[1].font.points - 10.0).abs() < f64::EPSILON);
    let result = view("<TextColor>s:ProbeColor</TextColor>", &resources);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|value| value.code == "style_missing")
    );
}
