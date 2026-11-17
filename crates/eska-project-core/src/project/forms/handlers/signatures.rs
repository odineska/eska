//! Technical handler signatures from the 1C 8.3 managed-form syntax catalog.
//! Parameter identifiers describe the language API, not translated UI captions.

use super::super::ElementKind;

#[derive(Clone, Copy, Debug)]
pub(super) struct Signature {
    pub russian: &'static str,
    pub english: &'static str,
    pub russian_parameters: &'static str,
    pub english_parameters: &'static str,
}

pub(super) fn signature(kind: Option<ElementKind>, event: &str) -> Option<Signature> {
    let entries = match kind {
        None => {
            return (event == "Action").then_some(Signature {
                russian: "",
                english: "",
                russian_parameters: "Команда",
                english_parameters: "Command",
            });
        }
        Some(ElementKind::Form) => FORM,
        Some(ElementKind::InputField) => INPUT,
        Some(
            ElementKind::CheckBoxField
            | ElementKind::RadioButtonField
            | ElementKind::ProgressBarField
            | ElementKind::TrackBarField,
        ) => FIELD,
        Some(ElementKind::Table) => TABLE,
        Some(ElementKind::Pages) => PAGES,
        Some(ElementKind::LabelField) => LABEL,
        Some(ElementKind::PictureField) => PICTURE,
        Some(ElementKind::LabelDecoration) => LABEL_DECORATION,
        Some(ElementKind::PictureDecoration) => PICTURE_DECORATION,
        Some(ElementKind::CalendarField) => CALENDAR,
        Some(ElementKind::ChartField) => CHART,
        Some(ElementKind::DendrogramField) => DENDROGRAM,
        Some(ElementKind::FormattedDocumentField) => FORMATTED,
        Some(ElementKind::GanttChartField) => GANTT,
        Some(ElementKind::GeographicalSchemaField) => GEOGRAPHICAL,
        Some(ElementKind::GraphicalSchemaField) => GRAPHICAL,
        Some(ElementKind::HtmlDocumentField) => HTML,
        Some(ElementKind::PdfDocumentField) => PDF,
        Some(ElementKind::PeriodField) => PERIOD,
        Some(ElementKind::PlannerField) => PLANNER,
        Some(ElementKind::SpreadsheetDocumentField) => SPREADSHEET,
        Some(ElementKind::TextDocumentField) => TEXT_DOCUMENT,
        _ => return None,
    };
    entries
        .iter()
        .chain(
            if kind.is_some_and(|kind| kind.family() == super::super::ElementFamily::Field) {
                FIELD
            } else {
                &[]
            },
        )
        .find_map(
            |&(english, russian, russian_parameters, english_parameters)| {
                (english == event).then_some(Signature {
                    russian,
                    english,
                    russian_parameters,
                    english_parameters,
                })
            },
        )
}

type Entry = (&'static str, &'static str, &'static str, &'static str);

