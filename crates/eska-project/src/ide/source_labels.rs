//! Bounded, IO-free SCM captions using the same Designer XML path model as `eska diff`.

use std::path::{Component, Path};

use eska_presentation::metadata::render_metadata_source_path;
use eska_project_core::project::{
    ProjectType, metadata, metadata_model::NodeId, metadata_workspace::ProjectSession,
};
use serde_json::{Value, json};

use super::{dto, envelope::error, params};

/// Resolve relative source paths even when an added or removed file cannot be parsed.
pub(super) fn workspace_labels(
    labels: &dto::Labels,
    project: &ProjectSession,
    args: &Value,
) -> Result<Value, Value> {
    let project_type = project.project().configuration().project_type();
    let external_root = match project.root() {
        NodeId::Object(id)
            if matches!(project_type, ProjectType::Processing | ProjectType::Report) =>
        {
            project.object(id).ok().map(|root| root.name.as_str())
        }
        _ => None,
    };
    labels_for_paths(labels, project_type, external_root, args)
}

fn labels_for_paths(
    labels: &dto::Labels,
    project_type: ProjectType,
    external_root: Option<&str>,
    args: &Value,
) -> Result<Value, Value> {
    let paths: Vec<params::Path> = params::decode(&args["paths"])?;
    if paths.len() > 512 {
        return Err(error(-32602));
    }
    let paths = paths
        .iter()
        .map(params::Path::native)
        .collect::<Result<Vec<_>, _>>()?;
    let captions = paths
        .iter()
        .map(|path| {
            let normalized = source_path(path)?;
            let source = normalized.as_deref().and_then(|path| {
                metadata::from_source_path(project_type, path.as_bytes().into(), external_root)
            });
            Ok(json!({
                "path": dto::path(path),
                "translations": source.map(|source| json!({
                    "ru-RU": render_metadata_source_path(&source, &labels.ru),
                    "en-US": render_metadata_source_path(&source, &labels.en),
                })),
            }))
        })
        .collect::<Result<Vec<Value>, Value>>()?;
    Ok(json!({"labels": captions}))
}

