use super::{
    FormDefinitionKind, FormEditError, FormEditRequest, FormElementSpec, FormOperation, FormTarget,
    document, empty_form, inspect, plan, render,
};
use crate::project::{
    forms::{ElementKind, FORM_NAMESPACE, parse},
    metadata_edit::{EditError, snapshot},
    metadata_model::{MetadataProperty, MetadataValue, ObjectId, PropertyKey},
};

fn form_id() -> ObjectId {
    ObjectId::from_parts(None, "CommonForm", "Test")
}

fn request(input: &str, operations: Vec<FormOperation>) -> FormEditRequest {
    FormEditRequest {
        form: form_id(),
        snapshot: snapshot(input),
        operations,
    }
}

fn property(name: &str, text: &str) -> MetadataProperty {
    MetadataProperty {
        key: render::key(FORM_NAMESPACE, name),
        qualifiers: Vec::new(),
        value: MetadataValue::Text(text.to_owned()),
    }
}

fn spec(kind: &str, name: &str) -> FormElementSpec {
    FormElementSpec {
        kind: kind.to_owned(),
        name: name.to_owned(),
        properties: Vec::new(),
        children: Vec::new(),
        auxiliary: Vec::new(),
    }
}

fn source(body: &str) -> String {
    format!(
        "\u{feff}<?xml version='1.0'?>\r\n<Form xmlns='{FORM_NAMESPACE}' xmlns:c='urn:custom'>\r\n<!--keep exactly-->{body}<c:Extra c:flag='yes'>opaque &amp; data</c:Extra>\r\n</Form>\r\n"
    )
}

#[test]
fn blank_form_has_native_autobar_and_independent_definition_identity_spaces() {
    let input = empty_form();
    let operations = vec![
        FormOperation::AddElement {
            parent: FormTarget::Root {},
            before: None,
            element: spec("InputField", "Value"),
        },
        FormOperation::AddDefinition {
            section: FormDefinitionKind::Attribute,
            parent: None,
            name: "Rows".into(),
            properties: Vec::new(),
        },
        FormOperation::AddDefinition {
            section: FormDefinitionKind::Attribute,
            parent: Some(FormTarget::Attribute {
                name: "Rows".into(),
            }),
            name: "Quantity".into(),
            properties: Vec::new(),
        },
        FormOperation::AddDefinition {
            section: FormDefinitionKind::Command,
            parent: None,
            name: "Run".into(),
            properties: Vec::new(),
        },
        FormOperation::AddDefinition {
            section: FormDefinitionKind::Parameter,
            parent: None,
            name: "Mode".into(),
            properties: Vec::new(),
        },
    ];
    let (edit, ids) = plan(&input, &request(&input, operations)).unwrap();
    assert_eq!(
        ids.iter().map(|item| item.id).collect::<Vec<_>>(),
        [Some(1), Some(1), Some(1), Some(1), None]
    );
    let form = parse("test", edit.output().to_owned()).unwrap();
    assert_eq!(form.diagnostics, []);
    assert_eq!(
        form.root.auxiliary[0].kind,
        Some(ElementKind::AutoCommandBar)
    );
    let schema = inspect(&form_id(), "Ext/Form.xml".into(), edit.output(), true).unwrap();
    assert!(schema.nodes.iter().any(|node| node.target
        == FormTarget::Attribute {
            name: "Rows.Quantity".into()
        }));
    assert!(!edit.output().contains("Parameter name=\"Mode\" id="));
}

#[test]
fn subtree_copy_rekeys_nested_ids_but_preserves_data_binding_events_and_unknown_xml() {
    let input = source(
        "<ChildItems><UsualGroup name='Group' id='10'><ChildItems><InputField name='Amount' id='011'><DataPath>Amount</DataPath><Events><Event name='OnChange'>AmountOnChange</Event></Events><c:Opaque x='1'/><ContextMenu name='Menu' id='12'/></InputField><Button name='Action' id='13'><AssociatedTableElementId> +11 </AssociatedTableElementId></Button></ChildItems></UsualGroup></ChildItems>",
    );
    let operation = FormOperation::CopyElement {
        target: FormTarget::Element { id: 10 },
        parent: FormTarget::Root {},
        before: None,
        name: "GroupCopy".into(),
    };
    let request = request(&input, vec![operation]);
    let (edit, ids) = plan(&input, &request).unwrap();
    let (again, repeated) = plan(&input, &request).unwrap();
    assert_eq!(edit.output(), again.output());
    assert_eq!(ids, repeated);
    assert_eq!(
        ids.iter().map(|item| item.id).collect::<Vec<_>>(),
        [Some(14), Some(15), Some(16), Some(17)]
    );
    assert_eq!(
        edit.output().matches("<DataPath>Amount</DataPath>").count(),
        2
    );
    assert_eq!(edit.output().matches("<c:Opaque x='1'/>").count(), 2);
    assert_eq!(edit.output().matches(">AmountOnChange</Event>").count(), 2);
    assert!(
        edit.output()
            .contains("<AssociatedTableElementId> +11 </AssociatedTableElementId>")
    );
    assert!(
        edit.output()
            .contains("<AssociatedTableElementId>15</AssociatedTableElementId>")
    );
    assert!(
        edit.output()
            .starts_with("\u{feff}<?xml version='1.0'?>\r\n")
    );
    assert!(
        edit.output()
            .ends_with("<c:Extra c:flag='yes'>opaque &amp; data</c:Extra>\r\n</Form>\r\n")
    );
    assert_eq!(
        parse("test", edit.output().to_owned()).unwrap().diagnostics,
        []
    );
}