const FORM: &[Entry] = &[
    (
        "ActivationProcessing",
        "ОбработкаАктивизации",
        "АктивныйОбъект, Источник",
        "ActiveObject, Source",
    ),
    (
        "AddInDetachmentOnError",
        "ОтключениеВнешнейКомпонентыПриОшибке",
        "Имя, Местоположение",
        "Name, Location",
    ),
    (
        "AfterWrite",
        "ПослеЗаписи",
        "ПараметрыЗаписи",
        "WriteParameters",
    ),
    (
        "AfterWriteAtServer",
        "ПослеЗаписиНаСервере",
        "ТекущийОбъект, ПараметрыЗаписи",
        "CurrentObject, WriteParameters",
    ),
    (
        "BeforeClose",
        "ПередЗакрытием",
        "Отказ, ЗавершениеРаботы, ТекстПредупреждения, СтандартнаяОбработка",
        "Cancel, Exit, WarningText, StandardProcessing",
    ),
    (
        "BeforeLoadDataFromSettingsAtServer",
        "ПередЗагрузкойДанныхИзНастроекНаСервере",
        "Настройки",
        "Settings",
    ),
    (
        "BeforeReopenFromOtherServer",
        "ПередПереоткрытиемСДругогоСервера",
        "Параметры, Отказ",
        "Parameters, Cancel",
    ),
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Отказ, ПараметрыЗаписи",
        "Cancel, WriteParameters",
    ),
    (
        "BeforeWriteAtServer",
        "ПередЗаписьюНаСервере",
        "Отказ, ТекущийОбъект, ПараметрыЗаписи",
        "Cancel, CurrentObject, WriteParameters",
    ),
    (
        "ChoiceProcessing",
        "ОбработкаВыбора",
        "ВыбранноеЗначение, ИсточникВыбора",
        "SelectedValue, ChoiceSource",
    ),
    (
        "CollaborationSystemUsersAutoComplete",
        "АвтоПодборПользователейСистемыВзаимодействия",
        "Текст, ДанныеВыбора, НазначениеВыбора, СтандартнаяОбработка",
        "Text, ChoiceData, ChoicePurpose, StandardProcessing",
    ),
    (
        "CollaborationSystemUsersChoiceFormGetProcessing",
        "ОбработкаПолученияФормыВыбораПользователейСистемыВзаимодействия",
        "НазначениеВыбора, Параметры, ВыбраннаяФорма, СтандартнаяОбработка",
        "ChoicePurpose, Parameters, SelectedForm, StandardProcessing",
    ),
    (
        "ExternalEvent",
        "ВнешнееСобытие",
        "Источник, Событие, Данные",
        "Source, Event, Data",
    ),
    (
        "FillCheckProcessingAtServer",
        "ОбработкаПроверкиЗаполненияНаСервере",
        "Отказ, ПроверяемыеРеквизиты",
        "Cancel, CheckedAttributes",
    ),
    (
        "NavigationProcessing",
        "ОбработкаПерехода",
        "ОбъектПерехода, СтандартнаяОбработка",
        "NavigationObject, StandardProcessing",
    ),
    (
        "NewWriteProcessing",
        "ОбработкаЗаписиНового",
        "НовыйОбъект, Источник, СтандартнаяОбработка",
        "NewObject, Source, StandardProcessing",
    ),
    (
        "NotificationProcessing",
        "ОбработкаОповещения",
        "ИмяСобытия, Параметр, Источник",
        "EventName, Parameter, Source",
    ),
    (
        "OnChangeDisplaySettings",
        "ПриИзмененииПараметровЭкрана",
        "",
        "",
    ),
    ("OnClose", "ПриЗакрытии", "ЗавершениеРаботы", "Exit"),
    (
        "OnCreateAtServer",
        "ПриСозданииНаСервере",
        "Отказ, СтандартнаяОбработка",
        "Cancel, StandardProcessing",
    ),
    (
        "OnLoadDataFromSettingsAtServer",
        "ПриЗагрузкеДанныхИзНастроекНаСервере",
        "Настройки",
        "Settings",
    ),
    (
        "OnMainServerAvailabilityChange",
        "ПриИзмененииДоступностиОсновногоСервера",
        "СтандартнаяОбработка",
        "StandardProcessing",
    ),
    ("OnOpen", "ПриОткрытии", "Отказ", "Cancel"),
    (
        "OnPasteFromClipboard",
        "ПриВставкеИзБуфераОбмена",
        "Значение, СтандартнаяОбработка",
        "Value, StandardProcessing",
    ),
    (
        "OnReadAtServer",
        "ПриЧтенииНаСервере",
        "ТекущийОбъект",
        "CurrentObject",
    ),
    ("OnReopen", "ПриПовторномОткрытии", "", ""),
    (
        "OnReopenFromOtherServer",
        "ПриПереоткрытииСДругогоСервера",
        "Источник, ПараметрыЗаполнения, СтандартнаяОбработка",
        "Source, FillParameters, StandardProcessing",
    ),
    (
        "OnSaveDataInSettingsAtServer",
        "ПриСохраненииДанныхВНастройкахНаСервере",
        "Настройки",
        "Settings",
    ),
    (
        "OnWriteAtServer",
        "ПриЗаписиНаСервере",
        "Отказ, ТекущийОбъект, ПараметрыЗаписи",
        "Cancel, CurrentObject, WriteParameters",
    ),
    (
        "URLGetProcessing",
        "ОбработкаПолученияНавигационнойСсылки",
        "Ключ, Представление, НавигационнаяСсылка, СтандартнаяОбработка",
        "Key, Presentation, URL, StandardProcessing",
    ),
    (
        "URLListGetProcessing",
        "ОбработкаПолученияСпискаНавигационныхСсылок",
        "СписокНавигационыхСсылок, КлючПоУмолчанию",
        "URLList, DefaultKey",
    ),
    (
        "URLProcessing",
        "ОбработкаНавигационнойСсылки",
        "НавигационнаяСсылкаФорматированнойСтроки, СтандартнаяОбработка",
        "FormattedStringURL, StandardProcessing",
    ),
    (
        "ValueChoice",
        "ВыборЗначения",
        "СтандартнаяОбработка",
        "StandardProcessing",
    ),
    (
        "OnLoadVariantAtServer",
        "ПриЗагрузкеВариантаНаСервере",
        "Настройки",
        "Settings",
    ),
    (
        "BeforeLoadUserSettingsAtServer",
        "ПередЗагрузкойПользовательскихНастроекНаСервере",
        "Настройки, ИспользуютсяСтандартныеНастройки",
        "Settings, StandardSettingsUsed",
    ),
    (
        "BeforeLoadVariantAtServer",
        "ПередЗагрузкойВариантаНаСервере",
        "Настройки",
        "Settings",
    ),
    (
        "OnLoadUserSettingsAtServer",
        "ПриЗагрузкеПользовательскихНастроекНаСервере",
        "Настройки, ИспользуютсяСтандартныеНастройки",
        "Settings, StandardSettingsUsed",
    ),
    (
        "OnSaveVariantAtServer",
        "ПриСохраненииВариантаНаСервере",
        "Настройки",
        "Settings",
    ),
    (
        "OnUpdateUserSettingSetAtServer",
        "ПриОбновленииСоставаПользовательскихНастроекНаСервере",
        "СтандартнаяОбработка",
        "StandardProcessing",
    ),
    (
        "OnSaveUserSettingsAtServer",
        "ПриСохраненииПользовательскихНастроекНаСервере",
        "Настройки",
        "Settings",
    ),
];

