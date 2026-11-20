# Самостоятельный VCS

`eska-vcs` выполняет существующие Git/workflow-операции над проектом Designer
XML. Сборка из checkout: `cargo build -p eska-vcs`. Полная `eska`, project-сервер,
платформа 1С и bsl-analyzer не нужны. До выборочной поставки T97 используется
локальная сборка.

```sh
eska-vcs --project-dir "$PROJECT_ROOT" status --format json
eska-vcs --project-dir "$PROJECT_ROOT" diff --semantic --format json
eska-vcs --project-dir "$PROJECT_ROOT" history --limit 10 --format json
eska-vcs --project-dir "$PROJECT_ROOT" start TASK-123
eska-vcs --project-dir "$PROJECT_ROOT" save --dry-run
eska-vcs --project-dir "$PROJECT_ROOT" save -m "Изменено поведение"
eska-vcs --project-dir "$PROJECT_ROOT" switch --base --dry-run --format json
eska-vcs --project-dir "$PROJECT_ROOT" shelves --format json
```

`PROJECT_ROOT` задаёт вызывающая среда. Доступны `clone`, `status`, `diff`,
`history`, `start`, `switch`, `save`, `finish`, `shelve`, `unshelve`, `shelves`.
Без подкоманды выводится ошибка аргументов. Глобальные `--lang ru|en`,
`--project-dir`, `--help`, `--version` не запускают другие ESKA-компоненты.
Аргументы, проектные/workspace selectors, revision/semantic diff и ограничения
операций сохранены из общего CLI. Создание манифеста относится к `eska-project`.

## Общая реализация

Общий CLI и самостоятельный процесс используют те же обработчики, repository,
workflow и XML-модель. `eska-vcs` не содержит runner, native infobase, установщик,
LSP или движок BSL-анализа. Существующий консервативный semantic diff читает
точные HEAD/index/worktree-снимки изменённых путей и сообщает fallbacks;
это не статический анализ проекта. Полный обход BSL для status/diff/save
не добавлен.

Repository discovery, refs, history, ancestry, status и committed snapshots
используют закреплённый `gix`. System Git остаётся capability fallback в одном
infrastructure layer: orchestration index/worktree/switch, hooks/editor/signing,
очистка отслеживаемых путей при сохранении полки и явно неподдержанный
transport. При остальных ошибках `gix` возвращает диагностику. Для разбора
результатов команд используются коды завершения и машинный вывод Git.
Clone/fetch могут требовать транспортные helpers Git. Для read-only
status/diff/history/shelves Git в PATH не нужен.

`eska clone` использует параллельную распаковку pack и checkout из gix. Число
потоков выбирает gix по доступным логическим процессорам и объёму работы.
Для этих стадий действуют настройки Git `pack.threads` и `checkout.workers`.
По умолчанию gix ограничивает checkout на macOS четырьмя рабочими потоками.

`eska clone` и `eska-vcs clone` сохраняют рабочую копию после полного checkout
и возвращают 0. Команда проверяет манифест в корне клона. Для `[workspace]`
она также проверяет перечисленные проекты. При отсутствии манифеста или ошибке
проверки в `stderr` появляется сообщение с префиксом `WARN`.

`status` параллельно обходит рабочее дерево и проверяет отслеживаемые файлы,
а gix читает большие индексы параллельно при наличии их таблицы смещений.

В терминале `clone`, а также сетевые стадии `start` и `finish` показывают
счётчики, объём и скорость через строчный рендерер Gitoxide в `stderr`.
Названия этапов локализованы. При перенаправлении `stderr` команды выводят
обычные диагностики и результат в прежнем формате.

## Сохранность и контракты

Workflow сохраняет существующие preflight и policy. Start отклоняет dirty
repository; finish проверяет интеграцию/публикацию согласно policy, ancestry,
состояние всего worktree и только затем завершает локальную задачу. Команда
не публикует изменения. Switch и полки работают со всем repository, включая
соседние workspace members и файлы вне вложенного проекта.