#[test]
fn copied_table_commands_follow_the_copied_element_but_keep_external_links() {
    let input = source(
        "<ChildItems><UsualGroup name='Group' id='1'><ChildItems><Table name='Rows' id='2'><DataPath>DataRows</DataPath><Events><Event name='OnChange'>RowsOnChange</Event></Events><AutoCommandBar name='RowsBar' id='3'><CommandSource>Item.rows</CommandSource><ChildItems><Button name='AddRow' id='4'><CommandName>Form.Item.ROWS.StandardCommand.Add</CommandName></Button></ChildItems></AutoCommandBar></Table><Button name='External' id='5'><CommandName>Form.Item.Outside.StandardCommand.Add</CommandName><Item>Outside</Item><c:CommandName>Form.Item.Rows.StandardCommand.Add</c:CommandName></Button><Button name='Custom' id='6'><CommandName>Form.Command.Rows</CommandName><Element>Rows</Element></Button></ChildItems></UsualGroup><Table name='Outside' id='7'/></ChildItems>",
    );
    let (edit, assigned) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::CopyElement {
                target: FormTarget::Element { id: 1 },
                parent: FormTarget::Root {},
                before: None,
                name: "GroupCopy".to_owned(),
            }],
        ),
    )
    .unwrap();
    let document = document::xml(edit.output()).unwrap();
    let copied = document::target(&document, &assigned[0].target).unwrap();
    let table = copied
        .descendants()
        .find(|node| node.has_tag_name((FORM_NAMESPACE, "Table")))
        .unwrap();
    assert_eq!(table.attribute("name"), Some("Rows_Copy1"));
    let values = |namespace, property| {
        copied
            .descendants()
            .filter(|node| node.has_tag_name((namespace, property)))
            .filter_map(|node| node.text())
            .collect::<Vec<_>>()
    };
    assert_eq!(values(FORM_NAMESPACE, "CommandSource"), ["Item.Rows_Copy1"]);
    assert_eq!(
        values(FORM_NAMESPACE, "CommandName"),
        [
            "Form.Item.Rows_Copy1.StandardCommand.Add",
            "Form.Item.Outside.StandardCommand.Add",
            "Form.Command.Rows",
        ]
    );
    assert_eq!(values(FORM_NAMESPACE, "Element"), ["Rows_Copy1"]);
    assert_eq!(values(FORM_NAMESPACE, "Item"), ["Outside"]);
    assert_eq!(values(FORM_NAMESPACE, "DataPath"), ["DataRows"]);
    assert_eq!(values(FORM_NAMESPACE, "Event"), ["RowsOnChange"]);
    assert_eq!(
        values("urn:custom", "CommandName"),
        ["Form.Item.Rows.StandardCommand.Add"]
    );
    let original = document::xml(&input).unwrap();
    let original = document::target(&original, &FormTarget::Element { id: 1 }).unwrap();
    assert!(edit.output().contains(&input[original.range()]));
    assert!(matches!(
        plan(
            edit.output(),
            &request(
                edit.output(),
                vec![FormOperation::Remove {
                    target: document::target_of(table).unwrap(),
                    force: false,
                }],
            ),
        ),
        Err(FormEditError::References { .. })
    ));
}

#[test]
fn move_preserves_namespace_bindings_and_refuses_cycles_and_invalid_page_nesting() {
    let input = source(
        "<ChildItems><UsualGroup name='Outer' id='1' xmlns:c='urn:origin'><ChildItems><UsualGroup name='Inner' id='2'><c:Raw>c:Type</c:Raw></UsualGroup></ChildItems></UsualGroup><UsualGroup name='Target' id='3' xmlns:c='urn:destination'/></ChildItems>",
    );
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::MoveElement {
                target: FormTarget::Element { id: 2 },
                parent: FormTarget::Element { id: 3 },
                before: None,
            }],
        ),
    )
    .unwrap();
    let xml = document::xml(edit.output()).unwrap();
    let moved = document::target(&xml, &FormTarget::Element { id: 2 }).unwrap();
    assert_eq!(moved.lookup_namespace_uri(Some("c")), Some("urn:origin"));
    assert!(
        moved
            .children()
            .any(|node| node.has_tag_name(("urn:origin", "Raw")))
    );
    for operation in [
        FormOperation::MoveElement {
            target: FormTarget::Element { id: 1 },
            parent: FormTarget::Element { id: 2 },
            before: None,
        },
        FormOperation::AddElement {
            parent: FormTarget::Root {},
            before: None,
            element: spec("Page", "Wrong"),
        },
    ] {
        assert!(matches!(
            plan(&input, &request(&input, vec![operation])),
            Err(FormEditError::Invalid { .. })
        ));
    }
}

