use super::*;

#[path = "sessions/prepared.rs"]
mod prepared;

#[path = "sessions/protection.rs"]
mod protection;

#[path = "sessions/source.rs"]
mod source;

#[path = "sessions/bsl.rs"]
mod bsl;

#[path = "sessions/projections.rs"]
mod projections;

const SOURCE: &str = "CommonForms/Queries/Ext/Form.xml";
const XML: &str = "\u{feff}<Form xmlns='http://v8.1c.ru/8.3/xcf/logform' xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'>\r\n<Attributes><Attribute name='List' id='1'><Settings xsi:type='DynamicList'><ManualQuery>true</ManualQuery><QueryText>SELECT 1</QueryText></Settings></Attribute></Attributes><other>keep</other>\r\n</Form>";

/// The standalone server sees an actual declared owner and current support policy, never an invented file URI.
fn source_fixture() -> (support::TestDir, std::path::PathBuf, std::path::PathBuf) {
    let (directory, root, private) = fixture();
    let source = root.join("src");
    let config = source.join("Configuration.xml");
    fs::write(
        &config,
        fs::read_to_string(&config).unwrap().replace(
            "<ChildObjects/>",
            "<ChildObjects><CommonForm>Queries</CommonForm></ChildObjects>",
        ),
    )
    .unwrap();
    fs::create_dir_all(source.join("CommonForms/Queries/Ext")).unwrap();
    fs::create_dir_all(source.join("Ext")).unwrap();
    fs::write(source.join("Ext/ParentConfigurations.bin"), "{6,0,0,0,0,0}").unwrap();
    fs::write(source.join("CommonForms/Queries.xml"),"<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses'><CommonForm uuid='22222222-2222-2222-2222-222222222222'><Properties><Name>Queries</Name></Properties></CommonForm></MetaDataObject>").unwrap();
    fs::write(source.join(SOURCE), XML).unwrap();
    (directory, root, private)
}

fn order_descriptor(source: &Path, child_objects: &str) {
    let configuration = source.join("Configuration.xml");
    fs::write(
        &configuration,
        fs::read_to_string(&configuration).unwrap().replace(
            "<CommonForm>Queries</CommonForm>",
            "<CommonForm>Queries</CommonForm><Document>Order</Document>",
        ),
    )
    .unwrap();
    fs::create_dir_all(source.join("Documents")).unwrap();
    fs::write(source.join("Documents/Order.xml"), format!("<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses'><Document uuid='33333333-3333-3333-3333-333333333333'><Properties><Name>Order</Name></Properties><ChildObjects>{child_objects}</ChildObjects></Document></MetaDataObject>")).unwrap();
}

const ORDER_FORM: &str = "Documents/Order/Forms/QueryForm/Ext/Form.xml";

fn order_form(source: &Path) -> std::path::PathBuf {
    order_descriptor(source, "<Form>QueryForm</Form>");
    let file = source.join(ORDER_FORM);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(source.join("Documents/Order/Forms/QueryForm.xml"), "<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses'><Form uuid='44444444-4444-4444-4444-444444444444'><Properties><Name>QueryForm</Name></Properties></Form></MetaDataObject>").unwrap();
    fs::write(&file, XML).unwrap();
    file
}

/// Context tokens are copied from replies, independently of production parameter types.
fn source_args(binding: &Value, extra: Value) -> Value {
    let mut value = json!({"sessionId":binding["sessionId"],"projectId":binding["projectId"],
        "generation":binding["generation"],"storeId":binding["storeId"]});
    let Value::Object(extra) = extra else {
        panic!("fixture extras are an object");
    };
    value.as_object_mut().unwrap().extend(extra);
    value
}

