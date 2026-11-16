//! JSON Schema describes semantic requests; live discovery supplies valid owners and properties.

use eska_project_core::project::metadata_model::MetadataKind;
use serde_json::{Value, json};

/// Clients can discover request shapes without reading help or guessing XML structure.
pub fn request_schemas() -> Value {
    let string = json!({"type":"string"});
    let optional_id = json!({"type":["string","null"]});
    let values = json!({"type":"object","additionalProperties":{"type":"string"}});
    let references = json!({"type":"object","additionalProperties":{"type":"array","maxItems":100_000,"items":{"type":"string"}}});
    let types = json!({"type":"object","additionalProperties":{"type":"array","maxItems":64,"items":property_key()}});
    let create = object(
        json!({
            "operation":{"const":"create"},"kind":{"enum":MetadataKind::ALL},"name":string,
            "parent":optional_id,"template":optional_id,"propertyValues":values,"propertyReferences":references,"propertyTypes":types,
        }),
        &["operation", "kind", "name"],
    );
    let copy = object(
        json!({
            "operation":{"const":"copy"},"source":string,"name":string,"parent":optional_id,"propertyValues":values,"propertyReferences":references,"propertyTypes":types,
        }),
        &["operation", "source", "name"],
    );
    let delete = object(
        json!({
            "operation":{"const":"delete"},"object":string,"force":{"type":"boolean","default":false},
        }),
        &["operation", "object"],
    );
    let form = object(
        json!({
            "form":string,"snapshot":string,"operations":{"type":"array","maxItems":1024,"items":{"$ref":"#/$defs/formOperation"}},
        }),
        &["form", "snapshot", "operations"],
    );
    let mut form_envelope = envelope(form);
    form_envelope["$defs"] = form_definitions();
    json!({
        "create-module":envelope(object(json!({"operation":{"const":"create-module"},"owner":string,"role":{"type":"string"}}), &["operation","owner","role"])),
        "create":envelope(create),"copy":envelope(copy),"delete":envelope(delete),"form-edit":form_envelope,
    })
}

fn envelope(request: Value) -> Value {
    let mut properties = json!({"schemaVersion":{"const":1}});
    properties["request"] = request;
    let mut schema = object(properties, &["schemaVersion", "request"]);
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema
}

fn object(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({"type":"object","required":required,"additionalProperties":false});
    schema["properties"] = properties;
    schema
}

fn form_definitions() -> Value {
    let string = json!({"type":"string"});
    let key = property_key();
    let named = |kind| {
        object(
            json!({"kind":{"const":kind},"name":string}),
            &["kind", "name"],
        )
    };
    let target = json!({"oneOf":[
        object(json!({"kind":{"const":"root"}}), &["kind"]),
        object(json!({"kind":{"const":"element"},"id":{"type":"integer"}}), &["kind","id"]),
        named("attribute"),named("command"),named("parameter"),
    ]});
    let record = json!({"type":"array","items":{"$ref":"#/$defs/property"}});
    let value = json!({"oneOf":[
        object(json!({"Text":string}), &["Text"]),
        object(json!({"QualifiedText":object(json!({"text":string,"key":key}), &["text","key"])}), &["QualifiedText"]),
        object(json!({"TypedText":object(json!({"text":string,"key":key}), &["text","key"])}), &["TypedText"]),
        object(json!({"Localized":{"type":"array","items":object(json!({"language":string,"content":string}), &["language","content"])}}), &["Localized"]),
        object(json!({"Record":record}), &["Record"]),
    ]});
    let property = object(
        json!({
            "key":key,"qualifiers":{"type":"array","items":{"type":"array","prefixItems":[key,string],"minItems":2,"maxItems":2}},
            "value":{"$ref":"#/$defs/value"},
        }),
        &["key", "qualifiers", "value"],
    );
    let element = object(
        json!({
            "kind":string,"name":string,"properties":record,
            "children":{"type":"array","items":{"$ref":"#/$defs/element"}},
            "auxiliary":{"type":"array","items":{"$ref":"#/$defs/element"}},
        }),
        &["kind", "name"],
    );
    let target_ref = json!({"$ref":"#/$defs/target"});
    let before = json!({"type":["integer","null"]});
    let operation = |kind: &str, mut properties: Value, required: &[&str]| {
        properties["kind"] = json!({"const":kind});
        let mut fields = vec!["kind"];
        fields.extend_from_slice(required);
        object(properties, &fields)
    };
    let operations = vec![
        operation(
            "setProperty",
            json!({"target":target_ref,"property":{"$ref":"#/$defs/property"}}),
            &["target", "property"],
        ),
        operation(
            "removeProperty",
            json!({"target":target_ref,"key":key}),
            &["target", "key"],
        ),
        operation(
            "setEvent",
            json!({"target":target_ref,"event":string,"handler":{"type":["string","null"]}}),
            &["target", "event"],
        ),
        operation(
            "addElement",
            json!({"parent":target_ref,"before":before,"element":{"$ref":"#/$defs/element"}}),
            &["parent", "element"],
        ),
        operation(
            "copyElement",
            json!({"target":target_ref,"parent":target_ref,"before":before,"name":string}),
            &["target", "parent", "name"],
        ),
        operation(
            "moveElement",
            json!({"target":target_ref,"parent":target_ref,"before":before}),
            &["target", "parent"],
        ),
        operation(
            "addDefinition",
            json!({"section":{"enum":["attribute","command","parameter"]},"parent":{"anyOf":[target_ref,{"type":"null"}]},"name":string,"properties":record}),
            &["section", "name"],
        ),
        operation(
            "remove",
            json!({"target":target_ref,"force":{"type":"boolean","default":false}}),
            &["target"],
        ),
    ];
    json!({"target":target,"property":property,"value":value,"element":element,"formOperation":{"oneOf":operations}})
}

fn property_key() -> Value {
    object(
        json!({"namespace":{"type":["string","null"]},"name":{"type":"string","minLength":1}}),
        &["name"],
    )
}