#[test]
fn scalar_property_and_events_keep_other_bytes_and_validate_known_domains() {
    let input = source(
        "<ChildItems><InputField name='Value' id='1'><Visible>true</Visible><Events><Event name='OnChange'>OldHandler</Event></Events></InputField></ChildItems>",
    );
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![
                FormOperation::SetProperty {
                    target: FormTarget::Element { id: 1 },
                    property: property("Visible", "false"),
                },
                FormOperation::SetEvent {
                    target: FormTarget::Element { id: 1 },
                    event: "OnChange".into(),
                    handler: Some("NewHandler".into()),
                },
            ],
        ),
    )
    .unwrap();
    assert_eq!(
        edit.output(),
        input
            .replace("<Visible>true", "<Visible>false")
            .replace(">OldHandler<", ">NewHandler<")
    );
    for operation in [
        FormOperation::SetProperty {
            target: FormTarget::Element { id: 1 },
            property: property("Visible", "perhaps"),
        },
        FormOperation::SetEvent {
            target: FormTarget::Element { id: 1 },
            event: "ImaginaryEvent".into(),
            handler: Some("Handler".into()),
        },
        FormOperation::SetProperty {
            target: FormTarget::Element { id: 1 },
            property: property("ChildItems", "x"),
        },
    ] {
        assert!(matches!(
            plan(&input, &request(&input, vec![operation])),
            Err(FormEditError::Invalid { .. })
        ));
    }
    let (same, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::SetProperty {
                target: FormTarget::Element { id: 1 },
                property: property("Visible", "true"),
            }],
        ),
    )
    .unwrap();
    assert!(same.is_empty());
}

#[test]
fn removal_checks_numeric_element_references() {
    let input = source(
        "<ChildItems><Table name='Rows' id='001'/><Button name='Action' id='2'><AssociatedTableElementId> +1 </AssociatedTableElementId><c:ItemId>1</c:ItemId></Button></ChildItems>",
    );
    let target = FormTarget::Element { id: 1 };
    let error = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::Remove {
                target: target.clone(),
                force: false,
            }],
        ),
    )
    .unwrap_err();
    let FormEditError::References {
        target: referenced,
        paths,
    } = error
    else {
        panic!("expected element references, got {error:?}");
    };
    assert_eq!(referenced, target);
    assert_eq!(paths.len(), 1);
    assert!(paths[0].starts_with("AssociatedTableElementId@"));
}

#[test]
fn removal_requires_explicit_force_for_live_form_references() {
    let input = source(
        "<ChildItems><InputField name='Value' id='1'><DataPath>Value</DataPath></InputField></ChildItems><Attributes><Attribute name='Value' id='1'><Type/></Attribute></Attributes>",
    );
    let target = FormTarget::Attribute {
        name: "Value".into(),
    };
    assert!(matches!(
        plan(
            &input,
            &request(
                &input,
                vec![FormOperation::Remove {
                    target: target.clone(),
                    force: false
                }]
            )
        ),
        Err(FormEditError::References { .. })
    ));
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::Remove {
                target,
                force: true,
            }],
        ),
    )
    .unwrap();
    assert!(!edit.output().contains("<Attribute name="));
    assert!(edit.output().contains("<DataPath>Value</DataPath>"));
}

#[test]
fn removing_properties_cannot_bypass_auxiliary_element_reference_checks() {
    let input = source(
        "<AutoCommandBar name='FormCommandBar' id='-1'/><ChildItems><InputField name='Value' id='1'><Visible>false</Visible><ContextMenu name='Menu' id='2'/><ExtendedTooltip name='Hint' id='3'/></InputField></ChildItems>",
    );
    for (target, key) in [
        (FormTarget::Root {}, "AutoCommandBar"),
        (FormTarget::Element { id: 1 }, "ContextMenu"),
        (FormTarget::Element { id: 1 }, "ExtendedTooltip"),
    ] {
        for operation in [
            FormOperation::RemoveProperty {
                target: target.clone(),
                key: render::key(FORM_NAMESPACE, key),
            },
            FormOperation::SetProperty {
                target,
                property: property(key, ""),
            },
        ] {
            assert!(matches!(
                plan(&input, &request(&input, vec![operation])),
                Err(FormEditError::Invalid {
                    field: "property",
                    reason: "structural_property"
                })
            ));
        }
    }
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::RemoveProperty {
                target: FormTarget::Element { id: 1 },
                key: render::key(FORM_NAMESPACE, "Visible"),
            }],
        ),
    )
    .unwrap();
    assert_eq!(edit.output(), input.replace("<Visible>false</Visible>", ""));
}

#[test]
fn removing_copied_group_keeps_bindings_to_same_named_attributes() {
    let input = source(
        "<ChildItems><UsualGroup name='Group' id='1'><ChildItems><InputField name='Amount' id='2'><DataPath>Amount</DataPath></InputField></ChildItems></UsualGroup></ChildItems><Attributes><Attribute name='Amount' id='1'><Type/></Attribute></Attributes>",
    );
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![
                FormOperation::CopyElement {
                    target: FormTarget::Element { id: 1 },
                    parent: FormTarget::Root {},
                    before: None,
                    name: "GroupCopy".into(),
                },
                FormOperation::Remove {
                    target: FormTarget::Element { id: 1 },
                    force: false,
                },
            ],
        ),
    )
    .unwrap();
    assert!(!edit.output().contains("name='Group'"));
    assert!(edit.output().contains("<DataPath>Amount</DataPath>"));
    assert!(edit.output().contains("<Attribute name='Amount'"));
    let remove_attribute = request(
        edit.output(),
        vec![FormOperation::Remove {
            target: FormTarget::Attribute {
                name: "Amount".into(),
            },
            force: false,
        }],
    );
    assert!(matches!(
        plan(edit.output(), &remove_attribute),
        Err(FormEditError::References { .. })
    ));
}

