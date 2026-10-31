# Проверка сборки на поддерживаемых ОС

Документ разделяет проверку переносимого process pipeline и приёмку с настоящей
платформой 1С. Тестовый `ibcmd` подтверждает передачу нативных аргументов,
stdout/stderr, exit codes и очистку ресурсов, но не совместимость Designer XML с
конкретной версией 1С.

## Переносимый процессный стенд

`tests/fixtures/ibcmd` — отдельный Rust-процесс без shell-команд. Интеграционный
набор собирает его один раз под текущую host-платформу и передаёт путь через
публичный `build --ibcmd`.

Linux и macOS:

```bash
ESKA_TEST_ROOT="$(realpath ../eska-playground)" \
  cargo test --test integration cli::build
```

Windows PowerShell:

```powershell
$env:ESKA_TEST_ROOT = (Resolve-Path ..\eska-playground).Path
cargo test --test integration cli::build
```

Набор использует пути с пробелом и кириллицей и проверяет:

| Контракт | Сценарий |
|---|---|
| `.cf`, `.cfe`, `.epf`, `.erf` и JSON | `builds_all_native_artifact_types_with_locale_independent_json` |
| контекст основной конфигурации для `.epf`/`.erf` | `external_build_uses_base_configuration_context_and_source_directory` |
| повторное использование и принудительное пересоздание служебной базы | `repeated_build_reuses_managed_infobase` |
| пересоздание при изменении базового `.cf` | `changed_base_configuration_invalidates_managed_infobase` |
| точное совпадение версии платформы | `exact_platform_version_is_required` |
| workspace, selectors и aggregate JSON | `workspace_build_uses_shared_outputs_and_distinct_json_shapes` |
| dry-run и отсутствие побочных эффектов | `dry_run_human_is_localized_and_does_not_change_the_filesystem` |
| паспорт и зафиксированный снимок исходников | `manifested_build_uses_the_captured_source_snapshot` |
| безопасная замена и сохранение служебной базы после ошибки | `failed_build_preserves_existing_artifact_and_managed_infobase` |
| прерывание, сохранение базы и очистка staging | `interrupted_build_preserves_managed_infobase` |

Сценарий прерывания пока посылает POSIX `SIGTERM`, поэтому исполняется только на
Unix. Проверка отказа и cleanup переносима, но не заменяет проверку Ctrl+C на
Windows. Проверка символических ссылок также остаётся Unix-специфичной.

### Зафиксированные результаты

| Дата | Host | Rust | Переносимый стенд | Настоящая 1С |
|---|---|---|---|---|
| 2026-09-09 | Fedora Linux 44, x86_64 | 1.98.1 | `PASS`: 28/28, включая SIGTERM и symlink | `UNVERIFIED`: `ibcmd` отсутствует |
| 2026-09-09 | Fedora Linux 44 + Distrobox `1c-ubuntu-env`, x86_64 | 1.98.1 | — | `PARTIAL`: `.epf` с базовым `.cf` на 8.3.27.2325 |
| 2026-10-20 | Linux x86_64 + Distrobox `1c-ubuntu-env` | 1.98.1 | полный Rust workspace: 805 passed, 12 ignored | `PASS` T94: build/repeat/unpack/clean CF/CFE/EPF/ERF, native cancellation; [отчёт](measurements/t94-native.json) |
| — | Windows, x86_64 | — | `UNVERIFIED`: host-runner недоступен | `UNVERIFIED` |
| — | macOS, x86_64/aarch64 | — | `UNVERIFIED`: host-runner недоступен | `UNVERIFIED` |

Компиляция для другого target не меняет `UNVERIFIED` на `PASS`: нужен фактический
запуск тестового executable на соответствующей ОС.

## Протокол с настоящей платформой 1С

Для каждой проверенной комбинации сохранить:

