//! Stable BSL aliases for native standard metadata fields.

/// Standard spelling is shared across configurations; owner-dependent availability comes from the existing field provider.
pub fn standard_name(english: &str) -> Option<&'static str> {
    Some(match english {
        "Ref" => "Ссылка",
        "ThisObject" => "ЭтотОбъект",
        "DeletionMark" => "ПометкаУдаления",
        "Predefined" => "Предопределенный",
        "PredefinedDataName" => "ИмяПредопределенныхДанных",
        "Code" => "Код",
        "Description" => "Наименование",
        "Parent" => "Родитель",
        "IsFolder" => "ЭтоГруппа",
        "Owner" => "Владелец",
        "Date" => "Дата",
        "Number" => "Номер",
        "Posted" => "Проведен",
        "LineNumber" => "НомерСтроки",
        "Active" => "Активность",
        "Recorder" => "Регистратор",
        "Period" => "Период",
        "RecordType" => "ВидДвижения",
        "RegistrationPeriod" => "ПериодРегистрации",
        "Account" => "Счет",
        "PeriodAdjustment" => "УточнениеПериода",
        "Started" => "Стартован",
        "Completed" => "Завершен",
        "Executed" => "Выполнена",
        "Type" => "Вид",
        "OffBalance" => "Забалансовый",
        "Order" => "Порядок",
        "ValueType" => "ТипЗначения",
        _ => return None,
    })
}