/// Discover the opaque node from actual tree replies, with no knowledge of `ObjectId` encoding.
fn owner_node(client: &mut Client, binding: &Value, root: &Value) -> Value {
    let children = client.request(
        "metadata/children",
        &source_args(binding, json!({"node":root})),
    );
    let common = children["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"]["collection"]["kind"] == "common")
        .unwrap();
    let groups = client.request(
        "metadata/children",
        &source_args(binding, json!({"node":common["id"]})),
    );
    let group = groups["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"]["collection"]["metadataKind"] == "common-form")
        .unwrap();
    let owners = client.request(
        "metadata/children",
        &source_args(binding, json!({"node":group["id"]})),
    );
    assert_eq!(owners["nodes"].as_array().unwrap().len(), 1);
    owners["nodes"][0]["id"].clone()
}

/// The session contract is opt-in and shares the existing bounded recovery store.
fn source_connect(client: &mut Client, root: &Path, private: &Path, locale: &str) -> Value {
    source_connect_version(client, root, private, locale, 15)
}

/// Version negotiation preserves memory-only 1.15 preparation while 1.16 opts into the durable journal.
fn source_connect_version(
    client: &mut Client,
    root: &Path,
    private: &Path,
    locale: &str,
    minor: u32,
) -> Value {
    source_connect_options(client, root, private, locale, minor, false)
}

/// Competing-writer tests opt into actual metadata writes; ordinary source-session tests retain the read-only handshake.
fn source_connect_options(
    client: &mut Client,
    root: &Path,
    private: &Path,
    locale: &str,
    minor: u32,
    allow_property_edits: bool,
) -> Value {
    let hello = client.request("initialize",&json!({"apiVersion":{"major":1,"minor":minor},"client":{"name":"session-test","version":"1"},"locale":locale,
        "allowPropertyEdits":allow_property_edits}));
    assert_eq!(hello["capabilities"]["querySessions"]["schemaVersion"], 1);
    assert_eq!(hello["capabilities"]["querySessions"]["writesOwner"], false);
    assert_eq!(
        hello["capabilities"]["querySessions"]["bslEditing"],
        minor >= 19
    );
    assert_eq!(
        hello["capabilities"]["querySessions"]["preparedStateDurable"],
        minor >= 16
    );
    assert_eq!(
        hello["capabilities"]["querySessions"]
            .get("saveCleanOwner")
            .and_then(Value::as_bool),
        (minor >= 55).then_some(true)
    );
    let workspace = client.request("workspace/open",&json!({"start":{"value":root,"encoding":"utf-8"},"selection":{"kind":"current"},"diskCache":false}));
    let opened = client.request(
        "query/recoveryOpen",
        &json!({"directory":{"value":private,"encoding":"utf-8"}}),
    );
    let mut binding = json!({"sessionId":workspace["sessionId"],"projectId":workspace["projects"][0]["projectId"],
        "generation":workspace["projects"][0]["generation"],"storeId":opened["storeId"],"records":opened["records"]});
    binding["node"] = owner_node(client, &binding, &workspace["projects"][0]["root"]);
    binding
}

/// Opening publishes the complete original snapshot before any adapter displays an editable query.
fn open_source(client: &mut Client, binding: &Value, text: &str, read_only: bool) -> Value {
    open_xml(client, binding, SOURCE, text, read_only)
}

fn open_xml(
    client: &mut Client,
    binding: &Value,
    path: &str,
    text: &str,
    read_only: bool,
) -> Value {
    let document = client.request("query/open",&source_args(binding,json!({"node":binding["node"],"source":{"value":path,"encoding":"utf-8"},"version":"7","text":text})));
    client.request("query/sessionOpen",&source_args(binding,json!({"documentId":document["documentId"],"id":ID,
        "query":{"kind":"dynamicList","name":"List","id":"1"},"format":"editor","userReadOnly":read_only})))
}

/// Source-session records can be read through the same independent chunk reader as generic recovery.
fn source_parts(client: &mut Client, binding: &Value, state: &Value) -> [String; 4] {
    PARTS.map(|part| read_part(client, binding, &state["record"], part))
}