const INPUT: &[Entry] = &[
    (
        "AutoComplete",
        "АвтоПодбор",
        "Элемент, Текст, ДанныеВыбора, ПараметрыПолученияДанных, Ожидание, СтандартнаяОбработка",
        "Item, Text, ChoiceData, DataGetParameters, Waiting, StandardProcessing",
    ),
    (
        "ChoiceProcessing",
        "ОбработкаВыбора",
        "Элемент, ВыбранноеЗначение, ДополнительныеДанные, ВыборДобавлением, СтандартнаяОбработка",
        "Item, ValueSelected, AdditionalData, ChoiceByAdding, StandardProcessing",
    ),
    (
        "Clearing",
        "Очистка",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "CommandGenerateProcessing",
        "ОбработкаФормированияКоманд",
        "Элемент, ПараметрыКоманд, Команды",
        "Item, CommandsParameters, Commands",
    ),
    (
        "Creating",
        "Создание",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "EditTextChange",
        "ИзменениеТекстаРедактирования",
        "Элемент, Текст, СтандартнаяОбработка",
        "Item, Text, StandardProcessing",
    ),
    (
        "MultipleValueOpening",
        "ОткрытиеМножественногоЗначения",
        "Элемент, Идентификатор, СтандартнаяОбработка",
        "Item, ID, StandardProcessing",
    ),
    (
        "MultipleValueURLProcessing",
        "ОбработкаНавигационнойСсылкиМножественногоЗначения",
        "Элемент, Идентификатор, НавигационнаяСсылкаФорматированнойСтроки, СтандартнаяОбработка",
        "Item, ID, FormattedStringURL, StandardProcessing",
    ),
    (
        "MultipleValuesAdd",
        "ДобавлениеМножественныхЗначений",
        "Элемент, Значения, СтандартнаяОбработка",
        "Item, Values, StandardProcessing",
    ),
    (
        "MultipleValuesDelete",
        "УдалениеМножественныхЗначений",
        "Элемент, Идентификаторы, СтандартнаяОбработка",
        "Item, IDs, StandardProcessing",
    ),
    ("OnChange", "ПриИзменении", "Элемент", "Item"),
    (
        "Opening",
        "Открытие",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "StartChoice",
        "НачалоВыбора",
        "Элемент, ДанныеВыбора, ВыборДобавлением, СтандартнаяОбработка",
        "Item, ChoiceData, ChoiceByAdding, StandardProcessing",
    ),
    (
        "StartListChoice",
        "НачалоВыбораИзСписка",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "TextEditEnd",
        "ОкончаниеВводаТекста",
        "Элемент, Текст, ДанныеВыбора, ПараметрыПолученияДанных, СтандартнаяОбработка",
        "Item, Text, ChoiceData, DataGetParameters, StandardProcessing",
    ),
    (
        "Tuning",
        "Регулирование",
        "Элемент, Направление, СтандартнаяОбработка",
        "Item, Direction, StandardProcessing",
    ),
];

