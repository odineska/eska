# ESKA

[![Release-plz](https://github.com/odineska/eska/actions/workflows/release-plz.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release-plz.yml)
[![Binaries](https://github.com/odineska/eska/actions/workflows/release.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release.yml)
[![Open VSX](https://img.shields.io/open-vsx/v/1c-tooling/eska-explorer?label=Open%20VSX)](https://open-vsx.org/extension/1c-tooling/eska-explorer)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**Инструменты для разработки на 1С в формате Designer XML.**

ESKA подключает выгрузку Конфигуратора, проверяет XML и BSL, ведёт изменения в Git и собирает файлы `.cf`, `.cfe`, `.epf` и `.erf`. **ESKA: 1C Explorer** предоставляет инструменты для VS Code и VSCodium.

**Проект в активной разработке.** Доступные команды установленного выпуска перечислены в `--help`.

[Руководство на сайте](https://odineska.tech/eska/docs/) · [Справочник CLI](docs/cli-reference.md) · [Руководство в репозитории](docs/user-guide.md)

- **Метаданные.** Поиск объектов, свойств и исходников. Создание объектов, форм и модулей, копирование с новыми UUID и проверка ссылок перед удалением. Для автоматизации доступны JSON Schema и планы изменений. [Работа с метаданными](docs/metadata-authoring.md)
- **Управляемые формы.** Изменение свойств, типов и условного оформления в редакторе, перетаскивание элементов, создание обработчиков BSL, автосохранение и отмена. Предпросмотр формы 8.3.27 имеет статус beta. [Редактор форм](docs/user-guide.md#forms)
- **BSL и запросы.** Подсказки, переходы и диагностика через BSL Analyzer, форматирование BSL и SDBL, отдельные вкладки запросов с сохранением в исходник через **Ctrl+S** и визуальный конструктор. [Запросы](docs/user-guide.md#queries) · [Форматирование](docs/formatting.md)
- **Git.** В терминале доступны задачи, история, полки и семантический diff. В панели Explorer можно сравнить и индексировать файлы по именам объектов 1С и создать коммит. [Git в Explorer](docs/user-guide.md#scm) · [Команды VCS](docs/vcs-tool.md)

## Запуск и отладка 1С

В профиле `test` можно обновить базу или приложение, установить или удалить расширение и запустить клиент. Для `test` и `prod` доступны запуск с отладкой и подключение к HTTP-серверу `dbgs`. Отладчик поддерживает точки останова, условия, шаги, стек, переменные, Watch и вычисление выражений BSL. Адреса базы и сервера задаются в `.eska/settings_local.toml`.

[Настройка и первый запуск](docs/run-debug.md) · [Самостоятельная работа с DAP](docs/eska-debug-standalone.md)

## Explorer в работе

Скринкасты записаны в VS Code с Explorer 0.1.23 и ESKA 0.32.0. Нажмите на изображение, чтобы открыть видео с элементами управления.

<details>
<summary>Настроить проводник</summary>

[![Настроить проводник — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18/explorer-setup.gif)](https://odineska.tech/eska/docs/#demo-setup)

</details>

<details>
<summary>Найти объект и открыть модуль</summary>

[![Найти объект и открыть модуль — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18/explorer-tree.gif)](https://odineska.tech/eska/docs/#demo-tree)

</details>

<details>
<summary>Перейти из свойств к XML</summary>

[![Перейти из свойств к XML — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18/explorer-properties.gif)](https://odineska.tech/eska/docs/#demo-properties)

</details>

<details>
<summary>Открыть управляемую форму</summary>

[![Открыть управляемую форму — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18/explorer-forms.gif)](https://odineska.tech/eska/docs/#demo-forms)

</details>

<details>
<summary>Изменить заголовок поля формы</summary>

[![Изменить заголовок поля формы — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18/explorer-form-edit.gif)](https://odineska.tech/eska/docs/#demo-form-edit)

</details>

<details>
<summary>Сравнить изменения модулей по именам объектов</summary>

[![Сравнить изменения модулей по именам объектов — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18/explorer-scm.gif)](https://odineska.tech/eska/docs/#demo-scm)

</details>

<details>
<summary>Изменить запрос и сохранить через Ctrl+S</summary>

[![Изменить запрос и сохранить через Ctrl+S — скринкаст без звука](https://raw.githubusercontent.com/odineska/odineska.github.io/main/assets/demos/2026-11-18-query/explorer-query.gif)](https://odineska.tech/eska/docs/#demo-query)

</details>

## Компоненты

| Программа | Назначение |
| --- | --- |
| `eska` | Общий вход: вызывает компонент выбранной операции |
| `eska-project` | Создание и подключение проектов, метаданные, контекст приложения и сервер IDE |
| `eska-check` | Полная структурная проверка XML и анализ BSL для CI |
| `eska-fmt` | Форматирование BSL и запросов без анализатора |
| `eska-build` | Сборка, распаковка, import и patch средствами установленной платформы 1С |
| `eska-vcs` | Git-workflow, состояние, semantic diff, история, сохранение и полки |
| `eska-debug` | HTTP-отладка 1С на Rust через штатный `dbgs`. [Самостоятельный режим DAP](docs/eska-debug-standalone.md) |

Для дерева Explorer достаточно `eska-project`.
[BSL Analyzer](https://github.com/itrous/bsl-analyzer) — отдельный внешний
инструмент для анализа BSL и языковых функций; для сборки нужна установленная
платформа с `ibcmd`.
[ESKA: 1C Explorer](https://github.com/odineska/eska-vscode-explorer).

## Быстрый старт

Установка опубликованного комплекта:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://odineska.tech/eska/install | sh
```

Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://odineska.tech/eska/install.ps1 | iex"
```

Повторный запуск установщика обновляет все семь утилит до одного выбранного
релиза, в том числе после выборочной установки из Explorer. BSL Analyzer
управляется отдельно. Откройте новый терминал после изменения PATH.

Установка полного комплекта через Cargo:

```sh
cargo install --locked eska-project eska-check eska-build eska-vcs eska-fmt eska-debug eska
```

Подключение уже существующей выгрузки без Git и платформы:

```sh
eska-project init ./my-project --source src
eska-project --project-dir ./my-project validate
```

В Explorer выберите project backend и при необходимости задайте путь
`eska.explorer.executable`. Для обработки или отчёта библиотека БСП подключается
командой `eska add ssl_3_1`; отсутствующая копия устанавливается в `~/.eska/lib`.

Документация описывает исходники текущей ветки. Готовый выпуск может отставать:
проверяйте состав [GitHub Releases](https://github.com/odineska/eska/releases)
и совместимость компонентов. Сборка текущих исходников:

```sh
cargo build --workspace --bins --release --locked
```

[Установка и совместимость](docs/user-guide.md#installation) ·
[Все команды](docs/cli-reference.md) · [Конфигурация](docs/user-guide.md#configuration) ·
[Ограничения](docs/user-guide.md#capabilities) · [Roadmap](docs/roadmap/README.md)

## Для разработки ESKA

[Архитектура](docs/architecture.md) · [Правила работы](AGENTS.md) ·
[Контракты IDE](docs/ide-protocol.md) · [Форматирование](docs/formatting.md) ·
[Сборка и её проверка](docs/build-validation.md) · [История выпусков](docs/releases.md)

[Задачи и правила трекера](docs/task-tracker.md) ·
[Публичная доска разработки](https://github.com/orgs/odineska/projects/1)

Лицензия: [Apache-2.0](LICENSE).
