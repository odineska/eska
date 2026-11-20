//! Published query rules require real Designer metadata and preserve the analyzer's effective policy.

use std::{fs, path::Path, sync::atomic::AtomicBool, time::Duration};

use eska_analyzer::{
    check::{self, Severity},
    process::Analyzer,
};

use super::{prepare, support};

const HEADER: &str = r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses"
    xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.20">"#;
const MODULE: &str = r#"Процедура Проверить() Экспорт
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Партии.Партия КАК Партия
    |ИЗ РегистрНакопления.ПартииТоваров.Остатки(, Партия = &Партия) КАК Партии
    |УПОРЯДОЧИТЬ ПО Партии.Партия.Дата";
    Запрос.Выполнить();
КонецПроцедуры
"#;
const MANIFEST: &str = "[project]\ntype='configuration'\nsource='src'\n[tools.bsl-analyzer.platform_help]\nsource='bundled'\n";
const RULES: [&str; 2] = ["QueryConditionWithoutIndex", "QueryOrderByDocumentDate"];

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn descriptor(kind: &str, uuid: &str, name: &str, properties: &str, children: &str) -> String {
    format!(
        "{HEADER}<{kind} uuid=\"{uuid}\"><Properties><Name>{name}</Name>{properties}</Properties><ChildObjects>{children}</ChildObjects></{kind}></MetaDataObject>\n"
    )
}

fn fixture(root: &Path) {
    write(root, "eska.toml", MANIFEST);
    write(
        root,
        "src/Configuration.xml",
        &descriptor(
            "Configuration",
            "a022c3c3-e72f-4012-9a5a-d6207e0e34a3",
            "Пример",
            "<Version>1.0.0.0</Version>",
            "<Document>Поступление</Document><AccumulationRegister>ПартииТоваров</AccumulationRegister><CommonModule>Проверка</CommonModule>",
        ),
    );
    write(
        root,
        "src/Documents/Поступление.xml",
        &descriptor(
            "Document",
            "0454154a-6ab3-433c-bb50-82542fa4bd33",
            "Поступление",
            "",
            "",
        ),
    );
    let dimensions = r#"
<Dimension uuid="22222222-2222-2222-2222-222222222222"><Properties>
    <Name>Склад</Name><Type><v8:Type>xs:string</v8:Type></Type><Indexing>DontIndex</Indexing>
</Properties></Dimension>
<Dimension uuid="33333333-3333-3333-3333-333333333333"><Properties>
    <Name>Партия</Name><Type><v8:Type>cfg:DocumentRef.Поступление</v8:Type></Type><Indexing>DontIndex</Indexing>
</Properties></Dimension>"#;
    write(
        root,
        "src/AccumulationRegisters/ПартииТоваров.xml",
        &descriptor(
            "AccumulationRegister",
            "760a6f96-d4f7-4277-9787-bf7fa28153ad",
            "ПартииТоваров",
            "<RegisterType>Balance</RegisterType>",
            dimensions,
        ),
    );
    write(
        root,
        "src/CommonModules/Проверка.xml",
        &descriptor(
            "CommonModule",
            "c90d550f-ef4d-4ba4-ad2a-a306491d3f30",
            "Проверка",
            "<Server>true</Server><Global>false</Global>",
            "",
        ),
    );
    write(root, "src/CommonModules/Проверка/Ext/Module.bsl", MODULE);
}

/// Both v0.4.6 rules use actual register dimensions and document-reference types.
#[test]
#[ignore = "requires ESKA_BSL_ANALYZER pointing to bsl-analyzer-app 0.4.6 or later"]
fn v046_query_rules_and_metadata_overrides_are_effective() {
    let directory = support::TestDir::new();
    fixture(&directory.0);
    let cancelled = AtomicBool::new(false);
    let analyzer = Analyzer {
        binary: std::env::var_os("ESKA_BSL_ANALYZER")
            .expect("explicit analyzer")
            .into(),
        timeout: Duration::from_secs(120),
        cancelled: &cancelled,
        connection_closed: None,
    };
    let inspect = || {
        let plan = prepare(&directory.0, &analyzer);
        let report = check::analyze(&plan, &analyzer).unwrap();
        let [file]: [_; 1] = report.files.try_into().unwrap();
        let mut diagnostics = file
            .diagnostics
            .into_iter()
            .filter(|diagnostic| RULES.contains(&diagnostic.code.as_str()))
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| left.code.cmp(&right.code));
        diagnostics
    };
    let original = inspect();
    assert_eq!(
        original
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>(),
        RULES
    );
    for (diagnostic, range) in original.iter().zip([(3, 50, 3, 66), (4, 20, 4, 38)]) {
        assert_eq!(diagnostic.severity, Severity::Warning);
        assert_eq!(
            (
                diagnostic.start_line,
                diagnostic.start_column,
                diagnostic.end_line,
                diagnostic.end_column
            ),
            range
        );
    }
    write(
        &directory.0,
        "eska.toml",
        &format!(
            "{MANIFEST}\n[tools.bsl-analyzer.diagnostics.metadataOverrides.QueryOrderByDocumentDate]\nlspSeverity='hint'\n"
        ),
    );
    let overridden = inspect();
    assert_eq!(overridden[0], original[0]);
    let mut expected = original[1].clone();
    expected.severity = Severity::Hint;
    assert_eq!(overridden[1], expected);
    assert_eq!(
        serde_json::to_value(&overridden[1]).unwrap()["severity"],
        "Hint"
    );
    let fixed = MODULE
        .replace("Партия = &Партия", "Склад = &Склад И Партия = &Партия")
        .replace("Партии.Партия.Дата", "Партии.Партия.МоментВремени");
    write(
        &directory.0,
        "src/CommonModules/Проверка/Ext/Module.bsl",
        &fixed,
    );
    assert_eq!(inspect(), []);
}