/// Only the draft changes in a background checkpoint; owner/base updates belong to the session receipt.
fn source_draft(client: &mut Client, binding: &Value, state: &Value, draft: &str) -> Value {
    let mut values = source_parts(client, binding, state);
    values[3] = draft.into();
    save(
        client,
        binding,
        &values,
        state["record"]["revision"].as_str(),
    );
    client.request(
        "query/sessionGet",
        &source_args(binding, json!({"querySessionId":state["querySessionId"]})),
    )
}

/// Editor facts carry exact UTF-8 hashes and process-local versions; no IDE API participates in these cases.
fn native(version: u64, text: &str, dirty: bool, read_only: bool) -> Value {
    json!({"version":version.to_string(),"snapshot":snapshot(text),"isDirty":dirty,"userReadOnly":read_only,"application":null})
}

/// Apply returned byte replacements solely to a synthetic native model, leaving the real source untouched.
fn applied_text(input: &str, plan: &Value) -> String {
    let mut output = input.to_owned();
    for replacement in plan["replacements"].as_array().unwrap().iter().rev() {
        let start = usize::try_from(replacement["range"]["start"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(replacement["range"]["end"].as_u64().unwrap()).unwrap();
        output.replace_range(start..end, replacement["text"].as_str().unwrap());
    }
    assert_eq!(snapshot(&output), plan["outputSnapshot"]);
    output
}

/// Real RPC owns dirty snapshots, draft CAS, a single apply token and the resulting checkpoint in both locales.
#[test]
fn standalone_query_session_checkpoints_and_acknowledges_dirty_owner_without_source_writes() {
    let mut expected = None;
    for locale in ["ru-RU", "en-US"] {
        let (_directory, root, private) = source_fixture();
        let mut client = Client::new(&root, "en");
        let binding = source_connect(&mut client, &root, &private, locale);
        let input = XML
            .strip_prefix('\u{feff}')
            .unwrap()
            .replace("<other>keep</other>", "<other>unsaved 😀</other>");
        let state = open_source(&mut client, &binding, &input, false);
        assert_eq!(state["resumed"], true);
        assert_eq!(source_parts(&mut client, &binding, &state)[1], input);
        let state = source_draft(
            &mut client,
            &binding,
            &state,
            "SELECT 2;\n// draft 😀\nSELECT 3",
        );
        assert_eq!(state["revision"], "2");
        let prepared=client.request("query/sessionPrepare",&source_args(&binding,json!({"querySessionId":state["querySessionId"],
            "revision":state["revision"],"owner":native(7,&input,true,false),"comments":[],"lineEnding":"crlf"})));
        assert_eq!(prepared["applyPending"], true);
        assert_eq!(prepared["plan"]["saveOwner"], false);
        assert_eq!(
            prepared["record"]["revision"], state["record"]["revision"],
            "API 1.15 preparation remains memory-only"
        );
        assert!(prepared["record"].get("prepared").is_none());
        let busy = client.response(
            "query/sessionPrepare",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],
            "revision":state["revision"],"owner":native(7,&input,true,false),"comments":[]}),
            ),
        );
        assert_eq!(busy["error"]["data"]["details"]["reason"], "apply_busy");
        let latest = source_draft(&mut client, &binding, &prepared, "SELECT newer");
        let output = applied_text(&input, &prepared["plan"]);
        let wrong = client.response(
            "query/sessionApplied",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],
            "applyToken":prepared["plan"]["applyToken"],"owner":native(8,&input,true,false)}),
            ),
        );
        assert_eq!(
            wrong["error"]["data"]["details"]["reason"],
            "invalid_receipt"
        );
        let acknowledged = client.request(
            "query/sessionApplied",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],
            "applyToken":prepared["plan"]["applyToken"],"owner":native(8,&output,true,false)}),
            ),
        );
        let values = source_parts(&mut client, &binding, &acknowledged);
        assert_eq!(values[1], output);
        assert_eq!(values[2], "SELECT 2;\n// draft 😀\nSELECT 3");
        assert_eq!(values[3], "SELECT newer");
        assert_eq!(acknowledged["record"]["metadata"]["ownerVersion"], "8");
        assert_eq!(
            fs::read_to_string(root.join("src").join(SOURCE)).unwrap(),
            XML
        );
        assert_ne!(
            latest["record"]["revision"],
            acknowledged["record"]["revision"]
        );
        let stable = json!({"plan":prepared["plan"],"base":values[2],"draft":values[3],"busy":busy["error"],"wrong":wrong["error"]});
        if let Some(expected) = &expected {
            assert_eq!(&stable, expected);
        } else {
            expected = Some(stable);
        }
        client.finish();
    }
}

