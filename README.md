# ESKA

[![Release-plz](https://github.com/odineska/eska/actions/workflows/release-plz.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release-plz.yml)
[![Binaries](https://github.com/odineska/eska/actions/workflows/release.yml/badge.svg)](https://github.com/odineska/eska/actions/workflows/release.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

**Инструменты для разработки на 1С в формате Designer XML.**

ESKA объединяет работу с выгрузкой Конфигуратора, Git, проверками и сборкой
`.cf`, `.cfe`, `.epf`, `.erf`. Расширение **ESKA: 1C Explorer** добавляет дерево
метаданных, свойства, редактор и конструктор запросов, BSL-сервисы и форматирование
в VS Code / VSCodium. Исходниками остаются обычные XML, BSL и ресурсы проекта.

[Подробное руководство на сайте](https://odineska.github.io/eska/docs/) ·
[Руководство в репозитории](docs/user-guide.md) ·
[Практический пример с БСП](docs/user-guide.md#practice) ·
[Справочник документации](docs/README.md)

## Компоненты

| Программа | Назначение |
| --- | --- |
| `eska` | Общий вход: вызывает компонент выбранной операции |
| `eska-project` | Создание и подключение проектов, метаданные, контекст приложения и сервер IDE |
| `eska-check` | Полная структурная проверка XML и анализ BSL для CI |
| `eska-fmt` | Форматирование BSL и запросов без анализатора |
| `eska-build` | Сборка, распаковка, import и patch средствами установленной платформы 1С |
| `eska-vcs` | Git-workflow, состояние, semantic diff, история, сохранение и полки |

Для дерева Explorer достаточно `eska-project`. BSL Analyzer — отдельный внешний
инструмент; для сборки нужна установленная платформа с `ibcmd`.
[ESKA: 1C Explorer](https://github.com/odineska/eska-vscode-explorer).

## Быстрый старт

Установка опубликованного комплекта:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://odineska.github.io/eska/install | sh
```

Windows PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://odineska.github.io/eska/install.ps1 | iex"
```

Повторный запуск установщика обновляет все шесть утилит до одного выбранного
релиза, в том числе после выборочной установки из Explorer. BSL Analyzer
управляется отдельно. Откройте новый терминал после изменения PATH.

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

Лицензия: [Apache-2.0](LICENSE).