`start`, `save`, `switch` и `finish` ищут `eska.toml` только в текущем рабочем
дереве Git. Если внутри независимого вложенного репозитория манифест отсутствует,
команды завершаются до изменения файлов, индекса или веток.

Полка сохраняет raw index, отдельные staged/unstaged байты, non-ignored
untracked, удаления и mode. Дерево для защиты staged-объектов строится через
`gix` из сохранённого индекса. Если полка содержит только untracked-файлы,
для сохранения и восстановления Git в PATH не нужен. Dry-run не создаёт полку
и не меняет worktree.
Restore требует совпадения branch/base и отсутствия конфликтов; ошибка
сохраняет единственную копию состояния. BOM/CRLF и произвольные Git path bytes
проходят через прежний контракт. [Подробности workflow и полок](roadmap/05-safe-vcs.md).

В обычном Git-репозитории без манифеста `status --format json` возвращает схему 4
с полями `schema_version`, `repository` и `changes`. Объект `repository` содержит
`root`, `root_encoding`, состояние `head` (`attached`, `detached` или `unborn`),
`branch` и `branch_encoding`. При detached HEAD оба поля ветки равны `null`.
`changes` содержит счётчики для всего рабочего дерева.

Для проекта и workspace `status` использует схему 3. У текущего файлового diff
схема 1, у сравнения ревизий 2, у semantic diff 4, у истории и полок 1.
JSON независим от локали и сохраняет обратимые кодировки путей и Git-строк. Human-вывод и ошибки доступны
на русском и английском; redirect/NO_COLOR подавляет оформление.
Exit 0 — успех, 1 — отказ операции, 2 — ошибка аргументов.

## Проверка T95

51 существующий process-тест clone/diff/history/save запускается повторно
против самостоятельного бинарника, включая rollback index, hooks/editor,
workspace selection, semantic fallbacks и revision JSON. Общий CLI продолжает
проходить исходный набор.

Три дополнительных process-теста проверяют всю справку RU/EN, четыре типа
проекта и read-only JSON при пустом PATH, отказ start без Git без изменения
refs/index/source. На Unix отдельный PATH содержит только Git: реальные
start → dirty task → base → task → save → finish сохраняют staged/unstaged
и untracked байты; finish до интеграции отклоняется. Все fixtures создаются
и удаляются только в уникальных каталогах соседнего playground.

```sh
ESKA_TEST_ROOT="$(realpath ../eska-playground)" cargo test -p eska-vcs
```

Полный workspace fmt/check/strict Clippy/test/Rustdoc прошёл: 859 тестов passed,
12 ignored. Первый полный запуск остановился на прежнем analyzer version-тесте;
после добавления фактического результата в assertion отдельный тест, его набор
и повторный полный прогон прошли. Причина первого сбоя не установлена;
исправление product-кода анализатора не заявлено.

Normal dependency graph проверен без полной ESKA, build runtime, project-сервера,
анализатора и updater. Нативная матрица этого этапа выполнена на Linux x86_64;
Windows/macOS и Remote этим результатом не подтверждены. Полная OS-матрица
относится к T89.

## Время жизни блокировки полок

`Session` явно освобождает OS-lock при завершении операции. Одного закрытия
своего `File` недостаточно: другой descriptor того же open file, в том числе
унаследованный конкурентным fork до exec, может продлить блокировку.
[Контракт std::fs::File](https://doc.rust-lang.org/std/fs/struct.File.html#method.unlock)
предусматривает явный `unlock` для этой границы владельца.
Детерминированная регрессия удерживает дублированный descriptor: до исправления
следующая Session получала `Locked`, после — захватывает lock сразу.
Это исправляет подтверждённый механизм удержания, но без syscall trace не
приписывает ему каждый прежний случай `Shelf(Locked)`.