/// Killing the server preserves both dirty texts; a new process starts paused and accepts only explicit rebinding.
#[test]
fn standalone_query_session_restores_with_explicit_resume_and_clean_independent_owner_save() {
    let (_directory, root, private) = source_fixture();
    let owner_file = order_form(&root.join("src"));
    let owner = json!({"kind":"object","objectId":"document:Order"});
    let form = json!({"kind":"object","objectId":"document:Order/form:QueryForm"});
    let input = XML
        .strip_prefix('\u{feff}')
        .unwrap()
        .replace("<other>keep</other>", "<other>dirty</other>");
    let mut first = Client::new(&root, "en");
    let mut binding = source_connect(&mut first, &root, &private, "en-US");
    first.request(
        "metadata/children",
        &source_args(&binding, json!({"node":owner})),
    );
    binding["node"] = form;
    let state = open_xml(&mut first, &binding, ORDER_FORM, &input, false);
    source_draft(&mut first, &binding, &state, "SELECT recovered");
    first.child.kill().unwrap();
    first.child.wait().unwrap();
    let mut client = Client::new(&root, "ru");
    let mut binding = source_connect(&mut client, &root, &private, "ru-RU");
    let recovered = client.request(
        "query/sessionRestore",
        &source_args(
            &binding,
            json!({"id":ID,
        "revision":binding["records"][0]["revision"],"format":"editor"}),
        ),
    );
    assert_eq!(recovered["resumed"], false);
    let request = source_args(
        &binding,
        json!({"querySessionId":recovered["querySessionId"],"revision":recovered["revision"],
        "owner":native(1,&input,true,false),"comments":[]}),
    );
    assert_eq!(
        client.response("query/sessionPrepare", &request)["error"]["data"]["details"]["reason"],
        "resume_required"
    );
    fs::write(&owner_file, format!("\u{feff}{input}")).unwrap();
    assert!(
        client
            .response("query/sessionResume", &request)
            .get("error")
            .is_some()
    );
    let resumed = client.request(
        "query/sessionResume",
        &source_args(
            &binding,
            json!({"querySessionId":recovered["querySessionId"],
        "revision":recovered["revision"],"owner":native(1,&input,false,false)}),
        ),
    );
    assert_eq!(resumed["resumed"], true);
    let mut state = resumed;
    let mut input = input;
    for (version, draft) in [(1, "SELECT recovered"), (2, "SELECT next")] {
        if version == 2 {
            let refreshed = client.request(
                "metadata/refresh",
                &source_args(&binding, json!({"node":owner})),
            );
            assert_ne!(refreshed["generation"], binding["generation"]);
            binding["generation"] = refreshed["generation"].clone();
            state = source_draft(&mut client, &binding, &state, draft);
        }
        let prepared = client.request(
            "query/sessionPrepare",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],"revision":state["revision"],
                    "owner":native(version,&input,false,false),"comments":[]}),
            ),
        );
        assert_eq!(prepared["plan"]["saveOwner"], true);
        let output = applied_text(&input, &prepared["plan"]);
        fs::write(&owner_file, format!("\u{feff}{output}")).unwrap();
        let acknowledged = client.request(
            "query/sessionApplied",
            &source_args(
                &binding,
                json!({"querySessionId":state["querySessionId"],"applyToken":prepared["plan"]["applyToken"],
                    "owner":native(version+1,&output,false,false)}),
            ),
        );
        assert_eq!(acknowledged["querySessionId"], state["querySessionId"]);
        assert_eq!(acknowledged["protection"], state["protection"]);
        assert_eq!(acknowledged["resumed"], true);
        assert_eq!(acknowledged["applyPending"], false);
        let parts = source_parts(&mut client, &binding, &acknowledged);
        assert_eq!(parts[1], output);
        assert_eq!(parts[2], draft);
        assert_eq!(parts[3], parts[2]);
        state = acknowledged;
        input = output;
    }
    client.finish();
}