const TABLE: &[Entry] = &[
    ("AfterDeleteRow", "ПослеУдаления", "Элемент", "Item"),
    (
        "BeforeAddRow",
        "ПередНачаломДобавления",
        "Элемент, Отказ, Копирование, Родитель, ЭтоГруппа, Параметр",
        "Item, Cancel, Clone, Parent, IsFolder, Parameter",
    ),
    (
        "BeforeCollapse",
        "ПередСворачиванием",
        "Элемент, Строка, Отказ",
        "Item, Row, Cancel",
    ),
    (
        "BeforeDeleteRow",
        "ПередУдалением",
        "Элемент, Отказ",
        "Item, Cancel",
    ),
    (
        "BeforeEditEnd",
        "ПередОкончаниемРедактирования",
        "Элемент, НоваяСтрока, ОтменаРедактирования, Отказ",
        "Item, NewRow, CancelEdit, Cancel",
    ),
    (
        "BeforeExpand",
        "ПередРазворачиванием",
        "Элемент, Строка, Отказ",
        "Item, Row, Cancel",
    ),
    (
        "BeforeLoadUserSettingsAtServer",
        "ПередЗагрузкойПользовательскихНастроекНаСервере",
        "Элемент, Настройки, ИспользуютсяСтандартныеНастройки",
        "Item, Settings, StandardSettingsUsed",
    ),
    (
        "BeforeRowChange",
        "ПередНачаломИзменения",
        "Элемент, Отказ",
        "Item, Cancel",
    ),
    (
        "ChoiceProcessing",
        "ОбработкаВыбора",
        "Элемент, ВыбранноеЗначение, СтандартнаяОбработка",
        "Item, ValueSelected, StandardProcessing",
    ),
    (
        "Drag",
        "Перетаскивание",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, Строка, Поле",
        "Item, DragParameters, StandardProcessing, Row, Field",
    ),
    (
        "DragCheck",
        "ПроверкаПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, Строка, Поле",
        "Item, DragParameters, StandardProcessing, Row, Field",
    ),
    (
        "DragEnd",
        "ОкончаниеПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragStart",
        "НачалоПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, Выполнение",
        "Item, DragParameters, Perform",
    ),
    (
        "NewWriteProcessing",
        "ОбработкаЗаписиНового",
        "Элемент, НовыйОбъект, Источник, СтандартнаяОбработка",
        "Item, NewObject, Source, StandardProcessing",
    ),
    ("OnActivateCell", "ПриАктивизацииЯчейки", "Элемент", "Item"),
    ("OnActivateField", "ПриАктивизацииПоля", "Элемент", "Item"),
    ("OnActivateRow", "ПриАктивизацииСтроки", "Элемент", "Item"),
    ("OnChange", "ПриИзменении", "Элемент", "Item"),
    (
        "OnCurrentParentChange",
        "ПриСменеТекущегоРодителя",
        "Элемент",
        "Item",
    ),
    (
        "OnEditEnd",
        "ПриОкончанииРедактирования",
        "Элемент, НоваяСтрока, ОтменаРедактирования",
        "Item, NewRow, CancelEdit",
    ),
    (
        "OnGetDataAtServer",
        "ПриПолученииДанныхНаСервере",
        "ИмяЭлемента, Настройки, Строки",
        "ItemName, Settings, Rows",
    ),
    (
        "OnLoadUserSettingsAtServer",
        "ПриЗагрузкеПользовательскихНастроекНаСервере",
        "Элемент, Настройки, ИспользуютсяСтандартныеНастройки",
        "Item, Settings, StandardSettingsUsed",
    ),
    (
        "OnSaveUserSettingsAtServer",
        "ПриСохраненииПользовательскихНастроекНаСервере",
        "Элемент, Настройки",
        "Item, Settings",
    ),
    (
        "OnStartEdit",
        "ПриНачалеРедактирования",
        "Элемент, НоваяСтрока, Копирование",
        "Item, NewRow, Clone",
    ),
    (
        "OnUpdateUserSettingSetAtServer",
        "ПриОбновленииСоставаПользовательскихНастроекНаСервере",
        "СтандартнаяОбработка",
        "StandardProcessing",
    ),
    (
        "RefreshRequestProcessing",
        "ОбработкаЗапросаОбновления",
        "Элемент",
        "Item",
    ),
    (
        "Selection",
        "Выбор",
        "Элемент, ВыбраннаяСтрока, Поле, СтандартнаяОбработка",
        "Item, RowSelected, Field, StandardProcessing",
    ),
    (
        "URLGetProcessing",
        "ОбработкаПолученияНавигационнойСсылки",
        "Элемент, Ключ, Представление, НавигационнаяСсылка, СтандартнаяОбработка",
        "Item, Key, Presentation, URL, StandardProcessing",
    ),
    (
        "URLListGetProcessing",
        "ОбработкаПолученияСпискаНавигационныхСсылок",
        "Элемент, СписокНавигационыхСсылок, КлючПоУмолчанию",
        "Item, URLList, DefaultKey",
    ),
    (
        "ValueChoice",
        "ВыборЗначения",
        "Элемент, Значение, СтандартнаяОбработка",
        "Item, Value, StandardProcessing",
    ),
];