- ОС, архитектуру, версию Rust и полный вывод `ibcmd --version`;
- точные команды `eska`, runner (`host` или `distrobox`) и пути проектов;
- exit code, stdout и stderr каждой команды;
- размеры и SHA-256 файлов `.cf`, `.cfe`, `.epf`, `.erf` и их паспортов;
- результат повторной сборки поверх существующего артефакта;
- результат Ctrl+C во время import: прежний артефакт и служебная ИБ сохранены,
  staging-файлы удалены;
- результат `eska clean`: служебная ИБ удалена, опубликованный артефакт сохранён;
- дату проверки и имя исполнителя.

Минимальная последовательность для каждого из четырёх типов проекта:

```text
ibcmd --version
eska build --dry-run --format json --ibcmd <полный-путь-к-ibcmd>
eska build --manifest --format json --ibcmd <полный-путь-к-ibcmd>
```

Для workspace выполнить те же две команды из корня без selector, затем с `-p`
для одного member. На Linux сценарий Distrobox записывается отдельно с
`--distrobox <container>`; он не считается проверкой host runner. Если ОС,
архитектура или версия 1С недоступны, строка результата должна оставаться
`UNVERIFIED` с причиной.

T94 проверяет самостоятельный [eska-build](build-tool.md). Повторная нативная
матрица запускается `tests/native_build.py` с явными Distrobox, путём ibcmd
и точной версией; все проекты и базы создаются в уникальном соседнем playground.
Этот запуск не закрывает оставшуюся OS-матрицу T55 и не считается проверкой
настоящего host runner или функциональной работой клиента 1С.

## Автоматическая основа EPF/ERF (T147)

Новые process-сценарии находятся в `crates/eska-build/tests/build_reference`
и `runtime_base`. Они проверяют manifest-ссылки по имени/пути, отдельные приложения,
изменения содержимого/пути/платформы, checksum CF, ошибки и сохранность результатов.
Core отдельно проверяет ownership member и пересечение с рабочими исходниками.

Нативная серия: Linux ARM64, ALT/Distrobox, 1С 8.3.27.2342.
[Матрица и ограничения](measurements/t147-build-libraries.json).
EPF/ERF с типом из библиотеки собираются обычным `eska build`, имеют тот же
native XML-экспорт, что вариант с явным CF; без основы импорт отклоняется.
Проверены два приложения workspace, member/selectors, повтор, паспорт, очистка
и отмена подготовки CF. Скрипт работает только в новом соседнем playground,
исходную библиотеку копирует и проверяет её сохранность:

```sh
ESKA_TEST_ROOT="$(realpath ../eska-playground)" python3 -B tests/native_libraries.py \
  --library-source "$ESKA_LIBRARY_SOURCE" --catalog "$ESKA_LIBRARY_CATALOG" \
  --platform-version "$PLATFORM_VERSION" --distrobox "$ESKA_DISTROBOX" \
  --platform-arch "$ESKA_PLATFORM_ARCH" --ibcmd "$ESKA_IBCMD"
```

Перед запуском собрать `eska`, `eska-build`, `eska-vcs`; значения переменных
относятся к выбранной установленной платформе. `--ibcmd` используется для
контрольного native-экспорта, build ищет стандартный путь по machine config.
Другие ОС и host runner этим результатом не подтверждены.


## Общая подготовка именованной библиотеки (T148)

Process-сценарии `build_reference::shared` проверяют CF-производителя без
`--manifest`, разные потребители, отдельные изменяемые ИБ, канонические пути,
root source layout, повреждение/удаление CF и шаблона, параллельную подготовку,
отмену ожидания lease, read-only исходники и locale-independent ошибки.

`tests/native_libraries.py` теперь проверяет общий CF, неизменяемую основу ИБ
и два параллельных native-потребителя. Реальная БСП 3.1 на 8.3.27.2342 прошла
всю серию Linux ARM64/ALT Distrobox. Новый независимый отчёт — 9,59 с после
подготовки основы; первоначальная сборка без общего результата — 24,62 с.
Проверка исходных байтов остаётся обязательной. Explorer отдельно проверен
в VS Code 1.144.0: раннее изменение manifest и живой `eska add` с dirty-буфером.
[Результаты и ограничения](measurements/t148-build-libraries.json).