/// Another window's acknowledged draft is checked before a source plan, rather than only after native application.
#[test]
fn standalone_query_session_detects_second_window_checkpoint_before_planning() {
    let (_directory, root, private) = source_fixture();
    let input = XML.strip_prefix('\u{feff}').unwrap();
    let mut first = Client::new(&root, "en");
    let binding = source_connect(&mut first, &root, &private, "en-US");
    let state = open_source(&mut first, &binding, input, false);
    let mut second = Client::new(&root, "en");
    let other = connect(&mut second, &root, &private, "en-US");
    let mut values = source_parts(&mut first, &binding, &state);
    values[3] = "SELECT second window".into();
    save(
        &mut second,
        &other,
        &values,
        state["record"]["revision"].as_str(),
    );
    let error = first.response(
        "query/sessionPrepare",
        &source_args(
            &binding,
            json!({"querySessionId":state["querySessionId"],
        "revision":state["revision"],"owner":native(7,input,false,false),"comments":[]}),
        ),
    );
    assert_eq!(
        error["error"]["data"]["details"]["reason"],
        "recovery_conflict"
    );
    assert_eq!(
        fs::read_to_string(root.join("src").join(SOURCE)).unwrap(),
        XML
    );
    first.finish();
    second.finish();
}

/// Full metadata/owner/base cannot be rewritten through a generic draft checkpoint while the core owns the session.
#[test]
fn standalone_query_session_rejects_forged_binding_and_preserves_user_restriction() {
    let (_directory, root, private) = source_fixture();
    let input = XML.strip_prefix('\u{feff}').unwrap();
    let mut client = Client::new(&root, "en");
    let binding = source_connect(&mut client, &root, &private, "en-US");
    let state = open_source(&mut client, &binding, input, true);
    assert_eq!(state["draftWritable"], false);
    let mut values = source_parts(&mut client, &binding, &state);
    let mut metadata: Value = serde_json::from_str(&values[0]).unwrap();
    metadata["ownerVersion"] = json!("8");
    values[0] = metadata.to_string();
    let upload = begin(
        &mut client,
        &binding,
        &values,
        state["record"]["revision"].as_str(),
    );
    append_parts(&mut client, &binding, &upload, &values);
    let rejected = client.response(
        "query/recoveryCommit",
        &args(&binding, json!({"updateId":upload["updateId"]})),
    );
    assert_eq!(
        rejected["error"]["data"]["details"]["reason"],
        "recovery_conflict"
    );
    let error = client.response(
        "query/sessionPrepare",
        &source_args(
            &binding,
            json!({"querySessionId":state["querySessionId"],
        "revision":state["revision"],"owner":native(7,input,false,true),"comments":[]}),
        ),
    );
    assert_eq!(
        error["error"]["data"]["details"]["reason"],
        "user_read_only"
    );
    let closed = client.request(
        "query/sessionClose",
        &source_args(&binding, json!({"querySessionId":state["querySessionId"]})),
    );
    assert_eq!(closed["releasedOwnerLock"], Value::Null);
    assert_eq!(closed["userReadOnly"], true);
    assert_eq!(
        fs::read_to_string(root.join("src").join(SOURCE)).unwrap(),
        XML
    );
    client.finish();
}

