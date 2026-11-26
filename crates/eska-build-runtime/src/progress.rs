//! Sanitized native activity. No native text or process arguments cross this boundary.
use crate::build::ProcessStream;

/// Allowlisted read-only platform operations selected from owned runtime commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgressCommand {
    ReadGeneration,
    ChecksumMain,
    ChecksumDatabase,
    ListExtensions,
    ExportConfiguration,
    SaveConfiguration,
    ReadExtensionInfo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Signal {
    CommandStarted(ProgressCommand),
    Activity,
    ObjectAdded(String),
    ObjectModified(String),
    ObjectRestructuring(String),
    DatabaseFileChanged,
    Reading,
    Importing,
    CheckingMetadata,
    Restructuring,
    GatheringInformation,
    AcceptingChanges,
    UpdatingDatabase,
    DynamicRestructuring,
    GenerationCreated,
    DatabaseUpdated,
    BuildingHelpIndex,
    HelpIndexBuilt,
}

/// Match complete, known diagnostic records only. A completion record describes
/// native output; only the process exit status establishes operation success.
#[must_use]
pub fn classify(bytes: &[u8]) -> Option<Signal> {
    let line = std::str::from_utf8(bytes)
        .ok()?
        .trim_end_matches(['\r', '\n']);
    let text = line.strip_prefix("[INFO] ")?;
    Some(match text {
        "Обновление конфигурации базы данных..."
        | "Database configuration update..."
        | "Updating database configuration..." => Signal::UpdatingDatabase,
        "Проверка корректности метаданных..." | "Validating metadata..." => {
            Signal::CheckingMetadata
        }
        "Обработка структуры базы данных..."
        | "Process database structure..."
        | "Processing data" => Signal::Restructuring,
        "Сбор служебной информации..." | "Collecting service information..." => {
            Signal::GatheringInformation
        }
        "Принятие изменений..." | "Accepting changes..." => {
            Signal::AcceptingChanges
        }
        "Изменен режим использования динамической реструктуризации данных"
        | "The mode of using dynamic data restructuring is changed" => Signal::DynamicRestructuring,
        "Обновление конфигурации базы данных успешно завершено"
        | "Database configuration update completed."
        | "Database configuration successfully updated" => Signal::DatabaseUpdated,
        "Построение индекса справки..." | "Building help index..." => {
            Signal::BuildingHelpIndex
        }
        "Построение индекса справки успешно завершено" | "Help index is built" => {
            Signal::HelpIndexBuilt
        }
        "Чтение конфигурации..." => Signal::Reading,
        "Загрузка конфигурации..."
        | "Importing configuration..."
        | "Importing configuration from XML..."
        | "Importing configuration files from XML..." => Signal::Importing,
        _ => {
            if let Some(object) = text
                .strip_prefix("New object: ")
                .or_else(|| text.strip_prefix("Новый объект: "))
            {
                return qualified_object(object)
                    .map(|object| Signal::ObjectAdded(object.to_owned()));
            }
            if let Some(object) = text
                .strip_prefix("Object modified: ")
                .or_else(|| text.strip_prefix("Объект изменен: "))
            {
                return qualified_object(object)
                    .map(|object| Signal::ObjectModified(object.to_owned()));
            }
            if let Some(object) = text.strip_prefix("Processing data Restructuring ") {
                return qualified_object(object)
                    .map(|object| Signal::ObjectRestructuring(object.to_owned()));
            }
            if let Some(object) = text.strip_prefix("Обработка данных Реструктуризация ")
            {
                return qualified_object(object)
                    .map(|object| Signal::ObjectRestructuring(object.to_owned()));
            }
            let generation = [
                "Создано поколение конфигурации:",
                "Создано поколение расширения конфигурации:",
                "Configuration generation is created:",
                "Created configuration generation:",
                "Configuration extension generation is created:",
            ]
            .iter()
            .find_map(|prefix| text.strip_prefix(prefix))?;
            crate::build::ibcmd_generation::parse_id(generation)?;
            Signal::GenerationCreated
        }
    })
}

