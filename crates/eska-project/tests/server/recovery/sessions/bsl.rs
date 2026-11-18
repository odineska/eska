use super::*;

const BSL_PATH: &str = "Ext/ManagedApplicationModule.bsl";
const DOCUMENT_BSL_PATH: &str = "Documents/Order/Ext/ObjectModule.bsl";
const BSL: &str = "// dirty owner 😀\r\nx = \"SELECT 1\r\n|AS Value\";\r\ny = 42;\r\n";

/// API 1.19 applies only the exact captured native revision and never asks the adapter to save a BSL module.
#[test]
fn bsl_session_protected_snapshot_conflicts_and_explicit_unsaved_receipt() {
    for locale in ["en-US", "ru-RU"] {
        let (_directory, root, private) = source_fixture();
        let disk = root.join("src").join(BSL_PATH);
        fs::write(&disk, BSL.replace("dirty owner", "disk owner")).unwrap();
        let original = fs::read(&disk).unwrap();
        let mut client = Client::new(&root, locale);
        let binding = source_connect_version(&mut client, &root, &private, locale, 19);
        let state = open_bsl(&mut client, &binding, BSL);
        assert_eq!(state["protection"]["ownerLock"], Value::Null);
        assert_eq!(
            state["protection"]["ownerProtection"],
            "protectedViewWithConflicts"
        );
        assert_eq!(state["draftWritable"], true);
        let state = reject_native(
            &mut client,
            &binding,
            &state,
            &native(7, &BSL.replace("42", "43"), true, false),
        );
        let state = source_draft(&mut client, &binding, &state, "SELECT 2\nAS Value");
        let state = reject_native(&mut client, &binding, &state, &native(8, BSL, true, false));
        assert_eq!(
            source_parts(&mut client, &binding, &state)[3],
            "SELECT 2\nAS Value"
        );
        let unsupported = client.response(
            "query/sessionPrepare",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],"revision":state["revision"],
                    "owner":native(7,BSL,true,false),"comments":[],"saveCleanOwner":true}),
            ),
        );
        assert_eq!(unsupported["error"]["code"], -32602);
        let prepared = client.request("query/sessionPrepare", &source_args(&binding, json!({"querySessionId":state["querySessionId"],
            "revision":state["revision"],"owner":native(7,BSL,true,false),"comments":[],"lineEnding":"crlf"})));
        assert_eq!(prepared["plan"]["saveOwner"], false);
        assert!(prepared["record"]["prepared"].is_object());
        let output = applied_text(BSL, &prepared["plan"]);
        assert!(output.starts_with("// dirty owner 😀\r\nx = "));
        assert!(output.ends_with(";\r\ny = 42;\r\n"));
        let applied = client.request(
            "query/sessionApplied",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],
            "applyToken":prepared["plan"]["applyToken"],"owner":native(8,&output,true,false)}),
            ),
        );
        assert_eq!(source_parts(&mut client, &binding, &applied)[1], output);
        assert_eq!(fs::read(&disk).unwrap(), original);
        let closed = client.request(
            "query/sessionClose",
            &source_args(&binding, json!({"querySessionId":state["querySessionId"]})),
        );
        assert_eq!(
            closed["releasedSourceRestriction"],
            state["protection"]["sourceRestriction"]
        );
        assert_eq!(closed["releasedOwnerLock"], Value::Null);
        client.finish();
    }
}

/// A conflicting version or hash pauses the session without consuming its draft or granting a new owner binding.
fn reject_native(client: &mut Client, binding: &Value, state: &Value, owner: &Value) -> Value {
    let changed = client.response(
        "query/sessionPrepare",
        &source_args(
            binding,
            json!({"querySessionId":state["querySessionId"],
        "revision":state["revision"],"owner":owner,"comments":[]}),
        ),
    );
    assert_eq!(
        changed["error"]["data"]["details"]["reason"],
        "stale_version"
    );
    client.request(
        "query/sessionResume",
        &source_args(
            binding,
            json!({"querySessionId":state["querySessionId"],
        "revision":state["revision"],"owner":native(7,BSL,true,false)}),
        ),
    )
}