/// Pending native output participates in the existing connection budget and failed admission allocates no live plan.
#[test]
fn standalone_query_session_bounds_pending_output_and_closes_after_stale_metadata() {
    let (_directory, root, private) = source_fixture();
    let input = XML.strip_prefix('\u{feff}').unwrap();
    let mut client = Client::new(&root, "en");
    let binding = source_connect(&mut client, &root, &private, "en-US");
    let state = open_source(&mut client, &binding, input, false);
    let state = source_draft(&mut client, &binding, &state, "SELECT changed");
    let bytes = PARTS[1..]
        .iter()
        .map(|part| state["record"]["parts"][*part]["bytes"].as_u64().unwrap())
        .sum::<u64>();
    let mut buffers = Vec::new();
    for bytes in [8_388_608, 8_388_608, 8_388_608, 8_388_608 - bytes] {
        buffers.push(client.request(
            "query/bufferBegin",
            &source_args(&binding, json!({"bytes":bytes,"snapshot":"a".repeat(64)})),
        ));
    }
    let parameters = source_args(
        &binding,
        json!({"querySessionId":state["querySessionId"],"revision":state["revision"],
        "owner":native(7,input,false,false),"comments":[]}),
    );
    assert_eq!(
        client.response("query/sessionPrepare", &parameters)["error"]["data"]["kind"],
        "resource_limit"
    );
    let info = client.request(
        "query/sessionGet",
        &source_args(&binding, json!({"querySessionId":state["querySessionId"]})),
    );
    assert_eq!(info["applyPending"], false);
    for buffer in buffers {
        client.request(
            "query/bufferClose",
            &source_args(&binding, json!({"bufferId":buffer["bufferId"]})),
        );
    }
    let prepared = client.request("query/sessionPrepare", &parameters);
    client.request("query/sessionAbort",&source_args(&binding,json!({"querySessionId":state["querySessionId"],"applyToken":prepared["plan"]["applyToken"]})));
    client.request(
        "workspace/close",
        &json!({"sessionId":binding["sessionId"]}),
    );
    let recovered = client.request(
        "query/recoveryOpen",
        &args(
            &binding,
            json!({"directory":{"value":private,"encoding":"utf-8"}}),
        ),
    );
    assert_eq!(recovered["records"].as_array().unwrap().len(), 1);
    client.finish();
}

/// An older applied draft remains retained when a smaller new draft arrives before the native receipt.
#[test]
fn standalone_query_session_budget_keeps_the_draft_awaiting_application_receipt() {
    let (_directory, root, private) = source_fixture();
    let input = XML.strip_prefix('\u{feff}').unwrap();
    let mut client = Client::new(&root, "en");
    let binding = source_connect(&mut client, &root, &private, "en-US");
    let state = open_source(&mut client, &binding, input, false);
    let state = source_draft(&mut client, &binding, &state, "SELECT changed");
    let prepared = client.request(
        "query/sessionPrepare",
        &source_args(
            &binding,
            json!({"querySessionId":state["querySessionId"],
        "revision":state["revision"],"owner":native(7,input,false,false),"comments":[]}),
        ),
    );
    let output = applied_text(input, &prepared["plan"]);
    let state = prepared;
    let bytes = PARTS[1..]
        .iter()
        .map(|part| state["record"]["parts"][*part]["bytes"].as_u64().unwrap())
        .sum::<u64>()
        + u64::try_from(output.len()).unwrap();
    let mut buffers = Vec::new();
    for bytes in [8_388_608, 8_388_608, 8_388_608, 8_388_608 - bytes] {
        buffers.push(client.request(
            "query/bufferBegin",
            &source_args(&binding, json!({"bytes":bytes,"snapshot":"a".repeat(64)})),
        ));
    }
    let mut values = source_parts(&mut client, &binding, &state);
    values[3] = "new".into();
    let upload = begin(
        &mut client,
        &binding,
        &values,
        state["record"]["revision"].as_str(),
    );
    append_parts(&mut client, &binding, &upload, &values);
    let failed = client.response(
        "query/recoveryCommit",
        &args(&binding, json!({"updateId":upload["updateId"]})),
    );
    assert_eq!(failed["error"]["data"]["kind"], "resource_limit");
    assert_eq!(
        source_parts(&mut client, &binding, &state)[3],
        "SELECT changed"
    );
    client.request(
        "query/recoveryCancel",
        &args(&binding, json!({"updateId":upload["updateId"]})),
    );
    for buffer in buffers {
        client.request(
            "query/bufferClose",
            &source_args(&binding, json!({"bufferId":buffer["bufferId"]})),
        );
    }
    let updated = source_draft(&mut client, &binding, &state, "new");
    assert_eq!(source_parts(&mut client, &binding, &updated)[3], "new");
    assert_eq!(updated["applyPending"], true);
    client.finish();
}