fn qualified_object(object: &str) -> Option<&str> {
    if object.len() > 512 {
        return None;
    }
    let object = object.trim_matches(|character: char| character.is_ascii_whitespace());
    let (kind, name) = object.split_once('.')?;
    if !matches!(
        kind,
        "Справочник"
            | "Catalog"
            | "Документ"
            | "Document"
            | "РегистрСведений"
            | "InformationRegister"
            | "РегистрНакопления"
            | "AccumulationRegister"
            | "РегистрБухгалтерии"
            | "AccountingRegister"
            | "РегистрРасчета"
            | "CalculationRegister"
            | "Constant"
            | "Константа"
            | "Enum"
            | "Перечисление"
            | "ChartOfAccounts"
            | "ПланСчетов"
            | "ChartOfCharacteristicTypes"
            | "ПланВидовХарактеристик"
            | "ChartOfCalculationTypes"
            | "ПланВидовРасчета"
            | "ExchangePlan"
            | "ПланОбмена"
            | "BusinessProcess"
            | "БизнесПроцесс"
            | "Task"
            | "Задача"
            | "DocumentJournal"
            | "ЖурналДокументов"
            | "Sequence"
            | "Последовательность"
    ) || name.chars().count() > 128
    {
        return None;
    }
    let mut characters = name.chars();
    let first = characters.next()?;
    if !(first.is_alphabetic() || first == '_')
        || !characters.all(|character| character.is_alphanumeric() || character == '_')
    {
        return None;
    }
    Some(object)
}

/// Decode independent streams, including split UTF-8, CR records and bounded lines.
#[derive(Default)]
pub struct Decoder {
    streams: [(Vec<u8>, bool); 2],
}

impl Decoder {
    pub fn push(
        &mut self,
        stream: ProcessStream,
        bytes: &[u8],
        emit: &mut impl FnMut(Option<Signal>),
    ) {
        let (line, discarded) = &mut self.streams[match stream {
            ProcessStream::Stdout => 0,
            ProcessStream::Stderr => 1,
        }];
        for &byte in bytes {
            if matches!(byte, b'\r' | b'\n') {
                if *discarded {
                    emit(None);
                } else if !line.is_empty() {
                    emit(classify(line));
                }
                line.clear();
                *discarded = false;
            } else if !*discarded {
                if line.len() == 16 * 1024 {
                    line.clear();
                    *discarded = true;
                } else {
                    line.push(byte);
                }
            }
        }
    }