#[test]
fn removal_resolves_full_attribute_paths_and_command_element_domains() {
    let input = source(
        "<ChildItems><InputField name='Value' id='1'><DataPath>Other.Value</DataPath></InputField><Button name='Run' id='2'><CommandName>Form.Command.Run</CommandName></Button><Button name='Execute' id='3'><CommandName>Form.Command.Run</CommandName></Button></ChildItems><Attributes><Attribute name='Rows' id='1'><Type/><Columns><Column name='Value' id='1'><Type/></Column></Columns></Attribute><Attribute name='Other' id='2'><Type/><Columns><Column name='Value' id='1'><Type/></Column></Columns></Attribute></Attributes><Commands><Command name='Run' id='1'/></Commands>",
    );
    for target in [
        FormTarget::Attribute {
            name: "Rows.Value".into(),
        },
        FormTarget::Element { id: 2 },
    ] {
        assert!(
            plan(
                &input,
                &request(
                    &input,
                    vec![FormOperation::Remove {
                        target,
                        force: false,
                    }],
                ),
            )
            .is_ok()
        );
    }
    for target in [
        FormTarget::Attribute {
            name: "Other.Value".into(),
        },
        FormTarget::Command { name: "Run".into() },
    ] {
        assert!(matches!(
            plan(
                &input,
                &request(
                    &input,
                    vec![FormOperation::Remove {
                        target,
                        force: false,
                    }],
                ),
            ),
            Err(FormEditError::References { .. })
        ));
    }
}

#[test]
fn removal_detects_element_command_sources_and_standard_commands() {
    for binding in [
        "<CommandSource>Item.Rows</CommandSource>",
        "<CommandName>Form.Item.Rows.StandardCommand.Add</CommandName>",
    ] {
        let input = source(&format!(
            "<ChildItems><Table name='Rows' id='1'/><Button name='Action' id='2'>{binding}</Button></ChildItems>"
        ));
        assert!(matches!(
            plan(
                &input,
                &request(
                    &input,
                    vec![FormOperation::Remove {
                        target: FormTarget::Element { id: 1 },
                        force: false,
                    }],
                ),
            ),
            Err(FormEditError::References { .. })
        ));
    }
}

#[test]
fn prepared_input_rejects_stale_snapshot_and_ambiguous_ids_and_extra_fields() {
    let input = empty_form();
    let mut stale = request(&input, Vec::new());
    stale.snapshot = "wrong".into();
    assert!(matches!(
        plan(&input, &stale),
        Err(FormEditError::Edit(EditError::Conflict))
    ));
    let duplicate = source(
        "<ChildItems><InputField name='One' id='1'/><InputField name='Two' id='1'/></ChildItems>",
    );
    assert!(matches!(
        plan(&duplicate, &request(&duplicate, Vec::new())),
        Err(FormEditError::Invalid { .. })
    ));
    assert!(serde_json::from_str::<FormTarget>(r#"{"kind":"root","unexpected":1}"#).is_err());
    assert!(
        serde_json::from_str::<FormTarget>(r#"{"kind":"element","id":1,"unexpected":1}"#).is_err()
    );
    let borrowed = source("<BaseForm/>");
    assert!(matches!(
        plan(&borrowed, &request(&borrowed, Vec::new())),
        Err(FormEditError::Invalid {
            reason: "borrowed_form",
            ..
        })
    ));
}

fn type_property(digits: &str, fraction: &str) -> MetadataProperty {
    let core = "http://v8.1c.ru/8.1/data/core";
    let part = |name: &str, text: &str| MetadataProperty {
        key: render::key(core, name),
        qualifiers: Vec::new(),
        value: MetadataValue::Text(text.into()),
    };
    MetadataProperty {
        key: render::key(FORM_NAMESPACE, "Type"),
        qualifiers: Vec::new(),
        value: MetadataValue::Record(vec![
            MetadataProperty {
                key: render::key(core, "Type"),
                qualifiers: Vec::new(),
                value: MetadataValue::QualifiedText {
                    text: "ignored:decimal".into(),
                    key: PropertyKey {
                        namespace: Some("http://www.w3.org/2001/XMLSchema".into()),
                        name: "decimal".into(),
                    },
                },
            },
            MetadataProperty {
                key: render::key(core, "NumberQualifiers"),
                qualifiers: Vec::new(),
                value: MetadataValue::Record(vec![
                    part("Digits", digits),
                    part("FractionDigits", fraction),
                    part("AllowedSign", "Any"),
                ]),
            },
        ]),
    }
}

#[test]
fn editing_flags_preserves_active_standard_commands() {
    let input = source(
        r#"<ChildItems>
        <Button name="Customize" id="1"><CommandName>Form.StandardCommand.CustomizeForm</CommandName></Button>
        <Table name="Folders" id="2"><ContextMenu name="Menu" id="3"><ChildItems>
        <Button name="Remove" id="4"><CommandName>Delete</CommandName></Button>
        </ChildItems></ContextMenu><ChildItems/></Table>
        <Button name="Create" id="5"><CommandName>Form.Item.Folders.StandardCommand.Create</CommandName></Button>
        <Table name="Items" id="6"><ChildItems/></Table>
        </ChildItems>"#,
    );
    let edit = |target, name| FormOperation::SetProperty {
        target,
        property: property(name, "false"),
    };
    for op in [
        edit(FormTarget::Root {}, "Customizable"),
        edit(FormTarget::Element { id: 2 }, "ChangeRowSet"),
    ] {
        assert!(matches!(
            plan(&input, &request(&input, vec![op])),
            Err(FormEditError::Invalid {
                reason: "used_standard_commands",
                ..
            })
        ));
    }
    let (result, _) = plan(
        &input,
        &request(
            &input,
            vec![edit(FormTarget::Element { id: 6 }, "ChangeRowSet")],
        ),
    )
    .unwrap();
    assert!(
        result
            .output()
            .contains("<ChangeRowSet>false</ChangeRowSet>")
    );
}

#[test]
fn decoration_and_tooltip_do_not_expose_field_read_only() {
    for kind in ["LabelDecoration", "PictureDecoration", "ExtendedTooltip"] {
        let kind = crate::project::forms::ElementKind::from_xml_name(kind).unwrap();
        let fields = crate::project::forms::authoring_properties(kind);
        assert!(fields.iter().all(|field| field.name != "ReadOnly"));
    }
    let field = crate::project::forms::ElementKind::CalendarField;
    assert!(
        crate::project::forms::authoring_properties(field)
            .iter()
            .any(|field| field.name == "ReadOnly")
    );
}

#[test]
fn popup_and_button_group_use_distinct_representation_values() {
    let input = source(
        "<ChildItems><Popup name='More' id='1'><Representation>Picture</Representation></Popup><ButtonGroup name='Commands' id='2'/></ChildItems>",
    );
    let edit = |id, value| {
        plan(
            &input,
            &request(
                &input,
                vec![FormOperation::SetProperty {
                    target: FormTarget::Element { id },
                    property: property("Representation", value),
                }],
            ),
        )
    };
    assert!(edit(1, "Text").is_ok());
    assert!(edit(1, "Compact").is_err());
    assert!(edit(2, "Compact").is_ok());
    assert!(edit(2, "Text").is_err());
}

#[test]
fn button_type_is_an_enum_and_remains_writable_after_an_edit() {
    let input = source(
        "<ChildItems><Button name='Choose' id='1'><Type>UsualButton</Type></Button></ChildItems>",
    );
    let target = FormTarget::Element { id: 1 };
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::SetProperty {
                target: target.clone(),
                property: property("Type", "Hyperlink"),
            }],
        ),
    )
    .unwrap();
    assert!(edit.output().contains("<Type>Hyperlink</Type>"));
    let schema = super::inspect_draft(edit.output(), true).unwrap();
    let button = schema
        .nodes
        .iter()
        .find(|node| node.target == target)
        .unwrap();
    assert!(
        button
            .properties
            .iter()
            .any(|field| field.property.key.name == "Type" && field.writable)
    );
}