const PAGES: &[Entry] = &[(
    "OnCurrentPageChange",
    "ПриСменеСтраницы",
    "Элемент, ТекущаяСтраница",
    "Item, CurrentPage",
)];

const LABEL: &[Entry] = &[
    (
        "Click",
        "Нажатие",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    ("OnChange", "ПриИзменении", "Элемент", "Item"),
    (
        "URLProcessing",
        "ОбработкаНавигационнойСсылки",
        "Элемент, НавигационнаяСсылкаФорматированнойСтроки, СтандартнаяОбработка",
        "Item, FormattedStringURL, StandardProcessing",
    ),
];

const PICTURE: &[Entry] = &[
    (
        "Click",
        "Нажатие",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "Drag",
        "Перетаскивание",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragCheck",
        "ПроверкаПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragEnd",
        "ОкончаниеПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragStart",
        "НачалоПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, Выполнение",
        "Item, DragParameters, Perform",
    ),
    ("OnChange", "ПриИзменении", "Элемент", "Item"),
];

const LABEL_DECORATION: &[Entry] = &[
    ("Click", "Нажатие", "Элемент", "Item"),
    (
        "URLProcessing",
        "ОбработкаНавигационнойСсылки",
        "Элемент, НавигационнаяСсылкаФорматированнойСтроки, СтандартнаяОбработка",
        "Item, FormattedStringURL, StandardProcessing",
    ),
];

const PICTURE_DECORATION: &[Entry] = &[
    ("Click", "Нажатие", "Элемент", "Item"),
    (
        "Drag",
        "Перетаскивание",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragCheck",
        "ПроверкаПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragEnd",
        "ОкончаниеПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "DragStart",
        "НачалоПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, Выполнение",
        "Item, DragParameters, Perform",
    ),
];

const FIELD: &[Entry] = &[("OnChange", "ПриИзменении", "Элемент", "Item")];

const CALENDAR: &[Entry] = &[
    (
        "DragStart",
        "НачалоПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, Выполнение",
        "Item, DragParameters, Perform",
    ),
    (
        "Selection",
        "Выбор",
        "Элемент, ВыбраннаяДата",
        "Item, SelectedDate",
    ),
    (
        "Drag",
        "Перетаскивание",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, ВыбраннаяДата",
        "Item, DragParameters, StandardProcessing, SelectedDate",
    ),
    (
        "DragEnd",
        "ОкончаниеПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    ("OnActivateDate", "ПриАктивизацииДаты", "Элемент", "Item"),
    (
        "OnPeriodOutput",
        "ПриВыводеПериода",
        "Элемент, ОформлениеПериода",
        "Item, PeriodAppearance",
    ),
    (
        "DragCheck",
        "ПроверкаПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, ВыбраннаяДата",
        "Item, DragParameters, StandardProcessing, SelectedDate",
    ),
];

const CHART: &[Entry] = &[
    ("OnActivate", "ПриАктивизации", "Элемент", "Item"),
    (
        "Selection",
        "Выбор",
        "Элемент, ЗначениеДиаграммы, СтандартнаяОбработка",
        "Item, ChartValue, StandardProcessing",
    ),
    (
        "DetailProcessing",
        "ОбработкаРасшифровки",
        "Элемент, Расшифровка, СтандартнаяОбработка",
        "Item, Details, StandardProcessing",
    ),
];

const DENDROGRAM: &[Entry] = &[
    (
        "Selection",
        "Выбор",
        "Элемент, ЗначениеДендрограммы, СтандартнаяОбработка",
        "Item, DendrogramValue, StandardProcessing",
    ),
    (
        "DetailProcessing",
        "ОбработкаРасшифровки",
        "Элемент, Расшифровка, СтандартнаяОбработка",
        "Item, Details, StandardProcessing",
    ),
];

const FORMATTED: &[Entry] = &[
    ("AfterWrite", "ПослеЗаписи", "Элемент", "Item"),
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Элемент, Копирование, Отказ",
        "Item, Clone, Cancel",
    ),
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
];

const GANTT: &[Entry] = &[
    (
        "Selection",
        "Выбор",
        "Элемент, Значения, СтандартнаяОбработка, Дата",
        "Item, Values, StandardProcessing, Date",
    ),
    (
        "OnIntervalEditEnd",
        "ПриОкончанииРедактированияИнтервала",
        "Элемент, Интервал, ОтменаРедактирования",
        "Item, Interval, CancelEdit",
    ),
    (
        "DetailProcessing",
        "ОбработкаРасшифровки",
        "Элемент, Расшифровка, СтандартнаяОбработка, Дата",
        "Item, Details, StandardProcessing, Date",
    ),
    (
        "BeforeCollapse",
        "ПередСворачиванием",
        "Элемент, ИдентификаторЗначения, Отказ",
        "Item, ValueID, Cancel",
    ),
    (
        "BeforeExpand",
        "ПередРазворачиванием",
        "Элемент, ИдентификаторЗначения, Отказ",
        "Item, ValueID, Cancel",
    ),
    (
        "OnActivateValue",
        "ПриАктивизацииЗначения",
        "Элемент",
        "Item",
    ),
    (
        "OnActivateInterval",
        "ПриАктивизацииИнтервала",
        "Элемент",
        "Item",
    ),
];

const GEOGRAPHICAL: &[Entry] = &[
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
    ("AfterWrite", "ПослеЗаписи", "Элемент", "Item"),
    (
        "DetailProcessing",
        "ОбработкаРасшифровки",
        "Элемент, Расшифровка, СтандартнаяОбработка",
        "Item, Details, StandardProcessing",
    ),
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Элемент, Копирование, Отказ",
        "Item, Clone, Cancel",
    ),
];

