use super::*;
use crate::project::forms::{ElementKey, compose, parse};

#[test]
fn handler_declarations_ignore_literal_comment_and_directive_decoys() {
    let input = "\u{feff}// Процедура Обработка() КонецПроцедуры\r\n&Before(\"Обработка\")\r\n#Region Обработка\r\nПроцедура ОБРАБОТКА(Знач Параметр = \"Function Fake()\") Экспорт\r\nТекст = \"Процедура Обработка()\n|КонецПроцедуры\"; // 😀\r\nКонецПроцедуры\r\n#EndRegion";
    let range = handler_range(input, "обработка").unwrap().unwrap();
    assert_eq!(&input[range], "ОБРАБОТКА");
    assert_eq!(handler_range(input, "Fake").unwrap(), None);
    let english = "Function _Handler(Arg) Export\nReturn '20260101';\nEndFunction";
    assert_eq!(
        &english[handler_range(english, "_handler").unwrap().unwrap()],
        "_Handler"
    );
}

#[test]
fn unavailable_and_ambiguous_declarations_do_not_guess_a_target() {
    for input in [
        "Procedure A()",
        "Procedure A(\nEndProcedure",
        "Function A() EndProcedure",
        "Procedure A() Procedure B() EndProcedure EndProcedure",
        "Text = \"incomplete",
    ] {
        assert_eq!(
            handler_range(input, "A"),
            Err(HandlerLookupError::InvalidModule),
            "{input}"
        );
    }
    let duplicate =
        "#If Client Then\nProcedure A() EndProcedure\n#Else\nProcedure a() EndProcedure\n#EndIf";
    assert_eq!(
        handler_range(duplicate, "A"),
        Err(HandlerLookupError::Ambiguous)
    );
    assert_eq!(
        handler_range("Procedure A() EndProcedure", "Module.A").unwrap(),
        None
    );
    assert_eq!(
        handler_range(&" ".repeat(query::MAX_SNAPSHOT_BYTES + 1), "A"),
        Err(HandlerLookupError::TooLarge)
    );
}

#[test]
fn effective_events_and_actions_keep_extension_provenance_and_repeats() {
    let body = "<Events><Event name=\"OnOpen\">OpenForm</Event><Event xmlns=\"urn:foreign\" name=\"OnOpen\">Foreign</Event></Events><ChildItems><InputField name=\"Name\" id=\"1\"><Events><Event name=\"OnChange\">Changed</Event><Event name=\"OnChange\" callType=\"After\">AfterChanged</Event><Event name=\"Other\">Module.External</Event></Events></InputField></ChildItems><Commands><Command name=\"Action\" id=\"2\"><Action>Execute</Action></Command></Commands>";
    let base = parse(
        "base",
        format!("<Form xmlns=\"{FORM_NAMESPACE}\">{body}</Form>"),
    )
    .unwrap();
    let changed = body.replace("Changed</Event>", "ExtensionChanged</Event>");
    let extension = parse(
        "extension",
        format!("<Form xmlns=\"{FORM_NAMESPACE}\">{changed}<BaseForm>{body}</BaseForm></Form>"),
    )
    .unwrap();
    let form = compose(&base, &[&extension]);
    let values = handlers(&form);
    assert_eq!(
        values
            .iter()
            .map(|value| value.name.as_str())
            .collect::<Vec<_>>(),
        [
            "OpenForm",
            "ExtensionChanged",
            "AfterExtensionChanged",
            "Execute"
        ]
    );
    assert_eq!(values[0].origin.source, "base");
    assert_eq!(values[1].origin.source, "extension");
    assert!(
        matches!(&values[1].owner,FormHandlerOwner::Element(identity) if identity.key==ElementKey::Xml(1) && identity.source=="base")
    );
    assert!(matches!(&values[3].owner,FormHandlerOwner::Command(name) if name=="Action"));
}