#[test]
fn type_descriptions_share_numeric_validation_and_resolve_namespaces_by_identity() {
    let input = empty_form();
    let operation = |property| FormOperation::AddDefinition {
        section: FormDefinitionKind::Attribute,
        parent: None,
        name: "Amount".into(),
        properties: vec![property],
    };
    let (edit, _) = plan(
        &input,
        &request(&input, vec![operation(type_property("12", "2"))]),
    )
    .unwrap();
    assert!(!edit.output().contains("ignored:"));
    assert!(edit.output().contains("xs:decimal"));
    assert!(
        plan(
            &input,
            &request(&input, vec![operation(type_property("0", "0"))])
        )
        .is_ok()
    );
    for (digits, fraction) in [("0", "1"), ("39", "2"), ("5", "6")] {
        assert!(
            plan(
                &input,
                &request(&input, vec![operation(type_property(digits, fraction))])
            )
            .is_err()
        );
    }
    assert!(
        plan(
            &input,
            &request(&input, vec![operation(property("Type", "garbage"))])
        )
        .is_err()
    );
}

#[test]
fn structured_attributes_escape_both_quotes_and_preserve_whitespace() {
    let input = source("<c:Custom><c:Child value='before'/></c:Custom>");
    let value = "double\" single' & < >\t\n\r";
    let property = MetadataProperty {
        key: render::key("urn:custom", "Custom"),
        qualifiers: Vec::new(),
        value: MetadataValue::Record(vec![MetadataProperty {
            key: render::key("urn:custom", "Child"),
            qualifiers: vec![(
                PropertyKey {
                    namespace: None,
                    name: "value".into(),
                },
                value.into(),
            )],
            value: MetadataValue::Text(String::new()),
        }]),
    };
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::SetProperty {
                target: FormTarget::Root {},
                property,
            }],
        ),
    )
    .unwrap();
    let xml = document::xml(edit.output()).unwrap();
    let child = xml
        .descendants()
        .find(|node| node.has_tag_name(("urn:custom", "Child")))
        .unwrap();
    assert_eq!(child.attribute("value"), Some(value));
}