const GRAPHICAL: &[Entry] = &[
    ("Selection", "Выбор", "Элемент", "Item"),
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Элемент, Копирование, Отказ",
        "Item, Clone, Cancel",
    ),
    ("AfterWrite", "ПослеЗаписи", "Элемент", "Item"),
    ("OnActivate", "ПриАктивизации", "Элемент", "Item"),
];

const HTML: &[Entry] = &[
    ("DocumentComplete", "ДокументСформирован", "Элемент", "Item"),
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Элемент, Копирование, Отказ",
        "Item, Clone, Cancel",
    ),
    ("AfterWrite", "ПослеЗаписи", "Элемент", "Item"),
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
    (
        "OnClick",
        "ПриНажатии",
        "Элемент, ДанныеСобытия, СтандартнаяОбработка",
        "Item, EventData, StandardProcessing",
    ),
];

const PDF: &[Entry] = &[(
    "URLClick",
    "НажатиеНаНавигационнойСсылке",
    "Элемент, НавигационнаяСсылка, СтандартнаяОбработка",
    "Item, URL, StandardProcessing",
)];

const PERIOD: &[Entry] = &[(
    "Selection",
    "Выбор",
    "Элемент, ВыбраннаяДата",
    "Item, SelectedDate",
)];

const PLANNER: &[Entry] = &[
    (
        "WrappedTimeScaleHeaderClick",
        "НажатиеНаПеренесенномЗаголовкеШкалыВремени",
        "Элемент, Дата",
        "Item, Date",
    ),
    (
        "Selection",
        "Выбор",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "DragEnd",
        "ОкончаниеПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "URLClick",
        "НажатиеНаНавигационнойСсылке",
        "Элемент, Источник, НавигационнаяСсылкаФорматированнойСтроки, СтандартнаяОбработка",
        "Item, Source, FormattedStringURL, StandardProcessing",
    ),
    (
        "PlannerActionClick",
        "НажатиеНаДействиеПланировщика",
        "Элемент, ЭлементПланировщика, ДействиеЭлементаПланировщика",
        "Item, PlannerItem, Action",
    ),
    (
        "DimensionItemClick",
        "НажатиеНаЭлементеИзмерения",
        "Элемент, ЭлементИзмерения, ЗначенияИзмерений, СтандартнаяОбработка",
        "Item, DimensionItem, DimensionValues, StandardProcessing",
    ),
    (
        "TimeScaleItemClick",
        "НажатиеНаЭлементеШкалыВремени",
        "Элемент, ЭлементШкалыВремени, Дата",
        "Item, TimeScaleItem, Date",
    ),
    (
        "DragStart",
        "НачалоПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, Выполнение",
        "Item, DragParameters, Perform",
    ),
    (
        "CommandGenerateProcessing",
        "ОбработкаФормированияКоманд",
        "Элемент, ПараметрыКоманд, Команды, КомандаПоУмолчанию",
        "Item, CommandsParameters, Commands, DefaultCommand",
    ),
    (
        "BeforeStartQuickEdit",
        "ПередНачаломБыстрогоРедактирования",
        "Элемент, СтандартнаяОбработка",
        "Item, StandardProcessing",
    ),
    (
        "BeforeStartEdit",
        "ПередНачаломРедактирования",
        "Элемент, НовыйЭлемент, СтандартнаяОбработка",
        "Item, NewItem, StandardProcessing",
    ),
    (
        "OnCurrentRepresentationPeriodChange",
        "ПриСменеТекущегоПериодаОтображения",
        "Элемент, ТекущиеПериодыОтображения, СтандартнаяОбработка",
        "Item, CurrentRepresentationPeriods, StandardProcessing",
    ),
    (
        "InsideDragCheck",
        "ПроверкаПеретаскиванияВнутри",
        "Элемент, ПараметрыПеретаскивания, ВариантИзмененияГраниц, ДействиеПеретаскивания, Отказ",
        "Item, DragParameters, DragBoundsChangeVariant, DragAction, Cancel",
    ),
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
    (
        "BeforeExpandDimensionItem",
        "ПередРазворачиваниемЭлементаИзмерения",
        "Элемент, ЭлементИзмерения, ЗначенияИзмерений, Отказ",
        "Item, DimensionItem, DimensionValues, Cancel",
    ),
    (
        "BeforeCollapseDimensionItem",
        "ПередСворачиваниемЭлементаИзмерения",
        "Элемент, ЭлементИзмерения, ЗначенияИзмерений, Отказ",
        "Item, DimensionItem, DimensionValues, Cancel",
    ),
    (
        "BeforeCreate",
        "ПередСозданием",
        "Элемент, Начало, Конец, ЗначенияИзмерений, Текст, СтандартнаяОбработка",
        "Item, Begin, End, DimensionValues, Text, StandardProcessing",
    ),
    (
        "BeforeDelete",
        "ПередУдалением",
        "Элемент, Отказ",
        "Item, Cancel",
    ),
    (
        "OnEditEnd",
        "ПриОкончанииРедактирования",
        "Элемент, НовыйЭлемент, ОтменаРедактирования",
        "Item, NewItem, CancelEdit",
    ),
    (
        "Drag",
        "Перетаскивание",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, Дата, ЗначенияИзмерений",
        "Item, DragParameters, StandardProcessing, Date, DimensionValues",
    ),
    ("OnActivate", "ПриАктивизации", "Элемент", "Item"),
    (
        "DragCheck",
        "ПроверкаПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, Дата, ЗначенияИзмерений",
        "Item, DragParameters, StandardProcessing, Date, DimensionValues",
    ),
];

