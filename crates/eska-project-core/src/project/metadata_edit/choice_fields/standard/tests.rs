use super::*;
use crate::project::metadata_parser::{self, PropertiesMode};

fn descriptor(kind: &str, name: &str, properties: &str, children: &str) -> String {
    format!(
        "<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses'><{kind} uuid='00000000-0000-0000-0000-000000000001'><Properties><Name>{name}</Name>{properties}</Properties><ChildObjects>{children}</ChildObjects></{kind}></MetaDataObject>"
    )
}

#[test]
fn standard_attribute_catalog_covers_reference_owners() {
    let cases = [
        (
            "Document",
            "<NumberLength>9</NumberLength>",
            vec!["Ref", "DeletionMark", "Date", "Posted", "Number"],
        ),
        (
            "DocumentJournal",
            "",
            vec!["Ref", "Date", "Posted", "DeletionMark", "Number"],
        ),
        ("Enum", "", vec!["Ref", "Order"]),
        (
            "ExchangePlan",
            "",
            vec![
                "Ref",
                "DeletionMark",
                "Code",
                "Description",
                "SentNo",
                "ReceivedNo",
                "ThisNode",
                "ExchangeDate",
            ],
        ),
        (
            "BusinessProcess",
            "<NumberLength>9</NumberLength>",
            vec![
                "Ref",
                "DeletionMark",
                "Date",
                "Started",
                "Completed",
                "HeadTask",
                "Number",
            ],
        ),
        (
            "Task",
            "<NumberLength>9</NumberLength><DescriptionLength>30</DescriptionLength>",
            vec![
                "Ref",
                "DeletionMark",
                "Date",
                "Executed",
                "BusinessProcess",
                "RoutePoint",
                "Number",
                "Description",
            ],
        ),
    ];
    for (kind, properties, expected) in cases {
        check_fields(kind, properties, &expected);
    }
}

#[test]
fn standard_attribute_catalog_covers_register_conditions_and_non_data_owners() {
    let cases = [
        (
            "InformationRegister",
            "<WriteMode>Independent</WriteMode><InformationRegisterPeriodicity>Nonperiodical</InformationRegisterPeriodicity>",
            vec![],
        ),
        (
            "InformationRegister",
            "<WriteMode>RecorderSubordinate</WriteMode><InformationRegisterPeriodicity>Month</InformationRegisterPeriodicity>",
            vec!["Active", "LineNumber", "Recorder", "Period"],
        ),
        (
            "AccumulationRegister",
            "<RegisterType>Balance</RegisterType>",
            vec!["Active", "LineNumber", "Recorder", "Period", "RecordType"],
        ),
        (
            "AccountingRegister",
            "<Correspondence>false</Correspondence><ChartOfAccounts>ChartOfAccounts.Main</ChartOfAccounts><PeriodAdjustmentLength>1</PeriodAdjustmentLength>",
            vec![
                "Active",
                "LineNumber",
                "Recorder",
                "Period",
                "Account",
                "RecordType",
                "PeriodAdjustment",
            ],
        ),
        (
            "CalculationRegister",
            "",
            vec![
                "RegistrationPeriod",
                "Active",
                "LineNumber",
                "Recorder",
                "ReversingEntry",
            ],
        ),
        ("CommonModule", "", vec![]),
        ("Constant", "", vec![]),
    ];
    for (kind, properties, expected) in cases {
        check_fields(kind, properties, &expected);
    }
    assert_eq!(
        fields_for("TabularSection", "DontUse", &|_| None, false),
        ["LineNumber"]
    );
}

fn check_fields(kind: &str, properties: &str, expected: &[&str]) {
    let input = descriptor(kind, "Owner", properties, "");
    let document = roxmltree::Document::parse(&input).unwrap();
    let owner = document.root_element().first_element_child().unwrap();
    assert_eq!(fields(owner, "DontUse"), expected, "{kind}");
}