#[test]
fn structured_values_inherit_the_rendered_namespace_scope() {
    let input = source("<c:Custom/>");
    let property = MetadataProperty {
        key: render::key("urn:custom", "Custom"),
        qualifiers: Vec::new(),
        value: MetadataValue::Record(vec![MetadataProperty {
            key: PropertyKey {
                namespace: None,
                name: "Bare".into(),
            },
            qualifiers: Vec::new(),
            value: MetadataValue::Record(vec![
                property("Width", "10"),
                MetadataProperty {
                    key: render::key("urn:nested", "Inner"),
                    qualifiers: Vec::new(),
                    value: MetadataValue::Record(vec![MetadataProperty {
                        key: render::key("urn:nested", "Value"),
                        qualifiers: Vec::new(),
                        value: MetadataValue::Text("nested".into()),
                    }]),
                },
            ]),
        }]),
    };
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::SetProperty {
                target: FormTarget::Root {},
                property,
            }],
        ),
    )
    .unwrap();
    let xml = document::xml(edit.output()).unwrap();
    for (name, namespace) in [
        ("Custom", Some("urn:custom")),
        ("Bare", None),
        ("Width", Some(FORM_NAMESPACE)),
        ("Inner", Some("urn:nested")),
        ("Value", Some("urn:nested")),
    ] {
        let node = xml
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == name)
            .unwrap();
        assert_eq!(
            node.tag_name()
                .namespace()
                .filter(|namespace| !namespace.is_empty()),
            namespace,
            "{name}"
        );
    }
}

#[test]
fn copy_and_move_preserve_absent_default_namespace_and_destination_child_scope() {
    let input = format!(
        "<f:Form xmlns:f='{FORM_NAMESPACE}'><f:ChildItems><f:InputField name='Value' id='1'><Opaque/></f:InputField><f:UsualGroup name='Target' id='2' xmlns='{FORM_NAMESPACE}'><ChildItems xmlns:f='urn:foreign'/></f:UsualGroup></f:ChildItems></f:Form>"
    );
    for operation in [
        FormOperation::CopyElement {
            target: FormTarget::Element { id: 1 },
            parent: FormTarget::Element { id: 2 },
            before: None,
            name: "Copy".into(),
        },
        FormOperation::MoveElement {
            target: FormTarget::Element { id: 1 },
            parent: FormTarget::Element { id: 2 },
            before: None,
        },
    ] {
        let (edit, _) = plan(&input, &request(&input, vec![operation])).unwrap();
        let xml = document::xml(edit.output()).unwrap();
        let target = document::target(&xml, &FormTarget::Element { id: 2 }).unwrap();
        let field = target
            .descendants()
            .find(|node| node.has_tag_name((FORM_NAMESPACE, "InputField")))
            .unwrap();
        let opaque = field.first_element_child().unwrap();
        assert!(opaque.tag_name().namespace().is_none_or(str::is_empty));
    }
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::AddElement {
                parent: FormTarget::Element { id: 2 },
                before: None,
                element: spec("InputField", "NewValue"),
            }],
        ),
    )
    .unwrap();
    let xml = document::xml(edit.output()).unwrap();
    let new = document::target(&xml, &FormTarget::Element { id: 3 }).unwrap();
    assert!(new.has_tag_name((FORM_NAMESPACE, "InputField")));
}

#[test]
fn numeric_booleans_and_foreign_properties_are_not_element_references() {
    let input = source(
        "<ChildItems><UsualGroup name='Group' id='1'><ChildItems><Button name='Run' id='2'><DefaultItem>1</DefaultItem><DefaultButton>1</DefaultButton><c:ElementId>1</c:ElementId><AssociatedTableElementId>1</AssociatedTableElementId></Button></ChildItems></UsualGroup></ChildItems>",
    );
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::CopyElement {
                target: FormTarget::Element { id: 1 },
                parent: FormTarget::Root {},
                before: None,
                name: "GroupCopy".into(),
            }],
        ),
    )
    .unwrap();
    assert_eq!(
        edit.output()
            .matches("<DefaultItem>1</DefaultItem>")
            .count(),
        2
    );
    assert_eq!(
        edit.output()
            .matches("<DefaultButton>1</DefaultButton>")
            .count(),
        2
    );
    assert_eq!(
        edit.output()
            .matches("<c:ElementId>1</c:ElementId>")
            .count(),
        2
    );
    assert!(
        edit.output()
            .contains("<AssociatedTableElementId>3</AssociatedTableElementId>")
    );

    let input = source(
        "<ChildItems><InputField name='Value' id='1'/><Button name='Run' id='2'><DefaultItem>1</DefaultItem><DefaultButton>1</DefaultButton></Button></ChildItems>",
    );
    assert!(
        plan(
            &input,
            &request(
                &input,
                vec![FormOperation::Remove {
                    target: FormTarget::Element { id: 1 },
                    force: false,
                }]
            )
        )
        .is_ok()
    );
}

#[test]
fn new_definitions_and_events_use_the_destination_container_namespace() {
    let input = format!(
        "<Form xmlns='{FORM_NAMESPACE}' xmlns:f='{FORM_NAMESPACE}'><f:Attributes xmlns='urn:foreign'/><f:Events xmlns='urn:foreign'/></Form>"
    );
    let (edit, _) = plan(
        &input,
        &request(
            &input,
            vec![
                FormOperation::AddDefinition {
                    section: FormDefinitionKind::Attribute,
                    parent: None,
                    name: "Value".into(),
                    properties: Vec::new(),
                },
                FormOperation::SetEvent {
                    target: FormTarget::Root {},
                    event: "OnOpen".into(),
                    handler: Some("Open".into()),
                },
            ],
        ),
    )
    .unwrap();
    let xml = document::xml(edit.output()).unwrap();
    for name in ["Attribute", "Type", "Event"] {
        let node = xml
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == name)
            .unwrap();
        assert_eq!(node.tag_name().namespace(), Some(FORM_NAMESPACE));
    }
}