const SPREADSHEET: &[Entry] = &[
    (
        "Drag",
        "Перетаскивание",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, Область",
        "Item, DragParameters, StandardProcessing, Area",
    ),
    (
        "URLProcessing",
        "ОбработкаНавигационнойСсылки",
        "Элемент, НавигационнаяСсылкаФорматированнойСтроки, СтандартнаяОбработка",
        "Item, FormattedStringURL, StandardProcessing",
    ),
    (
        "AdditionalDetailProcessing",
        "ОбработкаДополнительнойРасшифровки",
        "Элемент, Расшифровка, СтандартнаяОбработка, ДополнительныеПараметры",
        "Item, Details, StandardProcessing, AdditionalParameters",
    ),
    (
        "Selection",
        "Выбор",
        "Элемент, Область, СтандартнаяОбработка",
        "Item, Area, StandardProcessing",
    ),
    (
        "DragStart",
        "НачалоПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, Выполнение",
        "Item, DragParameters, Perform",
    ),
    (
        "DetailProcessing",
        "ОбработкаРасшифровки",
        "Элемент, Расшифровка, СтандартнаяОбработка, ДополнительныеПараметры",
        "Item, Details, StandardProcessing, AdditionalParameters",
    ),
    (
        "DragEnd",
        "ОкончаниеПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка",
        "Item, DragParameters, StandardProcessing",
    ),
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Элемент, Копирование, Отказ",
        "Item, Clone, Cancel",
    ),
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
    ("OnActivate", "ПриАктивизации", "Элемент", "Item"),
    ("AfterWrite", "ПослеЗаписи", "Элемент", "Item"),
    (
        "OnChangeAreaContent",
        "ПриИзмененииСодержимогоОбласти",
        "Элемент, Область, ДополнительныеПараметры",
        "Item, Area, AdditionalParameters",
    ),
    (
        "DragCheck",
        "ПроверкаПеретаскивания",
        "Элемент, ПараметрыПеретаскивания, СтандартнаяОбработка, Область",
        "Item, DragParameters, StandardProcessing, Area",
    ),
];