#[test]
fn metadata_catalog_preserves_disabled_native_standard_settings() {
    for (kind, properties, expected) in [
        (
            "Catalog",
            "<CodeLength>0</CodeLength><DescriptionLength>0</DescriptionLength><Hierarchical>false</Hierarchical><Owners/>",
            vec![
                "Ref",
                "DeletionMark",
                "Predefined",
                "PredefinedDataName",
                "Code",
                "Description",
                "Parent",
                "IsFolder",
                "Owner",
            ],
        ),
        (
            "InformationRegister",
            "<WriteMode>Independent</WriteMode><InformationRegisterPeriodicity>Nonperiodical</InformationRegisterPeriodicity>",
            vec!["Active", "LineNumber", "Recorder", "Period"],
        ),
        (
            "CalculationRegister",
            "<ActionPeriod>false</ActionPeriod><BasePeriod>false</BasePeriod>",
            vec![
                "RegistrationPeriod",
                "Active",
                "LineNumber",
                "Recorder",
                "CalculationType",
                "ActionPeriod",
                "BegOfActionPeriod",
                "EndOfActionPeriod",
                "BegOfBasePeriod",
                "EndOfBasePeriod",
                "ReversingEntry",
            ],
        ),
        (
            "AccountingRegister",
            "<Correspondence>false</Correspondence><PeriodAdjustmentLength>0</PeriodAdjustmentLength>",
            vec![
                "Active",
                "LineNumber",
                "Recorder",
                "Period",
                "Account",
                "RecordType",
            ],
        ),
        (
            "AccountingRegister",
            "<Correspondence>true</Correspondence><PeriodAdjustmentLength>1</PeriodAdjustmentLength>",
            vec![
                "Active",
                "LineNumber",
                "Recorder",
                "Period",
                "Account",
                "PeriodAdjustment",
            ],
        ),
    ] {
        let parsed = metadata_parser::parse(
            &descriptor(kind, "Owner", properties, ""),
            None,
            PropertiesMode::All,
        )
        .unwrap();
        assert_eq!(
            parsed_fields(&parsed.objects[0], "DontUse"),
            expected,
            "{kind}"
        );
    }
}

#[test]
fn extra_dimension_count_matches_native_editor_bounds() {
    for value in [0, 1, 3, 50] {
        assert_eq!(extra_dimension_count(&value.to_string()), Some(value));
    }
    for value in ["51", "-1", "", "4294967296", "abc"] {
        assert_eq!(extra_dimension_count(value), None);
    }
}

#[test]
fn metadata_catalog_respects_register_display_rules_and_non_data_owners() {
    for (kind, properties, expected) in [
        (
            "AccumulationRegister",
            "<RegisterType>Balance</RegisterType>",
            vec!["Active", "LineNumber", "Recorder", "Period", "RecordType"],
        ),
        (
            "AccumulationRegister",
            "<RegisterType>Turnovers</RegisterType>",
            vec!["Active", "LineNumber", "Recorder", "Period"],
        ),
        ("FilterCriterion", "", vec![]),
        ("Bot", "", vec![]),
        ("WebSocketClient", "", vec![]),
    ] {
        let parsed = metadata_parser::parse(
            &descriptor(kind, "Owner", properties, ""),
            None,
            PropertiesMode::All,
        )
        .unwrap();
        assert_eq!(
            parsed_fields(&parsed.objects[0], "DontUse"),
            expected,
            "{kind}"
        );
    }
}

#[test]
fn metadata_chart_attributes_include_latent_code_order_hierarchy_and_action_settings() {
    for (kind, expected) in [
        (
            "ChartOfAccounts",
            vec![
                "Ref",
                "DeletionMark",
                "Predefined",
                "PredefinedDataName",
                "Code",
                "Description",
                "Parent",
                "Type",
                "OffBalance",
                "Order",
            ],
        ),
        (
            "ChartOfCalculationTypes",
            vec![
                "Ref",
                "DeletionMark",
                "Predefined",
                "PredefinedDataName",
                "Code",
                "Description",
                "ActionPeriodIsBasic",
            ],
        ),
        (
            "ChartOfCharacteristicTypes",
            vec![
                "Ref",
                "DeletionMark",
                "Predefined",
                "PredefinedDataName",
                "Code",
                "Description",
                "ValueType",
                "Parent",
                "IsFolder",
            ],
        ),
    ] {
        let parsed = metadata_parser::parse(&descriptor(kind, "Owner", "<CodeLength>0</CodeLength><DescriptionLength>0</DescriptionLength><OrderLength>0</OrderLength><Hierarchical>false</Hierarchical><ActionPeriodUse>false</ActionPeriodUse>", ""), None, PropertiesMode::All).unwrap();
        assert_eq!(
            parsed_fields(&parsed.objects[0], "DontUse"),
            expected,
            "{kind}"
        );
    }
}