#[test]
fn conditional_appearance_roundtrips_typed_values_and_blocks_dangling_element_links() {
    let input = source(
        "<ChildItems><InputField name='AmountField' id='1'/></ChildItems><Attributes><Attribute name='Amount' id='1'/></Attributes>",
    );
    let operation = serde_json::from_value(serde_json::json!({
        "kind":"setConditionalAppearance",
        "rules":[{
            "original":null,"enabled":false,
            "fields":[{"name":"AmountField","enabled":true}],
            "conditions":[{"kind":"group","enabled":true,"group":"OrGroup","items":[
                {"kind":"comparison","enabled":true,"field":"Amount","operator":"Less","operand":{"kind":"number","value":"1.5"}},
                {"kind":"comparison","enabled":false,"field":"Amount","operator":"NotFilled","operand":null}
            ]}],
            "format":[
                {"parameter":"background","enabled":true,"value":{"kind":"color","value":"#FFEEDD"}},
                {"parameter":"font","enabled":true,"value":{"kind":"font","value":{"reference":"sys:DefaultGUIFont","effects":["bold"],"scale":null}}},
                {"parameter":"text","enabled":true,"value":{"kind":"localized","value":[{"language":"ru","content":"< 1,5 & пусто"}]}}
            ]
        }]
    })).unwrap();
    let (changed, _) = plan(&input, &request(&input, vec![operation])).unwrap();
    let output = changed.output();
    assert!(output.starts_with("\u{feff}<?xml version='1.0'?>\r\n"));
    assert!(output.contains("<!--keep exactly-->"));
    let schema = inspect(&form_id(), "Form.xml".into(), output, true).unwrap();
    let rules = schema.conditional_appearance.unwrap().rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].enabled, Some(false));
    assert_eq!(rules[0].format.as_ref().map(Vec::len), Some(3));
    assert_eq!(rules[0].conditions.as_ref().map(Vec::len), Some(1));
    let (same, _) = plan(
        output,
        &request(
            output,
            vec![FormOperation::SetConditionalAppearance { rules }],
        ),
    )
    .unwrap();
    assert_eq!(same.output(), output);
    for target in [
        FormTarget::Element { id: 1 },
        FormTarget::Attribute {
            name: "Amount".to_owned(),
        },
    ] {
        assert!(matches!(
            plan(
                output,
                &request(
                    output,
                    vec![FormOperation::Remove {
                        target,
                        force: false
                    }]
                )
            ),
            Err(FormEditError::References { .. })
        ));
    }
}

#[test]
fn appearance_edit_preserves_unsupported_filter_and_other_rule_bytes() {
    let unknown = concat!(
        "<s:filter><!--comment--><s:item xsi:type='s:FilterItemComparison'>",
        "<s:left xsi:type='d:Field'>Attr</s:left><s:comparisonType>InList</s:comparisonType>",
        "<s:right xsi:type='c:ValueListType'><c:future/></s:right></s:item></s:filter>"
    );
    let existing =
        format!("<s:item><s:selection/><c:Unknown flag='yes'/>{unknown}<s:appearance/></s:item>");
    let untouched = "<s:item><!--retained--><s:selection/><s:filter/><s:appearance/></s:item>";
    let input = source(&format!(
        "<Attributes><ConditionalAppearance xmlns:s='{}' xmlns:d='{}' xmlns:xsi='{}'>\r\n{existing}<!--gap-->{untouched}\r\n</ConditionalAppearance></Attributes>",
        "http://v8.1c.ru/8.1/data-composition-system/settings",
        "http://v8.1c.ru/8.1/data-composition-system/core",
        "http://www.w3.org/2001/XMLSchema-instance",
    ));
    let mut rules = inspect(&form_id(), "Form.xml".into(), &input, true)
        .unwrap()
        .conditional_appearance
        .unwrap()
        .rules;
    assert_eq!(rules[0].conditions, None);
    rules[0].enabled = Some(false);
    rules[0].format = Some(vec![
        serde_json::from_value(serde_json::json!({
            "parameter":"foreground","enabled":true,"value":{"kind":"color","value":"#FEEC02"}
        }))
        .unwrap(),
    ]);
    rules.swap(0, 1);
    let (edited, _) = plan(
        &input,
        &request(
            &input,
            vec![FormOperation::SetConditionalAppearance {
                rules: rules.clone(),
            }],
        ),
    )
    .unwrap();
    assert!(edited.output().contains(unknown));
    assert!(edited.output().contains(untouched));
    assert!(edited.output().contains("<c:Unknown flag='yes'/>"));
    assert!(edited.output().contains("<!--gap-->"));
    assert!(edited.output().find(untouched).unwrap() < edited.output().find(unknown).unwrap());
    rules[1].conditions = Some(Vec::new());
    assert!(matches!(
        plan(
            &input,
            &request(
                &input,
                vec![FormOperation::SetConditionalAppearance { rules }]
            )
        ),
        Err(FormEditError::Invalid {
            reason: "unsupported_rule_section",
            ..
        })
    ));
}

