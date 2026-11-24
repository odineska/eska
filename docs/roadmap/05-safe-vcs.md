# Незавершённые и безопасные VCS-операции

## T18 — `eska history`

**Статус:** `DONE`
**Зависит от:** T07, T12, T17

Показать локальную commit/task history через `gix`, без fetch и без изменения
repository. Первая версия имеет ограничение количества записей, локализованный
human output и стабильный JSON; commit ID, parents, author/time и subject
возвращаются структурированно. Связь с task показывается только там, где она
однозначно выводится из refs и workflow policy.

Не добавлять собственный history index, graph renderer и provider API до
подтверждённой необходимости.

**Реализовано:** `eska history [--limit 1..=1000] [--format human|json]` читает
достижимые из локального HEAD commits через `gix`, сортирует их по времени commit
и не выполняет fetch. Human-вывод локализован, JSON версии 1 не зависит от locale
и сохраняет commit ID, parents, author/time, subject и task структурированно;
произвольные Git-байты передаются с явной обратимой кодировкой.

Task attribution консервативен: commit должен находиться вне локальной base и
быть достижим ровно из одной локальной task-ветки, соответствующей workflow
policy. Отсутствующий workflow или base, влитая история и неоднозначные refs дают
`task = null`. Собственный индекс, graph renderer и provider API не добавлены.

История доступна и в Git-репозитории без `eska.toml`. В этом случае поле
`task` равно `null`. В workspace для привязки коммитов к задачам используется
общий workflow при запуске как из корня, так и из каталога участника.

## T34 — `eska switch`

**Статус:** `DONE`
**Зависит от:** T13, T15, T18

Безопасное переключение существующих локальных веток:

```text
eska switch <branch>
eska switch --base
```

Позиционное имя выбирает точную локальную ветку без добавления task-префикса,
независимо от workflow. Например, `eska switch develop` выбирает `develop`.
Манифест для этого пути не нужен; `--base` разрешает базовую ветку через policy
и требует `eska.toml`. Команда не создаёт ветки и не выполняет fetch.

Preflight через `gix` проверяет локальную ref и весь worktree. Автоматическое
сохранение и восстановление branch-bound полок описано в T35. Чистый detached
HEAD допускает возврат на локальную ветку с восстановлением её полки; грязный
отклоняется, поскольку его изменения нельзя привязать к исходной ветке.
Изменение HEAD/index/worktree выполняет существующий изолированный system Git
layer с отключённым угадыванием remote-веток и защитой ignored-файлов.
Human output локализован для RU/EN; JSON schema 1 сохраняет nullable `task`
со значением `null` для точного переключения веток.

## T35 — `shelve` / `unshelve` / `shelves`

**Статус:** `DONE`
**Зависит от:** T15, T34

Сделать переключение между задачами безопасным при незакоммиченных изменениях.
Обычный `eska switch <branch>` и `eska switch --base` должны автоматически:

1. сохранить Git-видимое состояние текущей ветки в привязанную к ней полку;
2. переключить ветку через существующий безопасный путь T34;
3. восстановить полку целевой ветки, если она существует.

При возвращении в задачу пользователь получает прежнее разделение index и
worktree: staged и unstaged изменения, удаления, переименования, executable bit
и untracked non-ignored файлы. Scope охватывает весь repository, включая пути
вне вложенного проекта и соседние workspace members, потому что branch switch
изменяет общий HEAD/index/worktree. Ignored-файлы не входят в полку и остаются на
диске; потенциальная коллизия не должна приводить к их перезаписи.

Добавить явные команды `eska shelve`, `eska unshelve` и `eska shelves` для
ручного управления тем же состоянием. Public CLI и сообщения не используют
термин `stash`. Полка хранит исходную branch ref, commit, время, стабильный ID и
краткий состав изменений; список и результаты доступны в human и JSON режимах.
`--dry-run` показывает сохраняемые или восстанавливаемые пути и изменения index,
не меняя repository.