    pub fn finish(&mut self, emit: &mut impl FnMut(Option<Signal>)) {
        for (line, discarded) in &mut self.streams {
            if *discarded {
                emit(None);
            } else if !line.is_empty() {
                emit(classify(line));
            }
            line.clear();
            *discarded = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_object_names_require_the_verified_template_and_safe_qualified_identifier() {
        for line in [
            "[INFO] New object: Справочник.NativeGoods",
            "[INFO] Новый объект: Справочник.NativeGoods",
        ] {
            assert_eq!(
                classify(line.as_bytes()),
                Some(Signal::ObjectAdded("Справочник.NativeGoods".to_owned()))
            );
        }
        assert_eq!(
            classify("[INFO] Object modified: Справочник.NativeGoods".as_bytes()),
            Some(Signal::ObjectModified("Справочник.NativeGoods".to_owned()))
        );
        assert_eq!(
            classify("[INFO] Processing data Restructuring Справочник.NativeGoods".as_bytes()),
            Some(Signal::ObjectRestructuring(
                "Справочник.NativeGoods".to_owned()
            ))
        );
        assert_eq!(
            classify("[INFO] Обработка данных Реструктуризация Справочник.NativeGoods ".as_bytes()),
            Some(Signal::ObjectRestructuring(
                "Справочник.NativeGoods".to_owned()
            ))
        );
        assert_eq!(
            classify("[INFO] Объект изменен: Справочник.NativeGoods".as_bytes()),
            Some(Signal::ObjectModified("Справочник.NativeGoods".to_owned()))
        );
        for object in [
            "Документ.Продажа",
            "Document.Sale",
            "InformationRegister.Settings",
            "AccumulationRegister.Stock",
            "AccountingRegister.Ledger",
            "CalculationRegister.Payroll",
            "БизнесПроцесс.Заказ",
            "ChartOfCharacteristicTypes.Properties",
            "Constant.Currency",
            "ЖурналДокументов.Продажи",
            "РегистрСведений.Параметры",
            "РегистрНакопления.Остатки",
            "РегистрБухгалтерии.Учет",
            "РегистрРасчета.Зарплата",
        ] {
            assert_eq!(
                classify(format!("[INFO] Object modified: {object} ").as_bytes()),
                Some(Signal::ObjectModified(object.to_owned()))
            );
        }
        assert_eq!(
            classify("[INFO] Processing data Restructuring Справочник.NativeGoods ".as_bytes()),
            Some(Signal::ObjectRestructuring(
                "Справочник.NativeGoods".to_owned()
            ))
        );
        assert_eq!(
            classify(b"[INFO] New object: Catalog.Goods"),
            Some(Signal::ObjectAdded("Catalog.Goods".to_owned()))
        );
        for object in [
            "Catalog.Secret --password=x",
            "Catalog.Name.Other",
            "Unknown.Name",
            "Catalog.12Name",
            "Catalog.",
            "Catalog.Name/x",
            "Catalog.Name\nsecret",
        ] {
            assert_eq!(
                classify(format!("[INFO] New object: {object}").as_bytes()),
                None
            );
        }
        assert_eq!(
            classify(format!("[INFO] New object: Catalog.{}", "x".repeat(129)).as_bytes()),
            None
        );
    }

    #[test]
    fn recognizes_native_854_restructuring_without_leaking_generation() {
        let expected = [
            (
                "Updating database configuration...",
                Signal::UpdatingDatabase,
            ),
            ("Validating metadata...", Signal::CheckingMetadata),
            ("Process database structure...", Signal::Restructuring),
            (
                "Collecting service information...",
                Signal::GatheringInformation,
            ),
            (
                "The mode of using dynamic data restructuring is changed",
                Signal::DynamicRestructuring,
            ),
            ("Accepting changes...", Signal::AcceptingChanges),
            (
                "Created configuration generation: 927ce8a6f269f91d1e9562db25ba27ff74ead2aa",
                Signal::GenerationCreated,
            ),
            (
                "Database configuration successfully updated",
                Signal::DatabaseUpdated,
            ),
        ];
        for (text, expected) in expected {
            assert_eq!(
                classify(format!("[INFO] {text}").as_bytes()),
                Some(expected)
            );
        }
        assert_eq!(
            classify(b"[INFO] Created configuration generation: password=secret"),
            None
        );
    }

    #[test]
    fn secrets_and_lookalikes_never_become_progress_text() {
        assert_eq!(classify(b"[INFO] password=secret"), None);
        assert_eq!(
            classify("[INFO] Принятие изменений... secret".as_bytes()),
            None
        );
        assert_eq!(
            classify("[INFO] Принятие изменений...".as_bytes()),
            Some(Signal::AcceptingChanges)
        );
        assert_eq!(
            classify(b"[INFO] Accepting changes..."),
            Some(Signal::AcceptingChanges)
        );
    }
    #[test]
    fn streams_split_unicode_and_cr_are_independent() {
        let mut decoder = Decoder::default();
        let text = "[INFO] Принятие изменений...\r".as_bytes();
        let mut actual = Vec::new();
        decoder.push(ProcessStream::Stdout, &text[..9], &mut |s| actual.push(s));
        decoder.push(ProcessStream::Stderr, b"secret\r", &mut |s| actual.push(s));
        decoder.push(ProcessStream::Stdout, &text[9..], &mut |s| actual.push(s));
        decoder.finish(&mut |s| actual.push(s));
        assert_eq!(actual, [None, Some(Signal::AcceptingChanges)]);
    }
    #[test]
    fn oversized_records_cannot_spoof_a_suffix() {
        let mut decoder = Decoder::default();
        let mut actual = Vec::new();
        decoder.push(ProcessStream::Stdout, &vec![b'x'; 100_000], &mut |s| {
            actual.push(s);
        });
        decoder.push(
            ProcessStream::Stdout,
            b"[INFO] Accepting changes...\n",
            &mut |s| actual.push(s),
        );
        assert!(actual.iter().all(Option::is_none));
        assert_eq!(actual.len(), 1);
        assert!(decoder.streams[0].0.len() <= 16 * 1024);
    }
}