/// A declared BSL module and SDBL literal are selected through the public protocol.
fn open_bsl(client: &mut Client, binding: &Value, text: &str) -> Value {
    let root = client.request("metadata/root", &source_args(binding, json!({})));
    let node = json!({"kind":"module","owner":root["node"]["id"]["objectId"],"role":"managed-application"});
    open_bsl_module(client, binding, &node, BSL_PATH, text)
}

fn open_bsl_module(
    client: &mut Client,
    binding: &Value,
    node: &Value,
    path: &str,
    text: &str,
) -> Value {
    let document = client.request(
        "query/open",
        &source_args(
            binding,
            json!({"node":node,"source":{"value":path,"encoding":"utf-8"},"version":"7","text":text}),
        ),
    );
    let query = client.request(
        "query/read",
        &source_args(
            binding,
            json!({"documentId":document["documentId"],"version":document["version"],
                "snapshot":document["snapshot"],"contextFingerprint":document["contextFingerprint"],
                "at":{"line":1,"character":7}}),
        ),
    );
    client.request(
        "query/sessionOpen",
        &source_args(
            binding,
            json!({"documentId":document["documentId"],"id":ID,
                "query":query["query"]["identity"],"format":"editor","userReadOnly":false}),
        ),
    )
}

#[test]
fn bsl_native_save_keeps_the_session_after_each_receipt_and_adopts_an_exact_owner_save() {
    for (locale, initially_dirty) in [("en-US", false), ("ru-RU", true)] {
        let (_directory, root, private) = source_fixture();
        let source = root.join("src");
        order_descriptor(&source, "");
        let disk = source.join(DOCUMENT_BSL_PATH);
        fs::create_dir_all(disk.parent().unwrap()).unwrap();
        let original = if initially_dirty {
            BSL.replace("dirty owner", "disk owner")
        } else {
            BSL.to_owned()
        };
        fs::write(&disk, format!("\u{feff}{original}")).unwrap();
        let mut client = Client::new(&root, locale);
        let mut binding = source_connect_version(&mut client, &root, &private, locale, 55);
        client.request(
            "metadata/children",
            &source_args(
                &binding,
                json!({"node":{"kind":"object","objectId":"document:Order"}}),
            ),
        );
        let module = json!({"kind":"module","owner":"document:Order","role":"object"});
        let mut state = open_bsl_module(&mut client, &binding, &module, DOCUMENT_BSL_PATH, BSL);
        let original_protection = state["protection"].clone();
        let mut owner = BSL.to_owned();
        for (version, text, is_dirty) in [
            (7, "SELECT 2\nAS Value", initially_dirty),
            (8, "SELECT 3\nAS Value", false),
        ] {
            state = source_draft(&mut client, &binding, &state, text);
            let request = source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],"revision":state["revision"],
                    "owner":native(version,&owner,is_dirty,false),"comments":[],
                    "saveCleanOwner":true,"lineEnding":"crlf"}),
            );
            let prepared = client.request("query/sessionPrepare", &request);
            assert_eq!(prepared["plan"]["saveOwner"], !is_dirty);
            assert_eq!(prepared["record"]["prepared"]["saveOwner"], !is_dirty);
            let output = applied_text(&owner, &prepared["plan"]);
            assert!(output.contains("\r\n|AS Value"));
            assert!(output.starts_with("// dirty owner 😀\r\n"));
            assert!(output.ends_with(";\r\ny = 42;\r\n"));
            let receipt = source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],"applyToken":prepared["plan"]["applyToken"],
                    "owner":native(version+1,&output,is_dirty,false)}),
            );
            if !is_dirty {
                let error = client.response("query/sessionApplied", &receipt);
                assert!(
                    error.get("error").is_some(),
                    "receipt requires the actual disk write"
                );
                fs::write(&disk, format!("\u{feff}{output}")).unwrap();
            }
            if version == 8 {
                changed_bsl(&mut client, &mut binding, "2");
            }
            let updated = client.request("query/sessionApplied", &receipt);
            assert_eq!(updated["querySessionId"], state["querySessionId"]);
            assert_eq!(updated["protection"], original_protection);
            assert_eq!(updated["resumed"], true);
            assert_eq!(updated["applyPending"], false);
            assert_eq!(source_parts(&mut client, &binding, &updated)[2], text);
            state = updated;
            owner = output;
            if is_dirty {
                assert_eq!(
                    fs::read_to_string(&disk).unwrap(),
                    format!("\u{feff}{original}")
                );
                // An independent native Save can make precisely the acknowledged buffer clean.
                fs::write(&disk, format!("\u{feff}{owner}")).unwrap();
            }
            if version == 7 {
                changed_bsl(&mut client, &mut binding, "1");
            }
        }
        assert_eq!(
            fs::read_to_string(&disk).unwrap(),
            format!("\u{feff}{owner}")
        );

        // Foreign bytes cannot be adopted merely because the client requests saving the owner.
        fs::write(&disk, format!("\u{feff}{owner}// external\r\n")).unwrap();
        let changed = source_draft(&mut client, &binding, &state, "SELECT later");
        let error = client.response(
            "query/sessionPrepare",
            &source_args(
                &binding,
                json!({"querySessionId":changed["querySessionId"],"revision":changed["revision"],
                    "owner":native(9,&owner,false,false),"comments":[],"saveCleanOwner":true}),
            ),
        );
        assert!(error.get("error").is_some());
        client.finish();
    }
}

