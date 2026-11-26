# ESKA

[![Release-plz](https://github.com/odineska/eska/actions/workflows/release-plz.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release-plz.yml)
[![Binaries](https://github.com/odineska/eska/actions/workflows/release.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release.yml)
[![Open VSX](https://img.shields.io/open-vsx/v/1c-tooling/eska-explorer?label=Open%20VSX)](https://open-vsx.org/extension/1c-tooling/eska-explorer)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

ESKA помогает разрабатывать проекты 1С из выгрузки Designer XML. В VS Code и VSCodium можно найти документ по имени, изменить его форму и запрос, затем посмотреть изменения по объектам 1С. Из терминала тот же проект можно проверить, сравнить с предыдущей версией и собрать в CF, CFE, EPF или ERF.

Проект в активной разработке. [Установка](https://odineska.tech/eska/) · [Руководство по Explorer](https://odineska.tech/eska/docs/explorer/) · [Справочник CLI](https://odineska.tech/eska/docs/cli/)

## Explorer в работе

Каждый ролик снят на двух копиях «Демонстрационного приложения» 2.0.0.21 для платформы 8.5.4.2072. Сначала показан интерфейс «Такси», затем «Воздушный». Нажмите на изображение, чтобы открыть видео с паузой и перемоткой.

### Посмотреть тип стандартного реквизита

В справочнике «Товары» раскройте стандартные реквизиты. В свойствах реквизита «Код» видны тип «Строка» и длина 9.

[![Посмотреть тип стандартного реквизита. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-26/explorer-standard-attributes.gif)](https://odineska.tech/eska/docs/#demo-standard-attributes)

### Перейти от свойства к его записи в XML

В карточке справочника «Товары» перейдите от свойства «Имя» к его определению в XML.

[![Перейти от свойства к его записи в XML. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-26/explorer-properties.gif)](https://odineska.tech/eska/docs/#demo-properties)

### Изменить заголовок поля на форме заказа

Измените заголовок «Покупатель» на «Клиент» в редакторе формы. Посмотрите, как меняется предпросмотр, затем отмените правку.

[![Изменить заголовок поля на форме заказа. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-26/explorer-form-edit.gif)](https://odineska.tech/eska/docs/#demo-form-edit)

### Отредактировать запрос внутри модуля BSL

Откройте запрос из общего модуля «Пользователи», добавьте сортировку по наименованию и сохраните её в исходном BSL.

[![Отредактировать запрос внутри модуля BSL. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-26/explorer-query.gif)](https://odineska.tech/eska/docs/#demo-query)

### Различить два изменённых файла ObjectModule.bsl

В панели Git откройте изменения модулей справочников «Контрагенты» и «Товары».

[![Различить два изменённых файла ObjectModule.bsl. Скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-26/explorer-scm.gif)](https://odineska.tech/eska/docs/#demo-scm)

## Подключить выгрузку

Если файлы конфигурации уже лежат в `my-project/src`, установите ESKA и выполните команды из родительского каталога.

```sh
eska-project init ./my-project --source src
eska-project --project-dir ./my-project validate
```

Откройте `my-project` в Explorer. Для дерева нужен `eska-project`, для форматирования нужен `eska-fmt`, для сборки файла поставки нужна платформа 1С с `ibcmd`. [Выбор компонентов](https://odineska.tech/eska/docs/#components) · [Проверки в CI](https://odineska.tech/eska/docs/#check) · [Пример разработки обработки с БСП](https://odineska.tech/eska/docs/#practice)

Спасибо команде [bsl-analyzer](https://github.com/itrous/bsl-analyzer) за открытый анализатор BSL. Его языковой сервер даёт Explorer переходы к определениям, подсказки и диагностику. Тот же анализатор проверяет код при запуске `eska check`. [Выпуски bsl-analyzer](https://github.com/itrous/bsl-analyzer/releases)

[Код Explorer](https://github.com/odineska/eska-vscode-explorer) · [Документация разработчика](docs/README.md) · [Сообщить об ошибке](https://github.com/odineska/eska/issues) · [Apache-2.0](LICENSE)