const TEXT_DOCUMENT: &[Entry] = &[
    (
        "BeforeWrite",
        "ПередЗаписью",
        "Элемент, Копирование, Отказ",
        "Item, Clone, Cancel",
    ),
    (
        "BeforePrint",
        "ПередПечатью",
        "Элемент, ДиалогПечати, Отказ",
        "Item, PrintDialog, Cancel",
    ),
    ("AfterWrite", "ПослеЗаписи", "Элемент", "Item"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::forms::{AuthoringSource, authoring_properties};

    #[test]
    fn authoring_catalog_events_have_real_handler_signatures() {
        for kind in [
            ElementKind::Form,
            ElementKind::InputField,
            ElementKind::CheckBoxField,
            ElementKind::Table,
            ElementKind::Pages,
        ] {
            for property in authoring_properties(kind)
                .into_iter()
                .filter(|property| property.source == AuthoringSource::Event)
            {
                assert!(
                    signature(Some(kind), property.name).is_some(),
                    "{kind:?}.{}",
                    property.name
                );
            }
        }
        assert!(signature(Some(ElementKind::Form), "OnClientApplicationSuspend").is_none());
        assert!(signature(Some(ElementKind::Form), "OnClientApplicationResume").is_none());
    }

    #[test]
    fn signatures_are_identifiers_with_matching_bilingual_parameter_counts() {
        for entries in [
            FORM,
            INPUT,
            TABLE,
            PAGES,
            LABEL,
            PICTURE,
            LABEL_DECORATION,
            PICTURE_DECORATION,
            FIELD,
            CALENDAR,
            CHART,
            DENDROGRAM,
            FORMATTED,
            GANTT,
            GEOGRAPHICAL,
            GRAPHICAL,
            HTML,
            PDF,
            PERIOD,
            PLANNER,
            SPREADSHEET,
            TEXT_DOCUMENT,
        ] {
            let mut events = std::collections::HashSet::new();
            for &(english, russian, ru, en) in entries {
                assert!(events.insert(english), "{english}");
                assert!(super::super::identifier(english));
                assert!(super::super::identifier(russian));
                let arguments = |text: &str| {
                    text.split(',')
                        .filter(|part| !part.is_empty())
                        .map(str::trim)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                };
                let (ru, en) = (arguments(ru), arguments(en));
                assert_eq!(ru.len(), en.len(), "{english}");
                for values in [ru, en] {
                    let mut unique = std::collections::HashSet::new();
                    for name in values {
                        assert!(
                            super::super::identifier(&name) && unique.insert(name.to_lowercase()),
                            "{english}: {name}"
                        );
                    }
                }
            }
        }
    }
}