#[test]
fn conditional_appearance_inserts_missing_sections_in_designer_order() {
    use serde_json::json;

    for existing in ["<s:item/>", "<s:item><s:appearance/></s:item>"] {
        let input = source(&format!(
            "<ChildItems><InputField name='AmountField' id='1'/></ChildItems><Attributes><ConditionalAppearance xmlns:s='http://v8.1c.ru/8.1/data-composition-system/settings'>{existing}</ConditionalAppearance></Attributes>"
        ));
        let operation = serde_json::from_value(json!({
            "kind":"setConditionalAppearance",
            "rules":[{"original":0,"enabled":false,"fields":[{"name":"AmountField","enabled":true}],"conditions":[
                {"kind":"comparison","enabled":true,"field":"Amount","operator":"NotFilled","operand":null}
            ],"format":[{"parameter":"visible","enabled":true,"value":{"kind":"boolean","value":false}}]}]
        })).unwrap();
        let (edit, _) = plan(&input, &request(&input, vec![operation])).unwrap();
        let xml = document::xml(edit.output()).unwrap();
        let rule = xml
            .descendants()
            .find(|node| node.has_tag_name("ConditionalAppearance"))
            .and_then(|node| node.first_element_child())
            .unwrap();
        let sections: Vec<_> = rule
            .children()
            .filter(roxmltree::Node::is_element)
            .map(|node| node.tag_name().name())
            .collect();
        assert_eq!(sections, ["use", "selection", "filter", "appearance"]);
    }
}

#[test]
fn conditional_appearance_rejects_new_rules_with_unknown_formatted_fields() {
    let input = empty_form();
    let operation = serde_json::from_value(serde_json::json!({
        "kind":"setConditionalAppearance",
        "rules":[{"fields":[{"name":"Missing","enabled":true}],"format":[
            {"parameter":"visible","enabled":true,"value":{"kind":"boolean","value":false}}
        ]}]
    }))
    .unwrap();
    assert!(matches!(
        plan(&input, &request(&input, vec![operation])),
        Err(FormEditError::Invalid {
            reason: "invalid_formatted_field",
            ..
        })
    ));
}

#[test]
fn dynamic_list_rules_preserve_other_scopes_and_dcs_settings() {
    let query = "<QueryText>SELECT Amount FROM Document.Test</QueryText>";
    let preferences = "<s:viewMode>QuickAccess</s:viewMode><s:userSettingID>keep</s:userSettingID>";
    let filter = "<s:filter><!--keep exactly--></s:filter>";
    let other = "<Attribute name='Other' id='2'><Type><v8:Type>cfg:DynamicList</v8:Type></Type></Attribute>";
    let settings = "http://v8.1c.ru/8.1/data-composition-system/settings";
    let input = source(&format!(
        r"
        <ChildItems><Table name='List' id='1'><DataPath>Rows</DataPath><ChildItems>
          <InputField name='RowAmount' id='2'><DataPath>Rows.Amount</DataPath></InputField>
        </ChildItems></Table></ChildItems>
        <Attributes xmlns:cfg='http://v8.1c.ru/8.1/data/enterprise/current-config' xmlns:v8='http://v8.1c.ru/8.1/data/core'
            xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance' xmlns:s='{settings}'>
          <Attribute name='Rows' id='1'><Type><v8:Type>cfg:DynamicList</v8:Type></Type>
            <Settings xsi:type='DynamicList'>{query}<ListSettings>{filter}
                <s:conditionalAppearance>{preferences}</s:conditionalAppearance>
            </ListSettings></Settings></Attribute>{other}
        </Attributes>"
    ));
    let rule = serde_json::json!({
        "fields":[{"name":"Amount","enabled":true}],
        "conditions":[{"kind":"comparison","enabled":true,"field":"Amount","operator":"InList",
            "operand":{"kind":"values","value":[
                {"kind":"number","value":"1.5"},
                {"kind":"string","value":"<tag>&"},
                {"kind":"date","value":"2026-11-16T14:20:00"}
            ]}}],
        "format":[{"parameter":"foreground","enabled":true,"value":{"kind":"color","value":"#123456"}}]
    });
    let op = |name: &str, rules| {
        serde_json::from_value(serde_json::json!({
            "kind":"setListConditionalAppearance","attribute":name,"rules":rules
        }))
        .unwrap()
    };
    let (edited, _) = plan(
        &input,
        &request(&input, vec![op("Rows", vec![rule.clone()])]),
    )
    .unwrap();
    let output = edited.output();
    for original in [query, preferences, filter, other] {
        assert!(output.contains(original));
    }
    assert!(output.contains("&lt;tag&gt;&amp;"));
    let schema = inspect(&form_id(), "Form.xml".into(), output, true).unwrap();
    assert_eq!(schema.dynamic_list_appearance[0].fields, ["Amount"]);
    assert_eq!(schema.dynamic_list_appearance[1].attribute, "Other");
    let saved = serde_json::to_value(
        &schema.dynamic_list_appearance[0]
            .appearance
            .as_ref()
            .unwrap()
            .rules,
    )
    .unwrap();
    assert_eq!(saved[0]["conditions"], rule["conditions"]);
    assert_eq!(output.matches("<dcsset:right ").count(), 3);
    let replay =
        serde_json::json!({"kind":"setListConditionalAppearance","attribute":"Rows","rules":saved});
    let (same, _) = plan(
        output,
        &request(output, vec![serde_json::from_value(replay).unwrap()]),
    )
    .unwrap();
    assert_eq!(same.output(), output);
    let (created, _) = plan(output, &request(output, vec![op("Other", vec![rule])])).unwrap();
    assert_eq!(
        inspect(&form_id(), "Form.xml".into(), created.output(), true)
            .unwrap()
            .dynamic_list_appearance[1]
            .appearance
            .as_ref()
            .unwrap()
            .rules
            .len(),
        1
    );
}
