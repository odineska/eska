# Самостоятельный eska-debug

У `eska-debug` два явных входа к одной DAP/HTTP-сессии.

- `eska-debug standalone` получает параметры из DAP. Для подключения нужны
  исполняемый файл, DAP-клиент, исходники Designer XML и доступный `dbgs`.
  Локальная 1С нужна для запуска её клиента. Запуск готовой EPF использует
  манифест её сборки. Самостоятельный адаптер запускается одним бинарником.
- `eska-debug dap --profile test --mode launch` загружает проект ESKA, локальный
  профиль и состав приложения. В `prod` действует проверка исходников Git.
  Этот режим использует Explorer. [Проектный режим и приёмка](1c-http-debug.md)

Соберите бинарник через `cargo build --release -p eska-debug` и скопируйте
`target/release/eska-debug` (Windows — `eska-debug.exe`) в нужный каталог.
Настройте клиент DAP на запуск `eska-debug standalone` с транспортом
stdio. Сеанс использует имена файлов и номера строк от 1.

## Запросы `attach` и `launch`

Общие параметры в `arguments` запроса DAP:

| Поле | Значение |
| --- | --- |
| `sourceRoot` | Каталог выгрузки основной конфигурации, содержащий `Configuration.xml`. При запуске EPF можно опустить |
| `extensionRoots` | Необязательный список каталогов выгрузок расширений. Имя каждого читается из его `Configuration.xml` |
| `debugServerUrl` | HTTP-адрес работающего `dbgs`, например `http://127.0.0.1:1550` |
| `infobaseAlias` | Имя базы, по которому она зарегистрирована в `dbgs`. По умолчанию `DefAlias`, при `launch` кластерной или автономной базы — её `name` |
| `autoAttachTypes` | Типы предметов для автоматического подключения. По умолчанию `ManagedClient`, `Client`, `Server`, `ServerEmulation` |

Для `attach` укажите `targetId`, если нужен один существующий предмет по UUID.
Если `targetId` опущен, адаптер подключает доступные предметы выбранных типов
после `configurationDone` и подключает новые по мере их появления. Отладка
затем идёт обычными запросами `setBreakpoints`, `threads`, `stackTrace`,
`scopes`, `variables`, `evaluate`, `setVariable`, `pause`, `next`,
`continue` и `disconnect`.
Подключение использует переданные локальные исходники. Их совпадение с кодом
в базе проверяет разработчик.

Для `launch` дополнительно передайте `infobase` и `runner`. Пример параметров:

```json
{
  "sourceRoot": "/work/export",
  "extensionRoots": ["/work/extension-export"],
  "debugServerUrl": "http://127.0.0.1:1550",
  "infobase": { "kind": "file", "path": "/work/local-base" },
  "runner": { "kind": "local", "version": "8.3.27.2342" }
}
```

Для базы также доступны `{"kind":"cluster","host":"host","name":"db"}`
и `{"kind":"standalone","url":"http://host:8314","name":"db"}`.
`runner.kind` допускает `local` и `distrobox`. Для `distrobox` укажите
`container`. В обоих случаях можно указать абсолютный `runner.ibcmd` для
выбора установки платформы. Клиент `1cv8c` берётся из той же установки.
Необязательный `authentication` содержит `user` и `password_env` с именем
переменной окружения. Значение пароля не передаётся в DAP. Относительные
пути в запросах разрешаются от рабочей директории адаптера. При отключении
адаптер завершает только запущенный им клиент.

При `launch` файловой базы можно указать `managedDebugServer: true` вместо
`debugServerUrl`. Штатный `dbgs` запускается из выбранной установки 1С,
слушает `127.0.0.1` и завершает работу вместе с сеансом. Необязательное
поле `debugServerPort` задаёт фиксированный порт. Самостоятельный `attach`
использует адрес уже запущенного сервера.

### Запуск готовой EPF

Добавьте в аргументы `launch` поле `externalProcessing` с путём к файлу
`artifact` и каталогом выгрузки этой обработки `sourceRoot`.

```json
{
  "externalProcessing": {
    "artifact": "/work/ReportTools.epf",
    "sourceRoot": "/work/ReportTools/src"
  },
  "debugServerUrl": "http://127.0.0.1:1550",
  "infobase": { "kind": "file", "path": "/work/local-base" },
  "runner": { "kind": "local", "version": "8.3.27.2342" }
}
```

Рядом с EPF нужен файл `<имя>.epf.manifest.json` версии 1, например
полученный командой `eska-build build --manifest` при сборке артефакта.
Адаптер проверяет SHA-256 EPF и снимок исходников из манифеста. Клиент 1С
получает приватную копию EPF через `/Execute` после передачи точек останова.
Если 1С показывает диалог безопасности, подтвердите открытие в клиенте.
Для перехода из стека адаптер проверяет URL загруженной копии, UUID и
содержимое модуля. После изменения исходника его путь скрывается из стека
до восстановления совпадения со снимком. Чтобы отладить новый код, соберите
новую EPF с манифестом и начните новый сеанс.

## Пример для VS Code

Для VS Code отладочный `type` объявляется расширением. В каталоге
[standalone-vscode](examples/standalone-vscode) есть декларативный `package.json`
без JavaScript и зависимостей. Скопируйте его и бинарник в один каталог, затем
откройте VS Code с `--extensionDevelopmentPath=<этот каталог>`. Параметр `program`
в манифесте запускает `eska-debug standalone`. [Документация VS Code](https://code.visualstudio.com/api/references/contribution-points#contributes.debuggers)

Пример `.vscode/launch.json` для файловой базы с работающим `dbgs`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "1C HTTP Attach",
      "type": "onec-http",
      "request": "attach",
      "sourceRoot": "${workspaceFolder}/src",
      "debugServerUrl": "http://127.0.0.1:1550",
      "infobaseAlias": "DefAlias"
    }
  ]
}
```

Для запуска поменяйте `request` на `launch` и добавьте `infobase` и `runner`.
Оставляйте локальные адреса и настройки аутентификации вне общего
`launch.json`, если они относятся к вашей машине. Адаптер переданные параметры
не сохраняет.

## Переход с OnecDebugAdapter

Создайте конфигурацию с типом `onec-http` из примера или настройте внешний
stdio-адаптер в своём DAP-клиенте. Перенесите адрес `dbgs` в `debugServerUrl`,
путь к основной XML-выгрузке в `sourceRoot`, пути к выгрузкам расширений в
`extensionRoots`. Для запуска укажите `infobase` и `runner`. Конфигурации
другого расширения используют собственные аргументы запуска и поля запроса.
Проверьте `attach` на тестовой базе, затем добавьте параметры `launch`.

## Проверка

На Linux ARM64 с 1С 8.3.27.2342 скопированный бинарник прошёл `launch`,
`attach` по списку типов и `attach` по UUID. Для точного подключения `PATH`
адаптера был пустым. Проверены точки, шаг, локальные переменные, Evaluate в
текущем и вызывающем кадрах, пути основной конфигурации и двух расширений,
а также сохранность уже запущенного клиента после `disconnect`. Декларативный
пример прошёл `launch` и `attach` в отдельном VS Code 1.144.0 без Explorer.
В этой же среде проверен самостоятельный запуск EPF через Distrobox,
с точкой в управляемой форме, переходом из стека, локальной переменной и
Evaluate. Отдельная проверка DAP отклоняет заменённый артефакт или изменённые
исходники и скрывает путь изменённого модуля во время сеанса.