/// Old clients retain BSL admission denial; the protected-view contract requires explicit API 1.19 negotiation.
#[test]
fn standalone_query_session_is_optional_and_keeps_legacy_bsl_admission() {
    let (_directory, root, private) = source_fixture();
    let mut old = Client::new(&root, "en");
    initialize(&mut old, "en-US");
    assert_eq!(
        old.response("query/sessionGet", &json!({}))["error"]["code"],
        -32601
    );
    old.finish();
    fs::write(
        root.join("src/Ext/ManagedApplicationModule.bsl"),
        "x = \"SELECT 1\";",
    )
    .unwrap();
    let mut client = Client::new(&root, "en");
    let binding = source_connect(&mut client, &root, &private, "en-US");
    let root_node = client.request("metadata/root", &source_args(&binding, json!({})));
    let module = json!({"kind":"module","owner":root_node["node"]["id"]["objectId"],"role":"managed-application"});
    let document=client.request("query/open",&source_args(&binding,json!({"node":module,"source":{"value":"Ext/ManagedApplicationModule.bsl","encoding":"utf-8"},"version":"1"})));
    let rejected=client.response("query/sessionOpen",&source_args(&binding,json!({"documentId":document["documentId"],"id":ID,
        "query":{"kind":"bslLiteral","range":{"start":4,"end":14}},"format":"source","userReadOnly":false})));
    assert_eq!(
        rejected["error"]["data"]["details"]["reason"],
        "owner_lock_required"
    );
    client.finish();
}

/// The core enforces one owner session and invalidates it when its acknowledged private record is removed.
#[test]
fn standalone_query_session_deduplicates_owner_and_does_not_revive_a_removed_checkpoint() {
    let (_directory, root, private) = source_fixture();
    let input = XML.strip_prefix('\u{feff}').unwrap();
    let mut client = Client::new(&root, "en");
    let binding = source_connect(&mut client, &root, &private, "en-US");
    let state = open_source(&mut client, &binding, input, false);
    let document=client.request("query/open",&source_args(&binding,json!({"node":binding["node"],"source":{"value":SOURCE,"encoding":"utf-8"},"version":"7","text":input})));
    let duplicate=client.response("query/sessionOpen",&source_args(&binding,json!({"documentId":document["documentId"],"id":"10000000-0000-4000-8000-000000000002",
        "query":{"kind":"dynamicList","name":"List","id":"1"},"format":"editor","userReadOnly":false})));
    assert_eq!(
        duplicate["error"]["data"]["details"]["reason"],
        "already_open"
    );
    assert_eq!(
        duplicate["error"]["data"]["details"]["querySessionId"],
        state["querySessionId"]
    );
    client.request(
        "query/recoveryRemove",
        &args(
            &binding,
            json!({"id":ID,"revision":state["record"]["revision"]}),
        ),
    );
    let late = client.response(
        "query/sessionPrepare",
        &source_args(
            &binding,
            json!({"querySessionId":state["querySessionId"],"revision":state["revision"],
        "owner":native(7,input,false,false),"comments":[]}),
        ),
    );
    assert_eq!(
        late["error"]["data"]["details"]["reason"],
        "unknown_query_session"
    );
    assert_eq!(
        client.request(
            "query/recoveryOpen",
            &json!({"directory":{"value":private,"encoding":"utf-8"}})
        )["records"],
        json!([])
    );
    client.finish();
}