fn changed_bsl(client: &mut Client, binding: &mut Value, sequence: &str) {
    client.send(
        &json!({"jsonrpc":"2.0","method":"workspace/didChangeFiles","params":{
            "sessionId":binding["sessionId"],"projectId":binding["projectId"],"sequence":sequence,
            "paths":[{"value":DOCUMENT_BSL_PATH,"encoding":"utf-8"}]
        }}),
    );
    let info = client.request("project/info", &json!({"sessionId":binding["sessionId"]}));
    let generation = &info["projects"][0]["generation"];
    assert_ne!(*generation, binding["generation"]);
    binding["generation"] = generation.clone();
}

#[test]
fn bsl_native_save_reconciles_a_saved_owner_after_backend_restart() {
    let (_directory, root, private) = source_fixture();
    let disk = root.join("src").join(BSL_PATH);
    fs::write(&disk, format!("\u{feff}{BSL}")).unwrap();
    let mut first = Client::new(&root, "en-US");
    let binding = source_connect_version(&mut first, &root, &private, "en-US", 55);
    let opened = open_bsl(&mut first, &binding, BSL);
    let state = source_draft(&mut first, &binding, &opened, "SELECT recovered");
    let prepared = first.request(
        "query/sessionPrepare",
        &source_args(
            &binding,
            json!({"querySessionId":state["querySessionId"],"revision":state["revision"],
                "owner":native(7,BSL,false,false),"comments":[],"saveCleanOwner":true}),
        ),
    );
    assert_eq!(prepared["plan"]["saveOwner"], true);
    let output = applied_text(BSL, &prepared["plan"]);
    fs::write(&disk, &output).unwrap();
    first.child.kill().unwrap();
    first.child.wait().unwrap();

    let mut second = Client::new(&root, "en-US");
    let binding = source_connect_version(&mut second, &root, &private, "en-US", 55);
    let restored = second.request(
        "query/sessionRestore",
        &source_args(
            &binding,
            json!({"id":ID,"revision":binding["records"][0]["revision"],"format":"editor"}),
        ),
    );
    assert_eq!(restored["resumed"], false);
    assert_eq!(restored["applyPending"], true);
    let resume = source_args(
        &binding,
        json!({"querySessionId":restored["querySessionId"],"revision":restored["revision"],
            "owner":native(1,&output,false,false)}),
    );
    assert_eq!(
        second.response("query/sessionResume", &resume)["error"]["data"]["details"]["reason"],
        "conflict"
    );
    fs::write(&disk, format!("\u{feff}{output}")).unwrap();
    let state = second.request("query/sessionResume", &resume);
    assert_eq!(state["resumed"], true);
    assert_eq!(state["applyPending"], false);
    let texts = source_parts(&mut second, &binding, &state);
    assert_eq!(texts[1], output);
    assert_eq!(texts[2], "SELECT recovered");
    assert_eq!(
        fs::read_to_string(&disk).unwrap(),
        format!("\u{feff}{output}")
    );
    second.finish();
}