/// Keep native separators separate from literal Unix backslashes and reject escaped roots.
fn source_path(path: &Path) -> Result<Option<String>, Value> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || cfg!(windows) && path.as_os_str().to_string_lossy().contains(':')
    {
        return Err(error(-32602));
    }
    Ok(path
        .iter()
        .map(|part| part.to_str())
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_the_descriptor_form_and_modules_without_reading_sources() {
        assert_captions(
            ProjectType::Configuration,
            &[
                (
                    "Configuration.xml",
                    "Конфигурация.Свойства",
                    "Configuration.Properties",
                ),
                (
                    "DataProcessors/ПросмотрОбъекта.xml",
                    "Обработка.ПросмотрОбъекта.Свойства",
                    "DataProcessor.ПросмотрОбъекта.Properties",
                ),
                (
                    "DataProcessors/ПросмотрОбъекта/Forms/Форма.xml",
                    "Обработка.ПросмотрОбъекта.Форма.Форма.Свойства",
                    "DataProcessor.ПросмотрОбъекта.Form.Форма.Properties",
                ),
                (
                    "DataProcessors/ПросмотрОбъекта/Forms/Форма/Ext/Form.xml",
                    "Обработка.ПросмотрОбъекта.Форма.Форма",
                    "DataProcessor.ПросмотрОбъекта.Form.Форма",
                ),
                (
                    "DataProcessors/ПросмотрОбъекта/Forms/Форма/Ext/Form/Module.bsl",
                    "Обработка.ПросмотрОбъекта.Форма.Форма.Модуль",
                    "DataProcessor.ПросмотрОбъекта.Form.Форма.Module",
                ),
                (
                    "CommonCommands/ПросмотрВоВнешнихСистемах.xml",
                    "ОбщаяКоманда.ПросмотрВоВнешнихСистемах.Свойства",
                    "CommonCommand.ПросмотрВоВнешнихСистемах.Properties",
                ),
                (
                    "CommonCommands/ПросмотрВоВнешнихСистемах/Ext/CommandModule.bsl",
                    "ОбщаяКоманда.ПросмотрВоВнешнихСистемах.МодульКоманды",
                    "CommonCommand.ПросмотрВоВнешнихСистемах.CommandModule",
                ),
                (
                    "Documents/ВыдачаЗайма/Ext/ObjectModule.bsl",
                    "Документ.ВыдачаЗайма.МодульОбъекта",
                    "Document.ВыдачаЗайма.ObjectModule",
                ),
                (
                    "Documents/ВыдачаЗайма/Commands/Печать/Ext/CommandModule.bsl",
                    "Документ.ВыдачаЗайма.Команда.Печать.МодульКоманды",
                    "Document.ВыдачаЗайма.Command.Печать.CommandModule",
                ),
                (
                    "CommonModules/ОбщийМодуль/Ext/Module.bsl",
                    "ОбщийМодуль.ОбщийМодуль.Модуль",
                    "CommonModule.ОбщийМодуль.Module",
                ),
                (
                    "CommonForms/Выбор.xml",
                    "ОбщаяФорма.Выбор.Свойства",
                    "CommonForm.Выбор.Properties",
                ),
                (
                    "CommonForms/Выбор/Ext/Form.bin",
                    "ОбщаяФорма.Выбор",
                    "CommonForm.Выбор",
                ),
                (
                    "CommonForms/Выбор/Ext/Form/Module.bin",
                    "ОбщаяФорма.Выбор.Модуль",
                    "CommonForm.Выбор.Module",
                ),
            ],
        );
    }

    #[test]
    fn distinguishes_templates_and_artifacts_within_the_same_object() {
        assert_captions(
            ProjectType::Extension,
            &[
                (
                    "Reports/Продажи/Templates/Печать.xml",
                    "Отчет.Продажи.Макет.Печать.Свойства",
                    "Report.Продажи.Template.Печать.Properties",
                ),
                (
                    "Reports/Продажи/Templates/Печать/Ext/Template.xml",
                    "Отчет.Продажи.Макет.Печать",
                    "Report.Продажи.Template.Печать",
                ),
                (
                    "CommonTemplates/Штамп/Ext/Template.txt",
                    "ОбщийМакет.Штамп",
                    "CommonTemplate.Штамп",
                ),
                (
                    "CommonTemplates/Штамп/Ext/Template/Items/Logo/Picture.png",
                    "ОбщийМакет.Штамп.Ресурс.Items/Logo/Picture.png",
                    "CommonTemplate.Штамп.Resource.Items/Logo/Picture.png",
                ),
                (
                    "Roles/Администратор/Ext/Rights.xml",
                    "Роль.Администратор.Права",
                    "Role.Администратор.Rights",
                ),
                (
                    "ScheduledJobs/Очистка/Ext/Schedule.xml",
                    "РегламентноеЗадание.Очистка.Расписание",
                    "ScheduledJob.Очистка.Schedule",
                ),
                (
                    "Catalogs/Товары/Ext/Predefined.xml",
                    "Справочник.Товары.ПредопределенныеДанные",
                    "Catalog.Товары.PredefinedData",
                ),
                (
                    "Subsystems/Раздел/Subsystems/Учет/Ext/CommandInterface.xml",
                    "Подсистема.Раздел.Подсистема.Учет.КомандныйИнтерфейс",
                    "Subsystem.Раздел.Subsystem.Учет.CommandInterface",
                ),
                (
                    "ExchangePlans/Обмен/Ext/Content.xml",
                    "ПланОбмена.Обмен.Состав",
                    "ExchangePlan.Обмен.Content",
                ),
                (
                    "BusinessProcesses/Задание/Ext/Flowchart.xml",
                    "БизнесПроцесс.Задание.КартаМаршрута",
                    "BusinessProcess.Задание.Flowchart",
                ),
                (
                    "CommonPictures/Логотип/Ext/Picture/Picture.png",
                    "ОбщаяКартинка.Логотип.Изображение.Picture.png",
                    "CommonPicture.Логотип.Picture.Picture.png",
                ),
                (
                    "CommonForms/Выбор/Ext/Help/_files/Picture.png",
                    "ОбщаяФорма.Выбор.Справка._files/Picture.png",
                    "CommonForm.Выбор.Help._files/Picture.png",
                ),
                (
                    "XDTOPackages/Обмен/Ext/Package.bin",
                    "ПакетXDTO.Обмен.Пакет",
                    "XDTOPackage.Обмен.Package",
                ),
                (
                    "WSReferences/Статистика/Ext/1.xsd",
                    "WSСсылка.Статистика.Схема.1.xsd",
                    "WSReference.Статистика.Schema.1.xsd",
                ),
                (
                    "Ext/MainSectionPicture/Picture.svg",
                    "Конфигурация.КартинкаОсновногоРаздела.Picture.svg",
                    "Configuration.MainSectionPicture.Picture.svg",
                ),
                (
                    "Ext/Splash.xml",
                    "Конфигурация.Заставка",
                    "Configuration.Splash",
                ),
                (
                    "Ext/ParentConfigurations/Base.cf",
                    "Конфигурация.РодительскиеКонфигурации.Base.cf",
                    "Configuration.ParentConfigurations.Base.cf",
                ),
                (
                    "Ext/MobileClientSignature.bin",
                    "Конфигурация.ПодписьМобильногоКлиента",
                    "Configuration.MobileClientSignature",
                ),
                (
                    "Catalogs/Товары/Ext/Unknown.xml",
                    "Справочник.Товары.Файл.Unknown.xml",
                    "Catalog.Товары.File.Unknown.xml",
                ),
            ],
        );
    }

    #[test]
    fn distinguishes_files_in_a_named_external_processing() {
        assert_captions(
            ProjectType::Processing,
            &[
                (
                    "Загрузка.xml",
                    "Обработка.Загрузка.Свойства",
                    "DataProcessor.Загрузка.Properties",
                ),
                (
                    "Загрузка/Forms/Выбор.xml",
                    "Обработка.Загрузка.Форма.Выбор.Свойства",
                    "DataProcessor.Загрузка.Form.Выбор.Properties",
                ),
                (
                    "Загрузка/Forms/Выбор/Ext/Form.xml",
                    "Обработка.Загрузка.Форма.Выбор",
                    "DataProcessor.Загрузка.Form.Выбор",
                ),
                (
                    "Загрузка/Forms/Выбор/Ext/Form/Module.bsl",
                    "Обработка.Загрузка.Форма.Выбор.Модуль",
                    "DataProcessor.Загрузка.Form.Выбор.Module",
                ),
            ],
        );
    }

    fn assert_captions(project_type: ProjectType, expected: &[(&str, &str, &str)]) {
        let paths: Vec<_> = expected
            .iter()
            .map(|(file, ..)| dto::path(Path::new(file)))
            .collect();
        let result = labels_for_paths(
            &dto::Labels::new().unwrap(),
            project_type,
            None,
            &json!({"paths": paths}),
        )
        .unwrap();
        let captions = result["labels"].as_array().unwrap();
        assert_eq!(captions.len(), expected.len());
        for (caption, &(file, ru, en)) in captions.iter().zip(expected) {
            assert_eq!(caption["path"], dto::path(Path::new(file)));
            assert_eq!(
                caption["translations"],
                json!({"ru-RU":ru,"en-US":en}),
                "{file}"
            );
        }
    }

    #[test]
    fn captions_reject_outside_paths_and_bound_batches() {
        let localizers = dto::Labels::new().unwrap();
        for path in ["", "../Documents/Loan.xml", "/Documents/Loan.xml"] {
            let args = json!({"paths": [dto::path(Path::new(path))]});
            assert!(
                labels_for_paths(&localizers, ProjectType::Configuration, None, &args).is_err()
            );
        }
        let oversized = vec![dto::path(Path::new("Configuration.xml")); 513];
        assert!(
            labels_for_paths(
                &localizers,
                ProjectType::Configuration,
                None,
                &json!({"paths": oversized})
            )
            .is_err()
        );
        assert_eq!(
            labels_for_paths(
                &localizers,
                ProjectType::Configuration,
                None,
                &json!({"paths": []})
            )
            .unwrap(),
            json!({"labels": []})
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_unix_names_do_not_become_windows_separators() {
        let path = Path::new("Documents/Имя\\Суффикс/Ext/ObjectModule.bsl");
        let result = labels_for_paths(
            &dto::Labels::new().unwrap(),
            ProjectType::Configuration,
            None,
            &json!({"paths": [dto::path(path)]}),
        )
        .unwrap();
        assert_eq!(
            result["labels"][0]["translations"]["ru-RU"],
            "Документ.Имя\\Суффикс.МодульОбъекта"
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_windows_separators_are_normalized() {
        let path = Path::new(r"Documents\Имя с пробелом\Ext\ObjectModule.bsl");
        let result = labels_for_paths(
            &dto::Labels::new().unwrap(),
            ProjectType::Configuration,
            None,
            &json!({"paths": [dto::path(path)]}),
        )
        .unwrap();
        assert_eq!(
            result["labels"][0]["translations"]["ru-RU"],
            "Документ.Имя с пробелом.МодульОбъекта"
        );
        for path in [
            r"C:\outside.xml",
            r"C:outside.xml",
            r"\\server\share\file.xml",
        ] {
            assert!(source_path(Path::new(path)).is_err());
        }
    }
}
