use super::*;
use crate::ide::server::Identity;

fn server(minor: u32, locale: &str) -> (Server, Value) {
    let mut server = Server::new(Identity::Project).unwrap();
    let hello = call(&mut server, "initialize", &json!({"apiVersion":{"major":1,"minor":minor},"client":{"name":"syntax-test","version":"1"},"locale":locale})).unwrap();
    (server, hello)
}

fn call(server: &mut Server, method: &str, args: &Value) -> Result<Value, Value> {
    server.execute(
        method,
        args,
        &mut Vec::new(),
        &AtomicBool::new(false),
        &AtomicBool::new(false),
    )
}

#[test]
fn language_geometry_is_additive_read_only_and_locale_independent() {
    let (mut old, hello) = server(33, "en-US");
    assert!(hello["capabilities"].get("bslMultilineStrings").is_none());
    let input = json!({"version":"7","text":"\u{feff}f(\"😀 один\r\n\t|два\");"});
    assert_eq!(
        call(&mut old, "language/multilineStrings", &input).unwrap_err()["code"],
        -32601
    );
    let mut outputs = Vec::new();
    for locale in ["ru-RU", "en-US"] {
        let (mut server, hello) = server(34, locale);
        assert_eq!(
            hello["capabilities"]["bslMultilineStrings"]["writesOwner"],
            false
        );
        let result = call(&mut server, "language/multilineStrings", &input).unwrap();
        let text = input["text"].as_str().unwrap();
        assert_eq!(result["version"], "7");
        assert_eq!(result["sourceSha256"], source_sha256(text));
        assert_eq!(
            result["strings"][0]["range"]["start"],
            text.find('"').unwrap()
        );
        assert_eq!(
            result["strings"][0]["continuations"],
            json!([text.find('|').unwrap()])
        );
        outputs.push(result);
    }
    assert_eq!(outputs[0], outputs[1]);
}

fn upload(server: &mut Server, namespace: &str, text: &str) -> String {
    let result = call(
        server,
        &format!("{namespace}/bufferBegin"),
        &json!({"bytes":text.len(),"snapshot":source_sha256(text)}),
    )
    .unwrap();
    let id = result["bufferId"].as_str().unwrap().to_owned();
    let first = text.len().min(INLINE_BYTES);
    for (offset, part) in [(0, &text[..first]), (first, &text[first..])] {
        if !part.is_empty() {
            call(
                server,
                &format!("{namespace}/bufferAppend"),
                &json!({"bufferId":id,"offset":offset,"text":part}),
            )
            .unwrap();
        }
    }
    id
}

#[test]
fn large_geometry_uses_shared_upload_limits_without_accepting_foreign_handles() {
    let (mut server, _) = server(34, "en-US");
    let text = format!("//{}\nf(\"one\n\t|two\");", "x".repeat(INLINE_BYTES));
    assert!(
        call(
            &mut server,
            "language/multilineStrings",
            &json!({"version":"1","text":text})
        )
        .is_err()
    );
    let id = upload(&mut server, "language", &text);
    let result = call(
        &mut server,
        "language/multilineStrings",
        &json!({"version":"1","bufferId":id}),
    )
    .unwrap();
    assert_eq!(
        result["strings"][0]["continuations"],
        json!([text.find('|').unwrap()])
    );
    let foreign = upload(&mut server, "format", &text);
    assert!(
        call(
            &mut server,
            "language/multilineStrings",
            &json!({"version":"1","bufferId":foreign})
        )
        .is_err()
    );
    assert_eq!(
        call(&mut server, "language/bufferClose", &json!({"bufferId":id})).unwrap()["released"],
        true
    );
    assert!(
        call(
            &mut server,
            "language/multilineStrings",
            &json!({"version":"1","bufferId":id})
        )
        .is_err()
    );
    assert!(
        call(
            &mut server,
            "language/bufferBegin",
            &json!({"bytes":MAX_TEXT_BYTES+1,"snapshot":"a".repeat(64)})
        )
        .is_err()
    );
}

#[test]
fn malformed_snapshots_and_cancellation_never_return_guessed_geometry() {
    let (mut server, _) = server(34, "ru-RU");
    let invalid = call(
        &mut server,
        "language/multilineStrings",
        &json!({"version":"1","text":"f(\"unfinished"}),
    )
    .unwrap_err();
    assert_eq!(
        invalid["data"],
        json!({"kind":"language_syntax_failed","details":{"reason":"invalid_literal"}})
    );
    for input in [
        json!({"version":"0","text":""}),
        json!({"version":"1","text":null}),
        json!({"version":"1","text":"","bufferId":"other"}),
        json!({"version":"1","text":"","source":"not-a-path-api"}),
    ] {
        assert!(call(&mut server, "language/multilineStrings", &input).is_err());
    }
    let result = server.execute(
        "language/multilineStrings",
        &json!({"version":"1","text":"f(\"one\n|two\");"}),
        &mut Vec::new(),
        &AtomicBool::new(true),
        &AtomicBool::new(false),
    );
    assert_eq!(result.unwrap_err()["code"], -32800);
}
