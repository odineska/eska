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
            vec!["Type", "Ref", "Date", "Posted", "DeletionMark", "Number"],
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
            vec!["RegistrationPeriod", "Active", "LineNumber", "Recorder"],
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