#[test]
fn standard_attribute_types_preserve_owner_references_and_platform_namespaces() {
    use crate::project::forms::{FieldValueKind, standard_type_for};
    let config = "http://v8.1c.ru/8.1/data/enterprise/current-config";
    let enterprise = "http://v8.1c.ru/8.1/data/enterprise";
    for (kind, name, expected) in [
        ("DocumentJournal", "Ref", (config, "DocumentRef")),
        ("Catalog", "Parent", (config, "CatalogRef.Owner")),
        ("Task", "BusinessProcess", (config, "BusinessProcessRef")),
        (
            "Task",
            "RoutePoint",
            (config, "BusinessProcessRoutePointRef"),
        ),
        ("BusinessProcess", "HeadTask", (config, "TaskRef")),
        ("AccumulationRegister", "Recorder", (config, "DocumentRef")),
        (
            "AccumulationRegister",
            "RecordType",
            (enterprise, "AccumulationRecordType"),
        ),
        (
            "AccountingRegister",
            "RecordType",
            (enterprise, "AccountingRecordType"),
        ),
        ("ChartOfAccounts", "Type", (enterprise, "AccountType")),
    ] {
        let parsed = metadata_parser::parse(
            &descriptor(kind, "Owner", "", ""),
            None,
            PropertiesMode::All,
        )
        .unwrap();
        let value = standard_type_for(name, &parsed.objects[0], &|_| None);
        assert_eq!(value.types.len(), 1, "{kind}.{name}");
        assert_eq!(
            (
                value.types[0].namespace.as_deref().unwrap(),
                value.types[0].name.as_str()
            ),
            expected,
            "{kind}.{name}"
        );
    }
    for (kind, name, expected) in [
        ("Enum", "Order", FieldValueKind::Number),
        ("ExchangePlan", "ExchangeDate", FieldValueKind::Date),
        ("Document", "Posted", FieldValueKind::Boolean),
    ] {
        let parsed = metadata_parser::parse(
            &descriptor(kind, "Owner", "", ""),
            None,
            PropertiesMode::All,
        )
        .unwrap();
        assert_eq!(
            standard_type_for(name, &parsed.objects[0], &|_| None).kind,
            expected,
            "{kind}.{name}"
        );
    }
}

#[test]
fn standard_attribute_types_reuse_direct_owner_links_and_native_qualifiers() {
    use crate::project::forms::{NumberSign, standard_type_for};
    for (kind, name, properties, expected) in [
        (
            "DocumentJournal",
            "Ref",
            "<RegisteredDocuments xmlns:r='http://v8.1c.ru/8.3/xcf/readable' xmlns:o='urn:foreign'><r:Item>Document.Invoice</r:Item><o:Item>Document.Foreign</o:Item><r:Item>Document.Payment</r:Item></RegisteredDocuments>",
            vec!["DocumentRef.Invoice", "DocumentRef.Payment"],
        ),
        (
            "BusinessProcess",
            "HeadTask",
            "<Task>Task.Approval</Task>",
            vec!["TaskRef.Approval"],
        ),
        (
            "CalculationRegister",
            "CalculationType",
            "<ChartOfCalculationTypes>ChartOfCalculationTypes.Payroll</ChartOfCalculationTypes>",
            vec!["ChartOfCalculationTypesRef.Payroll"],
        ),
    ] {
        let parsed = metadata_parser::parse(
            &descriptor(kind, "Owner", properties, ""),
            None,
            PropertiesMode::All,
        )
        .unwrap();
        let object = &parsed.objects[0];
        let value = standard_type_for(name, object, &|key| property(object, key));
        assert_eq!(
            value
                .types
                .iter()
                .map(|key| key.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let parsed = metadata_parser::parse(
        &descriptor(
            "AccountingRegister",
            "Owner",
            "<PeriodAdjustmentLength>2</PeriodAdjustmentLength>",
            "",
        ),
        None,
        PropertiesMode::All,
    )
    .unwrap();
    let object = &parsed.objects[0];
    for (name, digits) in [
        ("LineNumber", 9),
        ("SentNo", 10),
        ("ReceivedNo", 10),
        ("PeriodAdjustment", 2),
    ] {
        let value = standard_type_for(name, object, &|key| property(object, key));
        assert_eq!(value.digits, Some(digits));
        assert_eq!(value.fraction, Some(0));
        assert_eq!(value.allowed_sign, Some(NumberSign::Nonnegative));
    }
    assert_eq!(
        standard_type_for("PredefinedDataName", object, &|_| None).length,
        Some(256)
    );
}