Явные команды полок работают и без манифеста, используя ближайшее рабочее
дерево Git. Повреждённый локальный манифест останавливает операцию до записи.

Автоматическое восстановление разрешено только для полки той же branch ref и
ожидаемого commit. Если ветка сдвинулась, появились конфликтующие файлы или
восстановление не может быть доказано безопасным, `switch` сохраняет полку и
останавливается с явной инструкцией. Ошибка capture, switch или restore не
удаляет единственную копию состояния. Полка удаляется только после полного
успешного восстановления и проверки результата. Незавершённые merge/rebase,
unmerged index и detached HEAD отклоняются до любых изменений.

Первичный backend может использовать Git stash через единый изолированный
system Git layer, без разбора локализованного human output. Формат backend не
является публичным контрактом. Не добавлять remote storage, синхронизацию полок,
автоматический shelve для `start`/`finish` и T37 `continue`/`abort`.

**Готово, когда:** циклы `task A dirty → task B dirty → task A` и
`task → base → task` побайтно восстанавливают исходные index/worktree и пути;
проверены staged + unstaged одного файла, add/delete/rename, untracked, mode,
пробелы/кириллица, workspace и изменения вне проекта. Ошибки и повторный запуск
не теряют полку; конфликты, сдвиг branch и незавершённые Git-операции безопасно
останавливаются. RU/EN, JSON, exit codes, redirect/`NO_COLOR` и Windows/Unix
покрыты integration-тестами.

**Результат:** реализованы `shelve`, `unshelve [id]`, `shelves` и автоматический
цикл сохранения/восстановления в `switch <branch>` / `switch --base`, включая
workspace root/member. Добавлены RU/EN, JSON schema v1, read-only `--dry-run`.

Backend хранит исходный raw index и SHA-256 снимки changed paths в общем
Git-каталоге `eska-shelves/`; staged tree защищена private ref через gix.
Это сохраняет BOM/CRLF и разделение staged/unstaged без повторной нормализации.
System Git используется только через infrastructure layer: `write-tree` для
index tree, `restore` с literal NUL pathspec для очистки captured tracked paths,
`switch --no-guess --no-overwrite-ignore` для перехода между ветками. Полка
привязана к полной branch ref и точному commit; на ветку допускается одна полка.

Проверяются все payloads до восстановления; index заменяется через index.lock,
файлы — через временные соседние файлы. Журнал разрешает повторный `unshelve`
после частичного восстановления только при совпадении текущих файлов с исходным
или восстановленным состоянием. Ошибка очистки вызывает попытку rollback;
прерывание очистки сохраняет полку, но может потребовать ручного восстановления.
Внешние Git/editor mutations не входят в гарантию блокировки между командами eska.

Sparse/split index, submodules, intent-to-add, skip-worktree, assume-unchanged,
unmerged index, detached/unborn HEAD и незавершённые Git-операции отклоняются.
Формат storage внутренний; public CLI и JSON документированы в README.
Проверены циклы между dirty задачами и базой, вложенный проект, workspace,
raw index, staged/unstaged, rename/delete, ignored collisions, сдвиг ветки,
повреждение payload, повторное восстановление и linked worktree preflight.
Переносимые integration-тесты выполнены на Linux; Unix отдельно проверяет
не-UTF-8 пути, symlinks, executable bit и ошибку записи. Native Windows/macOS
прогон остаётся внешней проверкой T55; здесь он не заявляется выполненным.

## T36 — `eska restore`

**Статус:** `PLANNED`
**Зависит от:** T14, T15, T18

Восстанавливать явно выбранный project/object-aware scope с предварительным
preview и safeguards. Потенциально разрушающий путь требует явного `--force`,
если безопасной альтернативы нет; `reset --hard` не используется как общий
механизм. Backend выбирается по тем же gix-first правилам и документирует
оставшийся system Git fallback.
