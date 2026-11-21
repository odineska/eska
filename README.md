# ESKA

[![Release-plz](https://github.com/odineska/eska/actions/workflows/release-plz.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release-plz.yml)
[![Binaries](https://github.com/odineska/eska/actions/workflows/release.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release.yml)
[![Open VSX](https://img.shields.io/open-vsx/v/1c-tooling/eska-explorer?label=Open%20VSX)](https://open-vsx.org/extension/1c-tooling/eska-explorer)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

ESKA помогает разрабатывать проекты 1С из выгрузки Designer XML. В VS Code и VSCodium можно найти документ по имени, изменить его форму и запрос, затем посмотреть изменения по объектам 1С. Из терминала тот же проект можно проверить, сравнить с предыдущей версией и собрать в CF, CFE, EPF или ERF.

Проект в активной разработке. [Установка](https://odineska.tech/eska/) · [Руководство по Explorer](https://odineska.tech/eska/docs/explorer/) · [Справочник CLI](https://odineska.tech/eska/docs/cli/)

## Explorer в работе

Нажмите на любой скринкаст, чтобы открыть видео с паузой и перемоткой. Первый ролик записан в Explorer 0.1.32 с ESKA 0.38.0, остальные четыре сняты в Explorer 0.1.23 с ESKA 0.32.0.

### Посмотреть тип стандартного реквизита

В справочнике «Товары» раскройте стандартные реквизиты. В свойствах «Пометки удаления» виден тип «Булево» и доступность поля для редактирования.

[![Посмотреть тип стандартного реквизита. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-21/explorer-standard-attributes.gif)](https://odineska.tech/eska/docs/#demo-standard-attributes)

### Перейти от свойства к его записи в XML

Выберите свойство «Имя» в карточке документа и откройте его определение в выгрузке.

[![Перейти от свойства к его записи в XML. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-21/explorer-properties.gif)](https://odineska.tech/eska/docs/#demo-properties)

### Переименовать поле на форме и увидеть результат

Измените заголовок «Покупатель» на «Клиент» в редакторе формы. В ролике видно, как меняется предпросмотр при правке, отмене и повторе.

[![Переименовать поле на форме и увидеть результат. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-21/explorer-form-edit.gif)](https://odineska.tech/eska/docs/#demo-form-edit)

### Отредактировать запрос внутри модуля BSL

Откройте текст запроса в отдельной вкладке, измените сортировку и нажмите Ctrl+S. Изменение появится в строковом литерале модуля.

[![Отредактировать запрос внутри модуля BSL. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-21/explorer-query.gif)](https://odineska.tech/eska/docs/#demo-query)

### Различить два изменённых файла ObjectModule.bsl

Панель Git показывает имена объектов 1С. Выберите объект и откройте изменения именно его модуля.

[![Различить два изменённых файла ObjectModule.bsl. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-21/explorer-scm.gif)](https://odineska.tech/eska/docs/#demo-scm)

## Подключить выгрузку

Если файлы конфигурации уже лежат в `my-project/src`, установите ESKA и выполните команды из родительского каталога.

```sh
eska-project init ./my-project --source src
eska-project --project-dir ./my-project validate
```

Откройте `my-project` в Explorer. Для дерева нужен `eska-project`, для форматирования нужен `eska-fmt`, для сборки файла поставки нужна платформа 1С с `ibcmd`. [Выбор компонентов](https://odineska.tech/eska/docs/#components) · [Проверки в CI](https://odineska.tech/eska/docs/#check) · [Пример разработки обработки с БСП](https://odineska.tech/eska/docs/#practice)

Спасибо команде [bsl-analyzer](https://github.com/itrous/bsl-analyzer) за открытый анализатор BSL. Его языковой сервер даёт Explorer переходы к определениям, подсказки и диагностику. Тот же анализатор проверяет код при запуске `eska check`. [Выпуски bsl-analyzer](https://github.com/itrous/bsl-analyzer/releases)

[Код Explorer](https://github.com/odineska/eska-vscode-explorer) · [Документация разработчика](docs/README.md) · [Сообщить об ошибке](https://github.com/odineska/eska/issues) · [Apache-2.0](LICENSE)