/// A new selection is supplied explicitly; a session cannot keep using an older valid application silently.
#[test]
fn standalone_query_session_checks_current_application_and_the_context_that_opened_its_document() {
    let (_directory, root, private) = source_fixture();
    let input = XML.strip_prefix('\u{feff}').unwrap();
    let mut client = Client::new(&root, "en");
    let binding = source_connect(&mut client, &root, &private, "en-US");
    let state = open_source(&mut client, &binding, input, false);
    let mut current = native(7, input, false, false);
    current["application"] = json!("missing");
    let rejected=client.response("query/sessionPrepare",&source_args(&binding,json!({"querySessionId":state["querySessionId"],"revision":state["revision"],"owner":current,"comments":[]})));
    assert_eq!(
        rejected["error"]["data"]["kind"],
        "query_context_unavailable"
    );
    assert_eq!(
        client.request(
            "query/sessionGet",
            &source_args(&binding, json!({"querySessionId":state["querySessionId"]}))
        )["resumed"],
        false
    );
    client.request(
        "query/sessionClose",
        &source_args(&binding, json!({"querySessionId":state["querySessionId"]})),
    );
    let document=client.request("query/open",&source_args(&binding,json!({"node":binding["node"],"source":{"value":SOURCE,"encoding":"utf-8"},"version":"7","text":input})));
    let manifest = root.join("eska.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n# context changed\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    let rejected=client.response("query/sessionOpen",&source_args(&binding,json!({"documentId":document["documentId"],"id":"10000000-0000-4000-8000-000000000002",
        "query":{"kind":"dynamicList","name":"List","id":"1"},"format":"editor","userReadOnly":false})));
    assert_eq!(
        rejected["error"]["data"]["details"]["reason"],
        "context_changed"
    );
    assert_eq!(
        fs::read_to_string(root.join("src").join(SOURCE)).unwrap(),
        XML
    );
    client.finish();
}

#[path = "sessions/constructors.rs"]
mod constructors;

#[test]
fn private_draft_formatting_revalidates_the_owner_after_refresh() {
    let (_directory, root, private) = source_fixture();
    let manifest = root.join("eska.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[format]\nexclude = [\"**/Form.xml\"]\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    let mut client = Client::new(&root, "en-US");
    let mut binding = source_connect_version(&mut client, &root, &private, "en-US", 55);
    let opened = open_source(&mut client, &binding, XML, false);
    let request = json!({
        "querySessionId": opened["querySessionId"], "revision": opened["revision"],
        "version": "1", "text": "SELECT 1", "userReadOnly": false,
    });
    let original = client.request("format/query", &source_args(&binding, request.clone()));
    assert_eq!(original["effective"]["excluded"], true);

    let owner = binding["node"].clone();
    let refreshed = client.request(
        "metadata/refresh",
        &source_args(&binding, json!({"node":owner})),
    );
    assert_ne!(refreshed["generation"], binding["generation"]);
    binding["generation"] = refreshed["generation"].clone();
    let formatted = client.request("format/query", &source_args(&binding, request.clone()));
    assert_eq!(formatted, original);

    let path = root.join("src").join(SOURCE);
    let external = XML.replace("<other>keep</other>", "<other>external</other>");
    fs::write(&path, &external).unwrap();
    let refreshed = client.request(
        "metadata/refresh",
        &source_args(&binding, json!({"node":owner})),
    );
    assert_ne!(refreshed["generation"], binding["generation"]);
    binding["generation"] = refreshed["generation"].clone();
    let rejected = client.response("format/query", &source_args(&binding, request));
    assert_eq!(rejected["error"]["data"]["details"]["reason"], "conflict");
    assert_eq!(fs::read_to_string(path).unwrap(), external);
}
