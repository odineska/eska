use super::*;
use eska_project_core::formatting::protocol::source_sha256;

/// A plain project process needs no workspace, analyzer, file or writable owner to locate strings.
#[test]
fn multiline_geometry_crosses_stdio_and_notifications_cannot_allocate_buffers() {
    let fixture = support::TestDir::new();
    let text = "\u{feff}F(\"😀 первый\r\n\t|второй\");";
    let mut expected = None;
    for locale in ["ru", "en"] {
        let mut client = Client::new(&fixture.0, locale);
        let hello = client.request("initialize", &json!({"apiVersion":{"major":1,"minor":34},"client":{"name":"language-test","version":"1"}}));
        assert_eq!(
            hello["capabilities"]["bslMultilineStrings"]["schemaVersion"],
            1
        );
        // These reservations would exhaust the shared 32 MiB budget if a notification were admitted.
        for _ in 0..10 {
            client.send(&json!({"jsonrpc":"2.0","method":"language/bufferBegin","params":{"bytes":4*1024*1024,"snapshot":source_sha256("")}}));
        }
        let upload = client.request(
            "language/bufferBegin",
            &json!({"bytes":text.len(),"snapshot":source_sha256(text)}),
        );
        let id = &upload["bufferId"];
        client.request(
            "language/bufferAppend",
            &json!({"bufferId":id,"offset":0,"text":text}),
        );
        let result = client.request(
            "language/multilineStrings",
            &json!({"version":"8","bufferId":id}),
        );
        assert_eq!(result["sourceSha256"], source_sha256(text));
        assert_eq!(
            result["strings"][0]["range"]["start"],
            text.find('"').unwrap()
        );
        assert_eq!(
            result["strings"][0]["continuations"],
            json!([text.find('|').unwrap()])
        );
        if let Some(expected) = &expected {
            assert_eq!(expected, &result);
        } else {
            expected = Some(result);
        }
        assert_eq!(
            client.request("language/bufferClose", &json!({"bufferId":id}))["released"],
            true
        );
        client.finish();
    }
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}
