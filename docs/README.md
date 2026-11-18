# Документация ESKA

**ESKA в активной разработке.** В [руководстве](user-guide.md) описаны текущие команды и редакторы. [Документация и скринкасты на сайте](https://odineska.github.io/eska/docs/).

## Пользовательские возможности

| Задача | Где читать |
| --- | --- |
| Понять состав и ограничения продукта | [Возможности](user-guide.md#capabilities), [компоненты](user-guide.md#components) |
| Установить CLI, Explorer и анализатор | [Установка](user-guide.md#installation), [поставка компонентов](component-installation.md) |
| Создать проект или подключить XML | [Первый проект](user-guide.md#project), [eska-project](project-cli.md) |
| Настроить workspace, приложение и зависимости | [Приложения](user-guide.md#workspace), [точный контракт](application-context.md) |
| Подключить БСП к EPF/ERF | [Библиотеки](user-guide.md#add-library), [сборка с основой](build-tool.md) |
| Использовать дерево, поиск и файловые группы | [Explorer](user-guide.md#explorer), [модель дерева](configurator-tree.md), [поиск](metadata-search.md) |
| Отключить подпроекты и скрыть их | [Активность](user-guide.md#activity) |
| Изменить свойство, тип или имя объекта | [Свойства](user-guide.md#properties), [CLI и формат запросов](metadata-editing.md) |
| Добавить реквизит или табличную часть через CLI | [Интерактивный мастер, JSON и планы создания](metadata-creation.md) |
| Создать конфигурацию, копировать объекты, изменить форму через CLI | [Интерфейс для человека и ИИ](metadata-authoring.md) |
| Разобраться с поддержкой и read-only | [Правила поддержки](support-policy.md), [объяснение пользователю](user-guide.md#support) |
| Редактировать запрос XML или BSL | [Редактор запросов](user-guide.md#queries), [API сессий](ide-protocol.md) |
| Редактировать форму и её обработчики | [Редактор управляемых форм](user-guide.md#forms), [команды для формы](metadata-authoring.md#изменение-формы) |
| Работать с изменениями объектов в VS Code | [Панель Git](user-guide.md#scm) |
| Настроить базу, обновить приложение или подключить отладчик | [Запуск и отладка](run-debug.md), [отдельный DAP-адаптер](eska-debug-standalone.md) |
| Использовать конструктор запросов | [Полный набор операций](user-guide.md#constructor) |
| Подключить BSL-сервисы | [Языковой сервис](user-guide.md#language), [производный config](analyzer-configuration.md) |
| Настроить форматирование | [Пользовательский маршрут](user-guide.md#formatting), [все параметры](formatting.md), [JSON Schema](../schemas/format-v1.schema.json) |
| Запускать проверки в CI | [Проверка](user-guide.md#check), [eska-check и JSON](check-cli.md) |
| Работать с ветками, diff, commits и полками | [Git-workflow](user-guide.md#daily-work), [eska-vcs](vcs-tool.md) |
| Найти платформу, собрать, распаковать, импортировать | [Сборка](user-guide.md#build), [eska-build](build-tool.md), [платформы](platform-catalog.md) |
| Создать патч-расширение | [Ограниченный patch](user-guide.md#patch), [точные условия](roadmap/t42-patch-extension.md) |
| Менять версию проекта | [Версия 1С](user-guide.md#version) |
| Обновиться, работать без сети, удалить установку | [Обслуживание](user-guide.md#updates), [eska update](cli-update.md) |
| Посмотреть любой публичный CLI-вход | [Полный справочник команд](cli-reference.md) |
| Найти настройку или команду редактора | [Настройки и действия Explorer](user-guide.md#settings) |
| Решить проблему | [Диагностика](user-guide.md#troubleshooting) |

## Для интеграций и разработки

- [IDE protocol 1.x](ide-protocol.md): framing, capabilities, DTO, запросы, события,
  версии, защита, query-сессии, форматирование и активность workspace.
- [Композиция компонентов](component-delegation.md), [архитектура](architecture.md),
  [модульность](tooling-modularity.md), [проверка установки](component-installation.md).
- [Ревью ESKA и Explorer от 18 ноября 2026](reviews/2026-11-18.md).
- [Источники](metadata-sources.md), [парсер](metadata-parser.md),
  [модель](metadata-model.md), [workspace](metadata-workspace.md), [кеш](metadata-cache.md).
- [Измерения](measurements/) и [проверка Windows 2026-10-30](measurements/windows-2026-10-30.md)
  фиксируют конкретные среды и даты; они не заменяют пользовательскую инструкцию.
- [Roadmap](roadmap/README.md) и [исходная спецификация](eska-roadmap.md) содержат
  также запланированные и отложенные возможности. Наличие пункта там не означает
  наличие команды или редактора в продукте.

Материал сверяется с текущими CLI `--help`, кодом компонентов, `package.json`
Explorer и подтверждёнными проверками. Исторические отчёты сохраняют результаты
своего этапа; актуальные возможности перечислены в руководстве.
