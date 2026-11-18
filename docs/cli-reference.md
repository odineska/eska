# Полный справочник команд ESKA

Справочник соответствует публичной справке текущих бинарников. Служебные
скрытые bridge-команды не являются пользовательскими входами. Справка не
меняет проект и доступна без платформы 1С или анализатора.

`eska --version` — версия программы; `eska version` — версия проекта 1С.
`--project-dir` принимает каталог. Наличие флага `--format`, `--workspace`,
`--project`, `--member` или `--application` зависит от конкретной команды.
В примерах ниже `ПУТЬ`, `ФАЙЛ`, `ИМЯ` заменяются вашими значениями.

[Практическое руководство](user-guide.md) · [Назначение компонентов](user-guide.md#components)

<a id="cli-eska"></a>

## eska

| Вход | Назначение |
| --- | --- |
| [`eska`](#command-eska) | Инструментарий разработки проектов 1С:Предприятие |
| [`eska add`](#command-eska-add) | Подключить БСП к внешнему отчёту или обработке |
| [`eska update`](#command-eska-update) | Обновить установленную ESKA |
| [`eska metadata`](#command-eska-metadata) | Чтение, изменение и создание метаданных Designer XML |
| [`eska metadata create`](#command-eska-metadata-create) | Создать объект метаданных, при необходимости на основе существующего. |
| [`eska metadata copy`](#command-eska-metadata-copy) | Создать полную копию объекта с новыми идентификаторами, модулями и ресурсами. |
| [`eska metadata delete`](#command-eska-metadata-delete) | Удалить объект и его файлы после проверки входящих ссылок. |
| [`eska metadata object-schema`](#command-eska-metadata-object-schema) | Получить допустимые типы объектов, ограничения и JSON Schema запросов. |
| [`eska metadata form-schema`](#command-eska-metadata-form-schema) | Получить структуру формы, свойства и события для редактирования. |
| [`eska metadata form-edit`](#command-eska-metadata-form-edit) | Проверить или применить семантические изменения формы из JSON. |
| [`eska metadata recover`](#command-eska-metadata-recover) | Проверить или восстановить прерванную запись метаданных, включая создание начальной конфигурации. |
| [`eska metadata list`](#command-eska-metadata-list) | Найти объекты в выбранном родителе без рекурсивного обхода конфигурации. |
| [`eska metadata add`](#command-eska-metadata-add) | Добавить реквизит или табличную часть через общую модель метаданных |
| [`eska metadata create-module`](#command-eska-metadata-create-module) | Создать отсутствующий модуль поддерживаемой роли |
| [`eska metadata create-schema`](#command-eska-metadata-create-schema) | Прочитать доступные операции создания, владельцев, типы и ограничения |
| [`eska ide`](#command-eska-ide) | Постоянный read-only процесс для IDE |
| [`eska build`](#command-eska-build) | Собрать нативный артефакт 1С из Designer XML |
| [`eska run`](#command-eska-run) | Запустить 1С:Предприятие с профилем test |
| [`eska apply`](#command-eska-apply) | Предпросмотр и применение изменений Designer XML к тестовой базе |
| [`eska extension`](#command-eska-extension) | Управление расширениями в тестовой базе. |
| [`eska extension install`](#command-eska-extension-install) | Предпросмотр или установка выбранного расширения Designer XML в тестовую базу. |
| [`eska extension remove`](#command-eska-extension-remove) | Предпросмотр или удаление выбранного установленного расширения из тестовой базы. |
| [`eska check`](#command-eska-check) | Полная проверка Designer XML и BSL для CI |
| [`eska fmt`](#command-eska-fmt) | Настраиваемое форматирование BSL и запросов без анализатора |
| [`eska clean`](#command-eska-clean) | Удалить служебную базу сборки |
| [`eska clone`](#command-eska-clone) | Клонировать и проверить существующий проект eska |
| [`eska config`](#command-eska-config) | Управлять глобальными настройками этого компьютера |
| [`eska new`](#command-eska-new) | Создать новый каркас проекта |
| [`eska init`](#command-eska-init) | Подключить существующую Designer XML выгрузку |
| [`eska import`](#command-eska-import) | Заменить исходники проекта из файла CF, CFE, EPF или ERF |
| [`eska platform`](#command-eska-platform) | Найти установленные платформы 1С |
| [`eska patch`](#command-eska-patch) | Собрать патч из изменений методов общих модулей между Git-версиями |
| [`eska diff`](#command-eska-diff) | Показать изменения файлов проекта |
| [`eska doctor`](#command-eska-doctor) | Диагностировать окружение текущего проекта |
| [`eska finish`](#command-eska-finish) | Завершить активную задачу по правилам workflow |
| [`eska history`](#command-eska-history) | Показать локальную историю commit |
| [`eska save`](#command-eska-save) | Сохранить текущие изменения проекта в историю Git |
| [`eska start`](#command-eska-start) | Начать работу над задачей по workflow проекта |
| [`eska status`](#command-eska-status) | Показать состояние проекта и workflow |
| [`eska switch`](#command-eska-switch) | Переключиться на существующую задачу или базовую ветку workflow |
| [`eska shelve`](#command-eska-shelve) | Сохранить незакоммиченные изменения текущей ветки в полку |
| [`eska unshelve`](#command-eska-unshelve) | Восстановить полку текущей ветки с сохранением index и рабочих файлов |
| [`eska shelves`](#command-eska-shelves) | Показать сохранённые полки всех веток репозитория |
| [`eska version`](#command-eska-version) | Показать или изменить версию проекта 1С |
| [`eska metadata inspect`](#command-eska-metadata-inspect) | Прочитать свойства и схему допустимых изменений |
| [`eska metadata types`](#command-eska-metadata-types) | Получить допустимые типы существующего поля |
| [`eska metadata choices`](#command-eska-metadata-choices) | Получить допустимые ссылки на метаданные для существующего поля |
| [`eska metadata check`](#command-eska-metadata-check) | Проверить изменение свойства и показать заменяемые байты |
| [`eska metadata apply`](#command-eska-metadata-apply) | Применить одно проверенное изменение свойства |
| [`eska metadata rename-preview`](#command-eska-metadata-rename-preview) | Показать план переименования и сомнительные ссылки без записи файлов |
| [`eska metadata rename-apply`](#command-eska-metadata-rename-apply) | Применить проверенный план переименования по снимку источников |
| [`eska metadata rename-recovery`](#command-eska-metadata-rename-recovery) | Проверить или восстановить прерванное переименование |
| [`eska config init`](#command-eska-config-init) | Создать глобальный конфиг по умолчанию |
| [`eska config edit`](#command-eska-config-edit) | Открыть глобальный конфиг в редакторе |
| [`eska platform list`](#command-eska-platform-list) | Показать платформы в текущем раннере |
| [`eska version bump`](#command-eska-version-bump) | Увеличить один компонент версии проекта |

<a id="command-eska"></a>

<details>
<summary>eska: синтаксис и все параметры</summary>

```text
eska 0.32.0
Инструментарий разработки проектов 1С:Предприятие

Использование: eska [ПАРАМЕТРЫ] [КОМАНДА]

Команды:
  add        Подключить БСП к внешнему отчёту или обработке
  update     Обновить установленную ESKA
  metadata   Чтение, изменение и создание метаданных Designer XML
  ide        Постоянный read-only процесс для IDE
  build      Собрать нативный артефакт 1С из Designer XML
  run        Запустить 1С:Предприятие с профилем test
  apply      Предпросмотр и применение изменений Designer XML к тестовой базе
  extension  Управление расширениями в тестовой базе.
  check      Полная проверка Designer XML и BSL для CI
  fmt        Настраиваемое форматирование BSL и запросов без анализатора
  clean      Удалить служебную базу сборки
  clone      Клонировать и проверить существующий проект eska
  config     Управлять глобальными настройками этого компьютера
  new        Создать новый каркас проекта
  init       Подключить существующую Designer XML выгрузку
  import     Заменить исходники проекта из файла CF, CFE, EPF или ERF
  platform   Найти установленные платформы 1С
  patch      Собрать патч из изменений методов общих модулей между Git-версиями
  diff       Показать изменения файлов проекта
  doctor     Диагностировать окружение текущего проекта
  finish     Завершить активную задачу по правилам workflow
  history    Показать локальную историю commit
  save       Сохранить текущие изменения проекта в историю Git
  start      Начать работу над задачей по workflow проекта
  status     Показать состояние проекта и workflow
  switch     Переключиться на существующую задачу или базовую ветку workflow
  shelve     Сохранить незакоммиченные изменения текущей ветки в полку
  unshelve   Восстановить полку текущей ветки с сохранением index и рабочих файлов
  shelves    Показать сохранённые полки всех веток репозитория
  version    Показать или изменить версию проекта 1С
Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
  -V, --version                Показать версию
```

</details>

<a id="command-eska-add"></a>

<details>
<summary>eska add: синтаксис и все параметры</summary>

```text
Подключить БСП к внешнему отчёту или обработке

Использование: eska add <БИБЛИОТЕКА> [-p <ПРОЕКТ>] [--format human|json]

Аргументы:
  <БИБЛИОТЕКА>  Известная библиотека: ssl_3_1 или ssl_3_2; отсутствующая устанавливается из master в ~/.eska/lib
Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-update"></a>

<details>
<summary>eska update: синтаксис и все параметры</summary>

```text
Обновить установленную ESKA

Usage: eska update [OPTIONS]

Options:
      --check
          Проверить обновления без изменения установки
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --target-version <TARGET_VERSION>
          Выбрать точную стабильную версию; понижение версии запрещено
      --format <FORMAT>
          Формат вывода [default: human] [possible values: human, json]
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-metadata"></a>

<details>
<summary>eska metadata: синтаксис и все параметры</summary>

```text
Чтение, изменение и создание метаданных Designer XML

Usage: eska metadata [OPTIONS] <COMMAND>

Commands:
  create           Создать объект метаданных, при необходимости на основе существующего.
  create-module    Создать отсутствующий модуль поддерживаемой роли
  copy             Создать полную копию объекта с новыми идентификаторами, модулями и ресурсами.
  delete           Удалить объект и его файлы после проверки входящих ссылок.
  object-schema    Получить допустимые типы объектов, ограничения и JSON Schema запросов.
  form-schema      Получить структуру формы, свойства и события для редактирования.
  form-edit        Проверить или применить семантические изменения формы из JSON.
  recover          Проверить или восстановить прерванную запись метаданных, включая создание начальной конфигурации.
  list             Найти объекты в выбранном родителе без рекурсивного обхода конфигурации.
  add              Добавить реквизит или табличную часть через общую модель метаданных
  create-schema    Прочитать доступные операции создания, владельцев, типы и ограничения
  inspect          Прочитать свойства и схему допустимых изменений
  types            Получить допустимые типы существующего поля
  choices          Получить допустимые ссылки на метаданные для существующего поля
  check            Проверить изменение свойства и показать заменяемые байты
  apply            Применить одно проверенное изменение свойства
  rename-preview   Показать план переименования и сомнительные ссылки без записи файлов
  rename-apply     Применить проверенный план переименования по снимку источников
  rename-recovery  Проверить или восстановить прерванное переименование

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-metadata-create"></a>

<details>
<summary>eska metadata create: синтаксис и все параметры</summary>

```text
Создать объект метаданных, при необходимости на основе существующего.

Usage: eska metadata create [OPTIONS]

Options:
      --kind <KIND>                 Ключ типа из object-schema, например catalog, document или common-module.
      --lang <ЛОКАЛЬ>               Выбрать язык интерфейса
      --name <NAME>                 Имя нового элемента в конфигурации
      --project-dir <КАТАЛОГ>       Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --parent <PARENT>             Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --template <TEMPLATE>         Идентификатор существующего объекта того же типа, используемого как образец.
      --set <PROPERTY=VALUE>        Явное значение скалярного свойства; можно повторять. Идентификаторы менять нельзя.
      --reference <PROPERTY=VALUE>  Ссылка в свойстве-списке; повторяйте для нескольких значений, например RegisterRecords=AccumulationRegister.Остатки.
      --type <PROPERTY=TYPE>        Тип в свойстве-списке; повторяйте для составного типа, например Source=cfg:DocumentObject.Заказ или Type=xs:string.
      --input <INPUT>               Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>                 Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                     Вернуть подготовленный план создания без записи исходников
      --no-input                    Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                        Показать справку
  -p, --project <PROJECT>           Выбрать один проект рабочего пространства
      --format <FORMAT>             Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-copy"></a>

<details>
<summary>eska metadata copy: синтаксис и все параметры</summary>

```text
Создать полную копию объекта с новыми идентификаторами, модулями и ресурсами.

Usage: eska metadata copy [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>               Выбрать язык интерфейса
      --source <SOURCE>             Идентификатор копируемого объекта из metadata list или inspect.
      --name <NAME>                 Имя нового элемента в конфигурации
      --project-dir <КАТАЛОГ>       Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --parent <PARENT>             Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --set <PROPERTY=VALUE>        Явное значение скалярного свойства; можно повторять. Идентификаторы менять нельзя.
      --reference <PROPERTY=VALUE>  Ссылка в свойстве-списке; повторяйте для нескольких значений, например RegisterRecords=AccumulationRegister.Остатки.
      --type <PROPERTY=TYPE>        Тип в свойстве-списке; повторяйте для составного типа, например Source=cfg:DocumentObject.Заказ или Type=xs:string.
      --input <INPUT>               Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>                 Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                     Вернуть подготовленный план создания без записи исходников
      --no-input                    Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                        Показать справку
  -p, --project <PROJECT>           Выбрать один проект рабочего пространства
      --format <FORMAT>             Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-delete"></a>

<details>
<summary>eska metadata delete: синтаксис и все параметры</summary>

```text
Удалить объект и его файлы после проверки входящих ссылок.

Usage: eska metadata delete [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --object <OBJECT>        ID объекта из metadata inspect; по умолчанию корневой объект
      --force                  Разрешить удаление с обнаруженными входящими ссылками; ссылки останутся неразрешёнными.
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --yes                    Подтвердить удаление без интерактивного запроса. Не заменяет проверку ссылок и --force.
      --input <INPUT>          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>            Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                Вернуть подготовленный план создания без записи исходников
      --no-input               Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-object-schema"></a>

<details>
<summary>eska metadata object-schema: синтаксис и все параметры</summary>

```text
Получить допустимые типы объектов, ограничения и JSON Schema запросов.

Usage: eska metadata object-schema [OPTIONS]

Options:
      --kind <KIND>            Ключ типа из object-schema, например catalog, document или common-module.
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --parent <PARENT>        Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-form-schema"></a>

<details>
<summary>eska metadata form-schema: синтаксис и все параметры</summary>

```text
Получить структуру формы, свойства и события для редактирования.

Usage: eska metadata form-schema [OPTIONS] --form <FORM>

Options:
      --form <FORM>            Идентификатор формы из metadata list или inspect.
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-form-edit"></a>

<details>
<summary>eska metadata form-edit: синтаксис и все параметры</summary>

```text
Проверить или применить семантические изменения формы из JSON.

Usage: eska metadata form-edit [OPTIONS]

Options:
      --input <INPUT>          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --plan <PLAN>            Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --dry-run                Вернуть подготовленный план создания без записи исходников
      --no-input               Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-recover"></a>

<details>
<summary>eska metadata recover: синтаксис и все параметры</summary>

```text
Проверить или восстановить прерванную запись метаданных, включая создание начальной конфигурации.

Usage: eska metadata recover [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-list"></a>

<details>
<summary>eska metadata list: синтаксис и все параметры</summary>

```text
Найти объекты в выбранном родителе без рекурсивного обхода конфигурации.

Usage: eska metadata list [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --parent <PARENT>        Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --kind <KIND>            Ключ типа из object-schema, например catalog, document или common-module.
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --search <SEARCH>        Отфильтровать объекты по части идентификатора или имени без учёта регистра.
      --limit <LIMIT>          Максимальное число объектов в ответе (1–500). [default: 100]
      --after <AFTER>          Курсор nextAfter из предыдущей страницы; не включает указанный объект.
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-add"></a>

<details>
<summary>eska metadata add: синтаксис и все параметры</summary>

```text
Добавить реквизит или табличную часть через общую модель метаданных

Usage: eska metadata add [OPTIONS] [KIND]

Arguments:
  [KIND]  Создаваемый элемент: реквизит или табличная часть [possible values: attribute, table-part]

Options:
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
      --owner <OWNER>
          ID существующего владельца: справочника, документа или поддерживаемой табличной части
      --name <NAME>
          Имя нового элемента в конфигурации
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --type <VALUE_TYPE>
          Тип реквизита: string, number, boolean, date или reference; для составного типа используйте --input
      --reference <REFERENCE>
          ID существующего объекта метаданных для ссылочного типа
      --length <LENGTH>
          Длина строки; значения по умолчанию доступны в create-schema
      --digits <DIGITS>
          Общая разрядность числа
      --fraction-digits <FRACTION_DIGITS>
          Число знаков дробной части
      --nonnegative
          Разрешить только неотрицательные числа
      --fixed-length
          Использовать фиксированную длину строки
      --date-fractions <DATE_FRACTIONS>
          Дата, время или дата и время [possible values: date, time, date-time]
      --synonym <LANGUAGE=TEXT>
          Локализованный синоним в виде LANGUAGE=TEXT; можно повторить для нескольких языков
      --comment <COMMENT>
          Комментарий разработчика
      --snapshot <SNAPSHOT>
          Требовать снимок владельца из create-schema; для точного применения предпросмотра используйте --plan
      --input <INPUT>
          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>
          Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --interactive
          Интерактивно выбрать владельца и свойства, затем проверить изменения
      --no-input
          Отключить все вопросы; при нехватке параметров вернуть ошибку
      --dry-run
          Вернуть подготовленный план создания без записи исходников
  -h, --help
          Показать справку
  -p, --project <PROJECT>
          Выбрать один проект рабочего пространства
      --format <FORMAT>
          Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-create-schema"></a>

<details>
<summary>eska metadata create-schema: синтаксис и все параметры</summary>

```text
Прочитать доступные операции создания, владельцев, типы и ограничения

Usage: eska metadata create-schema [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --owner <OWNER>          ID существующего владельца: справочника, документа или поддерживаемой табличной части
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-ide"></a>

<details>
<summary>eska ide: синтаксис и все параметры</summary>

```text
Постоянный read-only процесс для IDE

Использование: eska ide [OPTIONS] --stdio

Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --stdio                  Обмен сообщениями IDE protocol через stdin/stdout
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-build"></a>

<details>
<summary>eska build: синтаксис и все параметры</summary>

```text
Собрать нативный артефакт 1С из Designer XML

Usage: eska build [ПАРАМЕТРЫ]

Options:
      --lang <ЛОКАЛЬ>                 Выбрать язык интерфейса
      --output <ПУТЬ>                 Путь артефакта; относительный путь считается от корня проекта или workspace; требует выбора одного проекта
      --base-configuration <ФАЙЛ_CF>  Базовая конфигурация .cf для сборки внешней обработки или отчёта
      --project-dir <КАТАЛОГ>         Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --recreate-infobase             Всегда пересоздавать служебную базу перед сборкой
      --ibcmd <ПУТЬ>                  Явный путь к ibcmd (или ESKA_IBCMD)
      --platform-arch <АРХИТЕКТУРА>   Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <КОНТЕЙНЕР>         Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --platform-version <ВЕРСИЯ>     Версия платформы только для текущей сборки
      --select-platform               Интерактивно выбрать установленную платформу только для текущей сборки
      --format <human|json>           Формат вывода [default: human] [possible values: human, json]
  -p, --project <ПРОЕКТ>              Выбрать проект workspace по имени; параметр можно повторять
      --workspace                     Выбрать все проекты workspace
      --dry-run                       Показать полностью проверенный план сборки без создания артефактов
      --manifest                      Собрать артефакт из зафиксированного снимка и записать рядом машиночитаемый паспорт
  -h, --help                          Показать справку
```

</details>

<a id="command-eska-check"></a>

<details>
<summary>eska check: синтаксис и все параметры</summary>

```text
Полная проверка Designer XML и BSL для CI

Usage: eska check [OPTIONS]

Options:
      --application <APPLICATION>  Выбрать приложение, объявленное в eska.toml
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --analyzer <ANALYZER>        Исполняемый файл опубликованного bsl-analyzer-app; по умолчанию ESKA_BSL_ANALYZER или PATH
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --timeout <TIMEOUT>          Таймаут каждого процесса анализатора в секундах [default: 600]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --report <REPORT>            Создать новый JSON-отчёт вне source; существующий файл сохраняется
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-fmt"></a>

<details>
<summary>eska fmt: синтаксис и все параметры</summary>

```text
Настраиваемое форматирование BSL и запросов без анализатора

Usage: eska fmt [OPTIONS] [SCOPE]

Arguments:
  [SCOPE]  Файл или каталог объекта относительно project-dir; по умолчанию исходники проекта

Options:
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --member <MEMBER>            Выбранный member workspace; можно указать несколько
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --workspace                  Все members workspace
      --check                      Проверить без записи; код 1 при необходимых изменениях
      --diff                       Показать исходную и отформатированную версии без записи
      --dump-config                Полная политика с происхождением значений без записи
      --style <STYLE>              Sparse override операции в JSON
      --query-at <QUERY_AT>        Явно форматировать статический BSL-запрос по UTF-8 byte offset
      --query-index <QUERY_INDEX>  Явно форматировать XML-запрос по индексу с нуля
      --migrate-config             Предпросмотр миграции настроек в секцию format
      --apply-migration            Применить миграцию настроек с резервной копией
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-clean"></a>

<details>
<summary>eska clean: синтаксис и все параметры</summary>

```text
Удалить служебную базу сборки

Usage: eska clean [ПАРАМЕТРЫ]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; параметр можно повторять
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --workspace              Удалить служебные базы всех проектов workspace
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-clone"></a>

<details>
<summary>eska clone: синтаксис и все параметры</summary>

```text
Клонировать и проверить существующий проект eska

Использование: eska clone [ПАРАМЕТРЫ] <РЕПОЗИТОРИЙ> [КАТАЛОГ]

Аргументы:
  <РЕПОЗИТОРИЙ>  URL или локальный путь репозитория
  [КАТАЛОГ]      Новый каталог назначения (по умолчанию — имя репозитория)

Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --remote <ИМЯ>           Имя для удалённого репозитория [default: origin]
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-config"></a>

<details>
<summary>eska config: синтаксис и все параметры</summary>

```text
Управлять глобальными настройками этого компьютера

Usage: eska config <КОМАНДА>

Commands:
  init  Создать глобальный конфиг по умолчанию
  edit  Открыть глобальный конфиг в редакторе

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-new"></a>

<details>
<summary>eska new: синтаксис и все параметры</summary>

```text
Создать новый каркас проекта

Использование: eska new [ПАРАМЕТРЫ] <КАТАЛОГ>

Аргументы:
  <КАТАЛОГ>  Новый каталог проекта (родительский каталог должен существовать)

Параметры:
      --application <APPLICATION>
          Приложение в workspace.members для нового участника; при нескольких приложениях выбор обязателен.
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
      --from <FROM>
          Создать проект из файла CF, CFE, EPF или ERF; определить тип по содержимому
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --ibcmd <IBCMD>
          Явный путь к ibcmd (или ESKA_IBCMD)
      --platform-arch <PLATFORM_ARCH>
          Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <DISTROBOX>
          Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --platform-version <PLATFORM_VERSION>
          Версия платформы 1С; сохраняется при создании проекта, действует однократно при импорте
      --select-platform
          Интерактивно выбрать установленную платформу 1С
      --type <ТИП>
          Тип проекта: configuration, extension, processing, report
      --workflow <СТРАТЕГИЯ>
          Workflow standalone-проекта: trunk, git-flow, github-flow, custom; для workspace задаётся в корне
      --no-vcs
          Не инициализировать Git
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-init"></a>

<details>
<summary>eska init: синтаксис и все параметры</summary>

```text
Подключить существующую Designer XML выгрузку

Использование: eska init [ПАРАМЕТРЫ] [КАТАЛОГ]

Аргументы:
  [КАТАЛОГ]  Каталог существующего проекта (по умолчанию — текущий)

Параметры:
      --application <APPLICATION>
          Приложение в workspace.members для нового участника; при нескольких приложениях выбор обязателен.
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --source <КАТАЛОГ>
          Каталог исходников относительно корня проекта (автопоиск: корень или src)
      --name <ИМЯ>
          Имя workspace-проекта; по умолчанию имя его каталога
      --workflow <СТРАТЕГИЯ>
          Workflow standalone-проекта: trunk, git-flow, github-flow, custom; для workspace задаётся в корне
      --no-vcs
          Не инициализировать Git
      --ibcmd <IBCMD>
          Явный путь к ibcmd (или ESKA_IBCMD)
      --platform-arch <PLATFORM_ARCH>
          Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <DISTROBOX>
          Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --platform-version <PLATFORM_VERSION>
          Версия платформы 1С; сохраняется в eska.toml (в workspace по умолчанию наследуется из корня)
      --select-platform
          Интерактивно выбрать установленную платформу 1С
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-import"></a>

<details>
<summary>eska import: синтаксис и все параметры</summary>

```text
Заменить исходники проекта из файла CF, CFE, EPF или ERF

Использование: eska import <ФАЙЛ> [ПАРАМЕТРЫ]

Аргументы:
  <ФАЙЛ>  Файл 1С для распаковки; путь относительно каталога запуска команды

Параметры:
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>
          Выбрать ровно один проект workspace по имени
      --force
          Подтвердить различия имени/UUID и потерю локальных исходников; проверки типа и путей сохраняются
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --dry-run
          Распаковать, проверить и показать план замены без изменения исходников проекта
      --format <FORMAT>
          Формат вывода [default: human] [possible values: human, json]
      --ibcmd <IBCMD>
          Явный путь к ibcmd (или ESKA_IBCMD)
      --platform-arch <PLATFORM_ARCH>
          Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <DISTROBOX>
          Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --platform-version <PLATFORM_VERSION>
          Версия платформы 1С; сохраняется при создании проекта, действует однократно при импорте
      --select-platform
          Интерактивно выбрать установленную платформу 1С
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-platform"></a>

<details>
<summary>eska platform: синтаксис и все параметры</summary>

```text
Найти установленные платформы 1С

Usage: eska platform <КОМАНДА>

Commands:
  list  Показать платформы в текущем раннере

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-patch"></a>

<details>
<summary>eska patch: синтаксис и все параметры</summary>

```text
Собрать патч из изменений методов общих модулей между Git-версиями

Usage: eska patch [OPTIONS]

Options:
      --base <BASE>
          Направление сравнения; по умолчанию integration_target
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
      --output <OUTPUT>
          Новый файл .cfe; существующие файлы не заменяются
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --dry-run
          Показать план без запуска платформы и записи файлов
      --ibcmd <IBCMD>
          Путь к ibcmd; рядом требуется 1cv8 той же установки
      --platform-arch <PLATFORM_ARCH>
          Архитектура платформы
      --distrobox <DISTROBOX>
          Контейнер Distrobox
      --platform-version <PLATFORM_VERSION>
          Версия платформы для этого запуска
      --format <FORMAT>
          Формат результата: human или json [default: human] [possible values: human, json]
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-diff"></a>

<details>
<summary>eska diff: синтаксис и все параметры</summary>

```text
Показать изменения файлов проекта

Usage: eska diff [ПАРАМЕТРЫ] [ОТКУДА] [КУДА]

Arguments:
  [РЕВИЗИЯ]...  Сравнить указанную ревизию с HEAD либо две указанные ревизии

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --since-branch-point     Сравнивать с точкой ветвления указанных ревизий
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --raw                    Вывести компактные коды изменений без локализации
      --semantic               Показать логические объекты, модули, методы, формы и свойства метаданных
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; параметр можно повторять
      --workspace              Выбрать все проекты и собственные файлы workspace
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-doctor"></a>

<details>
<summary>eska doctor: синтаксис и все параметры</summary>

```text
Диагностировать окружение текущего проекта

Usage: eska doctor [ПАРАМЕТРЫ]

Options:
      --ibcmd <ПУТЬ>                 Явный путь к ibcmd (или ESKA_IBCMD)
      --lang <ЛОКАЛЬ>                Выбрать язык интерфейса
      --platform-arch <АРХИТЕКТУРА>  Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --project-dir <КАТАЛОГ>        Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --distrobox <КОНТЕЙНЕР>        Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --format <ФОРМАТ>              Формат вывода: human или json [default: human] [possible values: human, json]
  -p, --project <ПРОЕКТ>             Выбрать проект workspace по имени; можно повторять
      --workspace                    Диагностировать все проекты workspace
  -h, --help                         Показать справку
```

</details>

<a id="command-eska-finish"></a>

<details>
<summary>eska finish: синтаксис и все параметры</summary>

```text
Завершить активную задачу по правилам workflow

Usage: eska finish

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

Если политика удаляет ветку задачи, перед завершением восстановите её полку через `unshelve` и сохраните изменения.

</details>

<a id="command-eska-history"></a>

<details>
<summary>eska history: синтаксис и все параметры</summary>

```text
Показать локальную историю commit

Usage: eska history [ПАРАМЕТРЫ]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -n, --limit <КОЛИЧЕСТВО>     Максимальное число commit для вывода (1–1000) [default: 20]
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-save"></a>

<details>
<summary>eska save: синтаксис и все параметры</summary>

```text
Сохранить текущие изменения проекта в историю Git

Usage: eska save [ПАРАМЕТРЫ]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -m, --message <СООБЩЕНИЕ>    Использовать указанное сообщение commit без генерации и редактора
  -p, --project <ПРОЕКТ>       Сохранить только один проект workspace по имени
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --workspace              Сохранить все изменения внутри workspace
      --dry-run                Показать точный план сохранения без изменения репозитория
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-start"></a>

<details>
<summary>eska start: синтаксис и все параметры</summary>

```text
Начать работу над задачей по workflow проекта

Usage: eska start <ЗАДАЧА>

Arguments:
  <ЗАДАЧА>  Идентификатор задачи для имени рабочей ветки

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-status"></a>

<details>
<summary>eska status: синтаксис и все параметры</summary>

```text
Показать состояние проекта и workflow

Usage: eska status [ПАРАМЕТРЫ]

Options:
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; параметр можно повторять
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --workspace              Выбрать все проекты и собственные файлы workspace
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-switch"></a>

<details>
<summary>eska switch: синтаксис и все параметры</summary>

```text
Переключиться на существующую задачу или базовую ветку workflow

Usage: eska switch [--dry-run] [--format human|json] <ЗАДАЧА> | --base

Arguments:
  [ЗАДАЧА]  Идентификатор существующей задачи

Options:
      --base                   Переключиться на базовую ветку workflow без завершения задачи
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --dry-run                Показать пути и состояния index/worktree без изменения репозитория
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-shelve"></a>

<details>
<summary>eska shelve: синтаксис и все параметры</summary>

```text
Сохранить незакоммиченные изменения текущей ветки в полку

Usage: eska shelve [OPTIONS]

Options:
      --dry-run                Показать пути и состояния index/worktree без изменения репозитория
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-unshelve"></a>

<details>
<summary>eska unshelve: синтаксис и все параметры</summary>

```text
Восстановить полку текущей ветки с сохранением index и рабочих файлов

Usage: eska unshelve [OPTIONS] [ID]

Arguments:
  [ID]  ID полки; по умолчанию полка текущей ветки

Options:
      --dry-run                Показать пути и состояния index/worktree без изменения репозитория
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-shelves"></a>

<details>
<summary>eska shelves: синтаксис и все параметры</summary>

```text
Показать сохранённые полки всех веток репозитория

Usage: eska shelves [OPTIONS]

Options:
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-version"></a>

<details>
<summary>eska version: синтаксис и все параметры</summary>

```text
Показать или изменить версию проекта 1С

Usage: eska version [--project <ПРОЕКТ>] [--workspace] [--format <ФОРМАТ>] [КОМАНДА]

Commands:
  bump  Увеличить один компонент версии проекта

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --workspace              Выбрать все проекты workspace
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-metadata-inspect"></a>

<details>
<summary>eska metadata inspect: синтаксис и все параметры</summary>

```text
Прочитать свойства и схему допустимых изменений

Usage: eska metadata inspect [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --object <OBJECT>        ID объекта из metadata inspect; по умолчанию корневой объект
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-types"></a>

<details>
<summary>eska metadata types: синтаксис и все параметры</summary>

```text
Получить допустимые типы существующего поля

Usage: eska metadata types [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-choices"></a>

<details>
<summary>eska metadata choices: синтаксис и все параметры</summary>

```text
Получить допустимые ссылки на метаданные для существующего поля

Usage: eska metadata choices [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-check"></a>

<details>
<summary>eska metadata check: синтаксис и все параметры</summary>

```text
Проверить изменение свойства и показать заменяемые байты

Usage: eska metadata check [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-apply"></a>

<details>
<summary>eska metadata apply: синтаксис и все параметры</summary>

```text
Применить одно проверенное изменение свойства

Usage: eska metadata apply [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-rename-preview"></a>

<details>
<summary>eska metadata rename-preview: синтаксис и все параметры</summary>

```text
Показать план переименования и сомнительные ссылки без записи файлов

Usage: eska metadata rename-preview [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-rename-apply"></a>

<details>
<summary>eska metadata rename-apply: синтаксис и все параметры</summary>

```text
Применить проверенный план переименования по снимку источников

Usage: eska metadata rename-apply [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-metadata-rename-recovery"></a>

<details>
<summary>eska metadata rename-recovery: синтаксис и все параметры</summary>

```text
Проверить или восстановить прерванное переименование

Usage: eska metadata rename-recovery [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-config-init"></a>

<details>
<summary>eska config init: синтаксис и все параметры</summary>

```text
Создать глобальный конфиг по умолчанию

Usage: eska config init

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-config-edit"></a>

<details>
<summary>eska config edit: синтаксис и все параметры</summary>

```text
Открыть глобальный конфиг в редакторе

Usage: eska config edit

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
```

</details>

<a id="command-eska-platform-list"></a>

<details>
<summary>eska platform list: синтаксис и все параметры</summary>

```text
Показать платформы в текущем раннере

Usage: eska platform list [ПАРАМЕТРЫ]

Options:
      --ibcmd <ПУТЬ>                 Явный путь к ibcmd (или ESKA_IBCMD)
      --lang <ЛОКАЛЬ>                Выбрать язык интерфейса
      --platform-arch <АРХИТЕКТУРА>  Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --project-dir <КАТАЛОГ>        Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --distrobox <КОНТЕЙНЕР>        Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --format <human|json>          Формат вывода [default: human] [possible values: human, json]
  -h, --help                         Показать справку
```

</details>

<a id="command-eska-version-bump"></a>

<details>
<summary>eska version bump: синтаксис и все параметры</summary>

```text
Увеличить один компонент версии проекта

Usage: eska version [--project <ПРОЕКТ>] bump <patch|minor|major> [--format <ФОРМАТ>]

Arguments:
  <КОМПОНЕНТ>  Компонент для увеличения [possible values: patch, minor, major]

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --workspace              Выбрать все проекты workspace
```

</details>

<a id="command-eska-run"></a>

<details>
<summary>eska run: синтаксис и все параметры</summary>

```text
Запустить 1С:Предприятие с профилем test

Usage: eska run [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --profile <PROFILE>      Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-apply"></a>

<details>
<summary>eska apply: синтаксис и все параметры</summary>

```text
Предпросмотр и применение изменений Designer XML к тестовой базе

Usage: eska apply [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --profile <PROFILE>          Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --application <APPLICATION>  Обновить указанное приложение workspace в порядке зависимостей.
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --initialize                 Предпросмотр явного полного импорта XML для нового снимка
      --confirm <CONFIRM>          Применить изменения из проверенного предпросмотра
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

Выполните первый запуск без `--confirm` для просмотра базы и состава изменений. Затем повторите команду с полученным токеном и теми же параметрами. Для первого полного импорта добавьте `--initialize`.

</details>

<a id="command-eska-extension"></a>

<details>
<summary>eska extension: синтаксис и все параметры</summary>

```text
Управление расширениями в тестовой базе.

Usage: eska extension [OPTIONS] <COMMAND>

Commands:
  install  Предпросмотр или установка выбранного расширения Designer XML в тестовую базу.
  remove   Предпросмотр или удаление выбранного установленного расширения из тестовой базы.

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-metadata-create-module"></a>

<details>
<summary>eska metadata create-module: синтаксис и все параметры</summary>

```text
Создать отсутствующий модуль поддерживаемой роли

Usage: eska metadata create-module [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --owner <OWNER>          ID существующего владельца модуля
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --role <ROLE>            Роль модуля из object-schema (например object или manager)
      --input <INPUT>          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>            Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                Вернуть подготовленный план создания без записи исходников
      --no-input               Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

Укажите идентификатор владельца из `metadata list` или `metadata inspect` и роль из `metadata object-schema`. С `--dry-run` можно проверить будущий путь и изменения до записи.

</details>

<a id="command-eska-extension-install"></a>

<details>
<summary>eska extension install: синтаксис и все параметры</summary>

```text
Предпросмотр или установка выбранного расширения Designer XML в тестовую базу.

Usage: eska extension install [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --profile <PROFILE>      Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --confirm <CONFIRM>      Применить изменения из проверенного предпросмотра
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                   Показать справку
```

Укажите проект расширения `InfoBase` через `--project-dir` или запустите команду из его каталога. Сначала получите план для профиля `test`, затем передайте его токен в `--confirm`.

</details>

<a id="command-eska-extension-remove"></a>

<details>
<summary>eska extension remove: синтаксис и все параметры</summary>

```text
Предпросмотр или удаление выбранного установленного расширения из тестовой базы.

Usage: eska extension remove [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --profile <PROFILE>      Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --confirm <CONFIRM>      Применить изменения из проверенного предпросмотра
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий)
      --force                  Явно разрешить удаление и возможную потерю данных расширения.
      --format <FORMAT>        Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                   Показать справку
```

Получите план удаления для профиля `test`. Для удаления повторите команду с `--confirm` и `--force`. Данные расширения могут быть удалены вместе с ним.

</details>


<a id="cli-eska-project"></a>

## eska-project

| Вход | Назначение |
| --- | --- |
| [`eska-project`](#command-eska-project) | Работа с проектами Designer XML |
| [`eska-project add`](#command-eska-project-add) | Подключить БСП к внешнему отчёту или обработке |
| [`eska-project doctor`](#command-eska-project-doctor) | Диагностировать окружение текущего проекта |
| [`eska-project serve`](#command-eska-project-serve) | Запустить самостоятельный сервер Metadata API проекта через stdio |
| [`eska-project analyzer-config`](#command-eska-project-analyzer-config) | Производная конфигурация bsl-analyzer |
| [`eska-project validate`](#command-eska-project-validate) | Проверить манифесты и корневые дескрипторы Designer XML |
| [`eska-project context`](#command-eska-project-context) | Определить контекст выбранного приложения |
| [`eska-project settings-local`](#command-eska-project-settings-local) | Просмотр и редактирование локальных настроек запуска проекта |
| [`eska-project settings-local show`](#command-eska-project-settings-local-show) | Показать текущие настройки и их ревизию |
| [`eska-project settings-local save`](#command-eska-project-settings-local-save) | Сохранить настройки из JSON-документа на стандартном вводе |
| [`eska-project settings-local validate`](#command-eska-project-settings-local-validate) | Проверить сохранённые профили |
| [`eska-project new`](#command-eska-project-new) | Создать проект Designer XML без инициализации Git |
| [`eska-project init`](#command-eska-project-init) | Подключить существующую выгрузку Designer XML без инициализации Git |
| [`eska-project metadata`](#command-eska-project-metadata) | Чтение, изменение и создание метаданных Designer XML |
| [`eska-project metadata create`](#command-eska-project-metadata-create) | Создать объект метаданных, при необходимости на основе существующего. |
| [`eska-project metadata copy`](#command-eska-project-metadata-copy) | Создать полную копию объекта с новыми идентификаторами, модулями и ресурсами. |
| [`eska-project metadata delete`](#command-eska-project-metadata-delete) | Удалить объект и его файлы после проверки входящих ссылок. |
| [`eska-project metadata object-schema`](#command-eska-project-metadata-object-schema) | Получить допустимые типы объектов, ограничения и JSON Schema запросов. |
| [`eska-project metadata form-schema`](#command-eska-project-metadata-form-schema) | Получить структуру формы, свойства и события для редактирования. |
| [`eska-project metadata form-edit`](#command-eska-project-metadata-form-edit) | Проверить или применить семантические изменения формы из JSON. |
| [`eska-project metadata recover`](#command-eska-project-metadata-recover) | Проверить или восстановить прерванную запись метаданных, включая создание начальной конфигурации. |
| [`eska-project metadata list`](#command-eska-project-metadata-list) | Найти объекты в выбранном родителе без рекурсивного обхода конфигурации. |
| [`eska-project metadata add`](#command-eska-project-metadata-add) | Добавить реквизит или табличную часть через общую модель метаданных |
| [`eska-project metadata create-module`](#command-eska-project-metadata-create-module) | Создать отсутствующий модуль поддерживаемой роли |
| [`eska-project metadata create-schema`](#command-eska-project-metadata-create-schema) | Прочитать доступные операции создания, владельцев, типы и ограничения |
| [`eska-project version`](#command-eska-project-version) | Показать или изменить версию проекта 1С |
| [`eska-project analyzer-config status`](#command-eska-project-analyzer-config-status) | Проверить актуальность без записи файлов |
| [`eska-project analyzer-config prepare`](#command-eska-project-analyzer-config-prepare) | Проверить и подготовить выбранный контекст |
| [`eska-project analyzer-config migrate`](#command-eska-project-analyzer-config-migrate) | Просмотреть миграцию; применить только с токеном этого preview |
| [`eska-project metadata inspect`](#command-eska-project-metadata-inspect) | Прочитать свойства и схему допустимых изменений |
| [`eska-project metadata types`](#command-eska-project-metadata-types) | Получить допустимые типы существующего поля |
| [`eska-project metadata choices`](#command-eska-project-metadata-choices) | Получить допустимые ссылки на метаданные для существующего поля |
| [`eska-project metadata check`](#command-eska-project-metadata-check) | Проверить изменение свойства и показать заменяемые байты |
| [`eska-project metadata apply`](#command-eska-project-metadata-apply) | Применить одно проверенное изменение свойства |
| [`eska-project metadata rename-preview`](#command-eska-project-metadata-rename-preview) | Показать план переименования и сомнительные ссылки без записи файлов |
| [`eska-project metadata rename-apply`](#command-eska-project-metadata-rename-apply) | Применить проверенный план переименования по снимку источников |
| [`eska-project metadata rename-recovery`](#command-eska-project-metadata-rename-recovery) | Проверить или восстановить прерванное переименование |
| [`eska-project version bump`](#command-eska-project-version-bump) | Увеличить один компонент версии проекта |

### Локальные профили запуска

```text
eska-project --project-dir <путь> settings-local show --format json
eska-project --project-dir <путь> settings-local validate
eska-project --project-dir <путь> settings-local save --format json
```

`show` возвращает `settings`, `revision` и `git_protection` в JSON версии 1.
Документ содержит локальные пути и адреса. `validate` проверяет объявления
профилей. `save` читает JSON с полями `revision` и `settings` из stdin и
возвращает новую ревизию. Для первого сохранения используйте `revision: null`.
Если файл изменился после чтения, прочитайте его заново и повторите правки.

Файл `.eska/settings_local.toml` относится к одному проекту. Если проект
находится в Git, сохранение допускается, когда файл исключён и его нет в
индексе. Для ранее созданного проекта проверьте `.gitignore` или локальные
исключения репозитория. `show` сообщает о проблеме защиты в `git_protection`.

Каждый профиль состоит из отдельных таблиц для базы, платформы и HTTP-отладки.
Следующий пример содержит только условные значения.

```toml
[test.infobase]
kind = "file"
path = "local/test-base"

[test.runner]
kind = "distrobox"
container = "onec"
version = "8.3.27.2342"

[prod.infobase]
kind = "cluster"
host = "server.example.invalid"
name = "demo"

[prod.runner]
kind = "local"

[prod.debug]
url = "http://debug.example.invalid:1550"
auto_attach_types = ["ManagedClient", "Server"]

[prod.authentication]
user = "developer"
password_env = "ESKA_DEMO_PASSWORD"
```

Путь к файловой базе может быть относительным к корню проекта. Профиль
автономной базы использует `kind = "standalone"`, `url` и `name`. Для применения
изменений задайте `admin_pid`, локальный PID процесса `ibsrv` в выбранном
раннере. Внутри Git-репозитория для проверки индекса и правил исключения
используется установленный `eska-vcs`. В `runner` можно указать
точную `version` и абсолютный путь `ibcmd`. `debug.url` задаёт отдельный
HTTP-сервер 1С. Для файловой базы вместо `debug.url` можно задать
`debug.managed = true` и необязательный `debug.port`. Управляемый сервер
слушает `127.0.0.1`; если порт не указан, его выбирает `dbgs`.
Без `debug.auto_attach_types` автоматически подключаются
`ManagedClient`, `Client`, `Server` и `ServerEmulation`. Пустой список
оставляет только ручной выбор. Поддерживаемые типы — `Client`, `ManagedClient`,
`WEBClient`, `COMConnector`, `Server`, `ServerEmulation`, `WEBService`,
`HTTPService`, `OData`, `JOB`, `JobFileMode`, `MobileClient`, `MobileServer`,
`MobileJobFileMode`, `MobileManagedClient`, `MobileManagedServer`.
В проекте типа `processing` таблица `debug.external_processing` задаёт
`artifact` и необязательный `debug_url` для подключения к загруженной копии.
Относительный путь артефакта считается от корня проекта. [Отладка EPF из проекта](1c-http-debug.md#epf-в-проекте-explorer)
`password_env` содержит имя переменной окружения, а её
значение передаётся отдельно от TOML.

<a id="command-eska-project"></a>

<details>
<summary>eska-project: синтаксис и все параметры</summary>

```text
eska-project 0.32.0
Работа с проектами Designer XML

Использование: eska-project [OPTIONS] [COMMAND]

Команды:
  add              Подключить БСП к внешнему отчёту или обработке
  doctor           Диагностировать окружение текущего проекта
  serve            Запустить самостоятельный сервер Metadata API проекта через stdio
  analyzer-config  Производная конфигурация bsl-analyzer
  validate         Проверить манифесты и корневые дескрипторы Designer XML
  context          Определить контекст выбранного приложения
  settings-local   Просмотр и редактирование локальных настроек запуска проекта
  new              Создать проект Designer XML без инициализации Git
  init             Подключить существующую выгрузку Designer XML без инициализации Git
  metadata         Чтение, изменение и создание метаданных Designer XML
  version          Показать или изменить версию проекта 1С
Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -h, --help                   Показать справку
  -V, --version                Показать версию
```

</details>

<a id="command-eska-project-add"></a>

<details>
<summary>eska-project add: синтаксис и все параметры</summary>

```text
Подключить БСП к внешнему отчёту или обработке

Использование: eska-project add [OPTIONS] <БИБЛИОТЕКА>

Аргументы:
  <БИБЛИОТЕКА>  Известная библиотека: ssl_3_1 или ssl_3_2; отсутствующая устанавливается из master в ~/.eska/lib
Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-project-doctor"></a>

<details>
<summary>eska-project doctor: синтаксис и все параметры</summary>

```text
Диагностировать окружение текущего проекта

Usage: eska-project doctor [OPTIONS]

Options:
      --ibcmd <ПУТЬ>                 Явный путь к ibcmd (или ESKA_IBCMD)
      --lang <ЛОКАЛЬ>                Выбрать язык интерфейса
      --platform-arch <АРХИТЕКТУРА>  Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --project-dir <КАТАЛОГ>        Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --distrobox <КОНТЕЙНЕР>        Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --format <ФОРМАТ>              Формат вывода: human или json [default: human] [possible values: human, json]
  -p, --project <ПРОЕКТ>             Выбрать проект workspace по имени; можно повторять
      --workspace                    Диагностировать все проекты workspace
  -h, --help                         Показать справку
```

</details>

<a id="command-eska-project-serve"></a>

<details>
<summary>eska-project serve: синтаксис и все параметры</summary>

```text
Запустить самостоятельный сервер Metadata API проекта через stdio

Использование: eska-project serve [OPTIONS] --stdio

Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --stdio                  Обмен сообщениями IDE protocol через stdin/stdout
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-project-analyzer-config"></a>

<details>
<summary>eska-project analyzer-config: синтаксис и все параметры</summary>

```text
Производная конфигурация bsl-analyzer

Использование: eska-project analyzer-config [OPTIONS] <COMMAND>

Команды:
  status   Проверить актуальность без записи файлов
  prepare  Проверить и подготовить выбранный контекст
  migrate  Просмотреть миграцию; применить только с токеном этого preview
Параметры:
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --application <APPLICATION>  Выбрать приложение, объявленное в eska.toml
      --analyzer <ANALYZER>        Исполняемый файл опубликованного bsl-analyzer-app; по умолчанию ESKA_BSL_ANALYZER или PATH
      --timeout <TIMEOUT>          Таймаут каждого процесса анализатора в секундах [default: 60]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-project-validate"></a>

<details>
<summary>eska-project validate: синтаксис и все параметры</summary>

```text
Проверить манифесты и корневые дескрипторы Designer XML

Использование: eska-project validate [OPTIONS]

Параметры:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --workspace              Выбрать все проекты workspace
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-project-context"></a>

<details>
<summary>eska-project context: синтаксис и все параметры</summary>

```text
Определить контекст выбранного приложения

Использование: eska-project context [OPTIONS]

Параметры:
      --application <ПРИЛОЖЕНИЕ>  Выбрать приложение, объявленное в eska.toml
      --lang <ЛОКАЛЬ>             Выбрать язык интерфейса
      --format <ФОРМАТ>           Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>     Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -h, --help                      Показать справку
```

</details>

<a id="command-eska-project-new"></a>

<details>
<summary>eska-project new: синтаксис и все параметры</summary>

```text
Создать проект Designer XML без инициализации Git

Использование: eska-project new [OPTIONS] --type <ТИП> <КАТАЛОГ>

Параметры:
      --application <ПРИЛОЖЕНИЕ>   Выбрать приложение, объявленное в eska.toml
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --type <ТИП>                 Тип проекта: configuration, extension, processing, report
      --source <КАТАЛОГ>           Каталог исходников относительно корня проекта (автопоиск: корень или src) [default: src]
      --platform-version <ВЕРСИЯ>  Необязательная точная версия платформы сборки; сохраняется как данные, платформа не запускается
      --format <ФОРМАТ>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-project-init"></a>

<details>
<summary>eska-project init: синтаксис и все параметры</summary>

```text
Подключить существующую выгрузку Designer XML без инициализации Git

Использование: eska-project init [OPTIONS] [КАТАЛОГ]

Параметры:
      --application <ПРИЛОЖЕНИЕ>   Выбрать приложение, объявленное в eska.toml
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --source <КАТАЛОГ>           Каталог исходников относительно корня проекта (автопоиск: корень или src)
      --name <ИМЯ>                 Имя workspace-проекта; по умолчанию имя его каталога
      --platform-version <ВЕРСИЯ>  Необязательная точная версия платформы сборки; сохраняется как данные, платформа не запускается
      --format <ФОРМАТ>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>

Тип существующей выгрузки определяется по корневому Designer XML.
Заимствованная `Configuration` с `ObjectBelonging=Adopted` распознаётся как
расширение и при отсутствующем `ConfigurationExtensionPurpose`. Исходные XML
при `init` не переписываются; запуск платформы для определения типа не требуется.

<a id="command-eska-project-metadata"></a>

<details>
<summary>eska-project metadata: синтаксис и все параметры</summary>

```text
Чтение, изменение и создание метаданных Designer XML

Usage: eska-project metadata [OPTIONS] <COMMAND>

Commands:
  create           Создать объект метаданных, при необходимости на основе существующего.
  create-module    Создать отсутствующий модуль поддерживаемой роли
  copy             Создать полную копию объекта с новыми идентификаторами, модулями и ресурсами.
  delete           Удалить объект и его файлы после проверки входящих ссылок.
  object-schema    Получить допустимые типы объектов, ограничения и JSON Schema запросов.
  form-schema      Получить структуру формы, свойства и события для редактирования.
  form-edit        Проверить или применить семантические изменения формы из JSON.
  recover          Проверить или восстановить прерванную запись метаданных, включая создание начальной конфигурации.
  list             Найти объекты в выбранном родителе без рекурсивного обхода конфигурации.
  add              Добавить реквизит или табличную часть через общую модель метаданных
  create-schema    Прочитать доступные операции создания, владельцев, типы и ограничения
  inspect          Прочитать свойства и схему допустимых изменений
  types            Получить допустимые типы существующего поля
  choices          Получить допустимые ссылки на метаданные для существующего поля
  check            Проверить изменение свойства и показать заменяемые байты
  apply            Применить одно проверенное изменение свойства
  rename-preview   Показать план переименования и сомнительные ссылки без записи файлов
  rename-apply     Применить проверенный план переименования по снимку источников
  rename-recovery  Проверить или восстановить прерванное переименование

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-project-metadata-create"></a>

<details>
<summary>eska-project metadata create: синтаксис и все параметры</summary>

```text
Создать объект метаданных, при необходимости на основе существующего.

Usage: eska-project metadata create [OPTIONS]

Options:
      --kind <KIND>                 Ключ типа из object-schema, например catalog, document или common-module.
      --lang <ЛОКАЛЬ>               Выбрать язык интерфейса
      --name <NAME>                 Имя нового элемента в конфигурации
      --project-dir <КАТАЛОГ>       Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --parent <PARENT>             Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --template <TEMPLATE>         Идентификатор существующего объекта того же типа, используемого как образец.
      --set <PROPERTY=VALUE>        Явное значение скалярного свойства; можно повторять. Идентификаторы менять нельзя.
      --reference <PROPERTY=VALUE>  Ссылка в свойстве-списке; повторяйте для нескольких значений, например RegisterRecords=AccumulationRegister.Остатки.
      --type <PROPERTY=TYPE>        Тип в свойстве-списке; повторяйте для составного типа, например Source=cfg:DocumentObject.Заказ или Type=xs:string.
      --input <INPUT>               Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>                 Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                     Вернуть подготовленный план создания без записи исходников
      --no-input                    Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                        Показать справку
  -p, --project <PROJECT>           Выбрать один проект рабочего пространства
      --format <FORMAT>             Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-copy"></a>

<details>
<summary>eska-project metadata copy: синтаксис и все параметры</summary>

```text
Создать полную копию объекта с новыми идентификаторами, модулями и ресурсами.

Usage: eska-project metadata copy [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>               Выбрать язык интерфейса
      --source <SOURCE>             Идентификатор копируемого объекта из metadata list или inspect.
      --name <NAME>                 Имя нового элемента в конфигурации
      --project-dir <КАТАЛОГ>       Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --parent <PARENT>             Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --set <PROPERTY=VALUE>        Явное значение скалярного свойства; можно повторять. Идентификаторы менять нельзя.
      --reference <PROPERTY=VALUE>  Ссылка в свойстве-списке; повторяйте для нескольких значений, например RegisterRecords=AccumulationRegister.Остатки.
      --type <PROPERTY=TYPE>        Тип в свойстве-списке; повторяйте для составного типа, например Source=cfg:DocumentObject.Заказ или Type=xs:string.
      --input <INPUT>               Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>                 Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                     Вернуть подготовленный план создания без записи исходников
      --no-input                    Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                        Показать справку
  -p, --project <PROJECT>           Выбрать один проект рабочего пространства
      --format <FORMAT>             Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-delete"></a>

<details>
<summary>eska-project metadata delete: синтаксис и все параметры</summary>

```text
Удалить объект и его файлы после проверки входящих ссылок.

Usage: eska-project metadata delete [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --object <OBJECT>        ID объекта из metadata inspect; по умолчанию корневой объект
      --force                  Разрешить удаление с обнаруженными входящими ссылками; ссылки останутся неразрешёнными.
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --yes                    Подтвердить удаление без интерактивного запроса. Не заменяет проверку ссылок и --force.
      --input <INPUT>          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>            Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                Вернуть подготовленный план создания без записи исходников
      --no-input               Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-object-schema"></a>

<details>
<summary>eska-project metadata object-schema: синтаксис и все параметры</summary>

```text
Получить допустимые типы объектов, ограничения и JSON Schema запросов.

Usage: eska-project metadata object-schema [OPTIONS]

Options:
      --kind <KIND>            Ключ типа из object-schema, например catalog, document или common-module.
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --parent <PARENT>        Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-form-schema"></a>

<details>
<summary>eska-project metadata form-schema: синтаксис и все параметры</summary>

```text
Получить структуру формы, свойства и события для редактирования.

Usage: eska-project metadata form-schema [OPTIONS] --form <FORM>

Options:
      --form <FORM>            Идентификатор формы из metadata list или inspect.
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-form-edit"></a>

<details>
<summary>eska-project metadata form-edit: синтаксис и все параметры</summary>

```text
Проверить или применить семантические изменения формы из JSON.

Usage: eska-project metadata form-edit [OPTIONS]

Options:
      --input <INPUT>          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --plan <PLAN>            Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --dry-run                Вернуть подготовленный план создания без записи исходников
      --no-input               Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-recover"></a>

<details>
<summary>eska-project metadata recover: синтаксис и все параметры</summary>

```text
Проверить или восстановить прерванную запись метаданных, включая создание начальной конфигурации.

Usage: eska-project metadata recover [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-list"></a>

<details>
<summary>eska-project metadata list: синтаксис и все параметры</summary>

```text
Найти объекты в выбранном родителе без рекурсивного обхода конфигурации.

Usage: eska-project metadata list [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --parent <PARENT>        Идентификатор родителя; по умолчанию — корень проекта, для копии — исходный родитель.
      --kind <KIND>            Ключ типа из object-schema, например catalog, document или common-module.
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --search <SEARCH>        Отфильтровать объекты по части идентификатора или имени без учёта регистра.
      --limit <LIMIT>          Максимальное число объектов в ответе (1–500). [default: 100]
      --after <AFTER>          Курсор nextAfter из предыдущей страницы; не включает указанный объект.
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-add"></a>

<details>
<summary>eska-project metadata add: синтаксис и все параметры</summary>

```text
Добавить реквизит или табличную часть через общую модель метаданных

Usage: eska-project metadata add [OPTIONS] [KIND]

Arguments:
  [KIND]  Создаваемый элемент: реквизит или табличная часть [possible values: attribute, table-part]

Options:
      --lang <ЛОКАЛЬ>
          Выбрать язык интерфейса
      --owner <OWNER>
          ID существующего владельца: справочника, документа или поддерживаемой табличной части
      --name <NAME>
          Имя нового элемента в конфигурации
      --project-dir <КАТАЛОГ>
          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --type <VALUE_TYPE>
          Тип реквизита: string, number, boolean, date или reference; для составного типа используйте --input
      --reference <REFERENCE>
          ID существующего объекта метаданных для ссылочного типа
      --length <LENGTH>
          Длина строки; значения по умолчанию доступны в create-schema
      --digits <DIGITS>
          Общая разрядность числа
      --fraction-digits <FRACTION_DIGITS>
          Число знаков дробной части
      --nonnegative
          Разрешить только неотрицательные числа
      --fixed-length
          Использовать фиксированную длину строки
      --date-fractions <DATE_FRACTIONS>
          Дата, время или дата и время [possible values: date, time, date-time]
      --synonym <LANGUAGE=TEXT>
          Локализованный синоним в виде LANGUAGE=TEXT; можно повторить для нескольких языков
      --comment <COMMENT>
          Комментарий разработчика
      --snapshot <SNAPSHOT>
          Требовать снимок владельца из create-schema; для точного применения предпросмотра используйте --plan
      --input <INPUT>
          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>
          Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --interactive
          Интерактивно выбрать владельца и свойства, затем проверить изменения
      --no-input
          Отключить все вопросы; при нехватке параметров вернуть ошибку
      --dry-run
          Вернуть подготовленный план создания без записи исходников
  -h, --help
          Показать справку
  -p, --project <PROJECT>
          Выбрать один проект рабочего пространства
      --format <FORMAT>
          Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-create-schema"></a>

<details>
<summary>eska-project metadata create-schema: синтаксис и все параметры</summary>

```text
Прочитать доступные операции создания, владельцев, типы и ограничения

Usage: eska-project metadata create-schema [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --owner <OWNER>          ID существующего владельца: справочника, документа или поддерживаемой табличной части
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-version"></a>

<details>
<summary>eska-project version: синтаксис и все параметры</summary>

```text
Показать или изменить версию проекта 1С

Usage: eska-project version [OPTIONS] [COMMAND]

Commands:
  bump  Увеличить один компонент версии проекта

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --workspace              Выбрать все проекты workspace
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-project-analyzer-config-status"></a>

<details>
<summary>eska-project analyzer-config status: синтаксис и все параметры</summary>

```text
Проверить актуальность без записи файлов

Использование: eska-project analyzer-config status [OPTIONS]

Команды:

Параметры:
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --application <APPLICATION>  Выбрать приложение, объявленное в eska.toml
      --analyzer <ANALYZER>        Исполняемый файл опубликованного bsl-analyzer-app; по умолчанию ESKA_BSL_ANALYZER или PATH
      --timeout <TIMEOUT>          Таймаут каждого процесса анализатора в секундах [default: 60]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-project-analyzer-config-prepare"></a>

<details>
<summary>eska-project analyzer-config prepare: синтаксис и все параметры</summary>

```text
Проверить и подготовить выбранный контекст

Использование: eska-project analyzer-config prepare [OPTIONS]

Команды:

Параметры:
      --lang <ЛОКАЛЬ>              Выбрать язык интерфейса
      --switch-context             Сменить производный контекст после остановки всех его активных клиентов
      --project-dir <КАТАЛОГ>      Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --application <APPLICATION>  Выбрать приложение, объявленное в eska.toml
      --analyzer <ANALYZER>        Исполняемый файл опубликованного bsl-analyzer-app; по умолчанию ESKA_BSL_ANALYZER или PATH
      --timeout <TIMEOUT>          Таймаут каждого процесса анализатора в секундах [default: 60]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-project-analyzer-config-migrate"></a>

<details>
<summary>eska-project analyzer-config migrate: синтаксис и все параметры</summary>

```text
Просмотреть миграцию; применить только с токеном этого preview

Использование: eska-project analyzer-config migrate [OPTIONS]

Команды:

Параметры:
      --apply                          Применить просмотренную миграцию и сохранить исходный конфиг
      --lang <ЛОКАЛЬ>                  Выбрать язык интерфейса
      --preview-token <PREVIEW_TOKEN>  Точный токен из preview миграции
      --project-dir <КАТАЛОГ>          Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --application <APPLICATION>      Выбрать приложение, объявленное в eska.toml
      --analyzer <ANALYZER>            Исполняемый файл опубликованного bsl-analyzer-app; по умолчанию ESKA_BSL_ANALYZER или PATH
      --timeout <TIMEOUT>              Таймаут каждого процесса анализатора в секундах [default: 60]
      --format <FORMAT>                Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                           Показать справку
```

</details>

<a id="command-eska-project-metadata-inspect"></a>

<details>
<summary>eska-project metadata inspect: синтаксис и все параметры</summary>

```text
Прочитать свойства и схему допустимых изменений

Usage: eska-project metadata inspect [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --object <OBJECT>        ID объекта из metadata inspect; по умолчанию корневой объект
  -h, --help                   Показать справку
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-types"></a>

<details>
<summary>eska-project metadata types: синтаксис и все параметры</summary>

```text
Получить допустимые типы существующего поля

Usage: eska-project metadata types [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-choices"></a>

<details>
<summary>eska-project metadata choices: синтаксис и все параметры</summary>

```text
Получить допустимые ссылки на метаданные для существующего поля

Usage: eska-project metadata choices [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-check"></a>

<details>
<summary>eska-project metadata check: синтаксис и все параметры</summary>

```text
Проверить изменение свойства и показать заменяемые байты

Usage: eska-project metadata check [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-apply"></a>

<details>
<summary>eska-project metadata apply: синтаксис и все параметры</summary>

```text
Применить одно проверенное изменение свойства

Usage: eska-project metadata apply [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-rename-preview"></a>

<details>
<summary>eska-project metadata rename-preview: синтаксис и все параметры</summary>

```text
Показать план переименования и сомнительные ссылки без записи файлов

Usage: eska-project metadata rename-preview [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-rename-apply"></a>

<details>
<summary>eska-project metadata rename-apply: синтаксис и все параметры</summary>

```text
Применить проверенный план переименования по снимку источников

Usage: eska-project metadata rename-apply [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-metadata-rename-recovery"></a>

<details>
<summary>eska-project metadata rename-recovery: синтаксис и все параметры</summary>

```text
Проверить или восстановить прерванное переименование

Usage: eska-project metadata rename-recovery [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Файл запроса JSON или - для чтения stdin

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

</details>

<a id="command-eska-project-version-bump"></a>

<details>
<summary>eska-project version bump: синтаксис и все параметры</summary>

```text
Увеличить один компонент версии проекта

Usage: eska-project version bump [OPTIONS] <КОМПОНЕНТ>

Arguments:
  <КОМПОНЕНТ>  Компонент для увеличения [possible values: patch, minor, major]

Options:
  -h, --help                   Показать справку
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -p, --project <ПРОЕКТ>       Выбрать проект workspace по имени; при чтении версий можно повторять
      --workspace              Выбрать все проекты workspace
```

</details>

<a id="command-eska-project-settings-local"></a>

<details>
<summary>eska-project settings-local: синтаксис и все параметры</summary>

```text
Просмотр и редактирование локальных настроек запуска проекта

Usage: eska-project settings-local [OPTIONS] <COMMAND>

Commands:
  show      Показать текущие настройки и их ревизию
  save      Сохранить настройки из JSON-документа на стандартном вводе
  validate  Проверить сохранённые профили

Options:
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -h, --help                   Показать справку
```

</details>

<a id="command-eska-project-settings-local-show"></a>

<details>
<summary>eska-project settings-local show: синтаксис и все параметры</summary>

```text
Показать текущие настройки и их ревизию

Использование: eska-project settings-local show [ПАРАМЕТРЫ]

Параметры:
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
```

Формат `json` возвращает `settings`, `revision` и `git_protection`.

</details>

<a id="command-eska-project-settings-local-save"></a>

<details>
<summary>eska-project settings-local save: синтаксис и все параметры</summary>

```text
Сохранить настройки из JSON-документа на стандартном вводе

Использование: eska-project settings-local save [ПАРАМЕТРЫ]

Параметры:
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
```

Передайте JSON с полями `revision` и `settings` через стандартный ввод. Для первого сохранения укажите `revision: null`. В Git-проекте сохранение доступно, когда файл `.eska/settings_local.toml` исключён и отсутствует в индексе.

</details>

<a id="command-eska-project-settings-local-validate"></a>

<details>
<summary>eska-project settings-local validate: синтаксис и все параметры</summary>

```text
Проверить сохранённые профили

Использование: eska-project settings-local validate [ПАРАМЕТРЫ]

Параметры:
      --format <ФОРМАТ>        Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
```

Проверяет сохранённые профили запуска без открытия сеанса 1С.

</details>

<a id="command-eska-project-metadata-create-module"></a>

<details>
<summary>eska-project metadata create-module: синтаксис и все параметры</summary>

```text
Создать отсутствующий модуль поддерживаемой роли

Usage: eska-project metadata create-module [OPTIONS]

Options:
      --lang <ЛОКАЛЬ>          Выбрать язык интерфейса
      --owner <OWNER>          ID существующего владельца модуля
      --project-dir <КАТАЛОГ>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --role <ROLE>            Роль модуля из object-schema (например object или manager)
      --input <INPUT>          Файл запроса создания в JSON с версией схемы, либо - для stdin
      --plan <PLAN>            Сохранённый JSON-вывод add --dry-run, либо - для stdin; сохраняет рассмотренные идентификаторы
      --dry-run                Вернуть подготовленный план создания без записи исходников
      --no-input               Отключить все вопросы; при нехватке параметров вернуть ошибку
  -h, --help                   Показать справку
  -p, --project <PROJECT>      Выбрать один проект рабочего пространства
      --format <FORMAT>        Формат вывода; add использует human, прежние операции и create-schema — JSON [possible values: human, json]
```

Укажите идентификатор владельца из `metadata list` или `metadata inspect` и роль из `metadata object-schema`. С `--dry-run` можно проверить будущий путь и изменения до записи.

</details>


<a id="cli-eska-check"></a>

## eska-check

| Вход | Назначение |
| --- | --- |
| [`eska-check`](#command-eska-check-default) | Полная проверка Designer XML и BSL для CI |

<a id="command-eska-check-default"></a>

<details>
<summary>eska-check: синтаксис и все параметры</summary>

```text
eska-check 0.32.0
Полная проверка Designer XML и BSL для CI

Использование: eska-check [OPTIONS]

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --application <APPLICATION>  Выбрать приложение, объявленное в eska.toml
      --analyzer <ANALYZER>        Исполняемый файл опубликованного bsl-analyzer-app; по умолчанию ESKA_BSL_ANALYZER или PATH
      --timeout <TIMEOUT>          Таймаут каждого процесса анализатора в секундах [default: 600]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --report <REPORT>            Создать новый JSON-отчёт вне source; существующий файл сохраняется
  -h, --help                       Показать справку
  -V, --version                    Показать версию
```

</details>


<a id="cli-eska-build"></a>

## eska-build

| Вход | Назначение |
| --- | --- |
| [`eska-build`](#command-eska-build-default) | Сборка и распаковка нативных артефактов 1С |
| [`eska-build build`](#command-eska-build-build) | Собрать нативный артефакт 1С из Designer XML |
| [`eska-build run`](#command-eska-build-run) | Запустить 1С:Предприятие с профилем test |
| [`eska-build probe`](#command-eska-build-probe) | Проверить платформу и доступность адресов профиля |
| [`eska-build apply`](#command-eska-build-apply) | Предпросмотр и применение изменений Designer XML к тестовой базе |
| [`eska-build extension`](#command-eska-build-extension) | Управление расширениями в тестовой базе. |
| [`eska-build extension install`](#command-eska-build-extension-install) | Предпросмотр или установка выбранного расширения Designer XML в тестовую базу. |
| [`eska-build extension remove`](#command-eska-build-extension-remove) | Предпросмотр или удаление выбранного установленного расширения из тестовой базы. |
| [`eska-build unpack`](#command-eska-build-unpack) | Распаковать CF/CFE/EPF/ERF в новый каталог Designer XML |
| [`eska-build patch`](#command-eska-build-patch) | Собрать патч из изменений методов общих модулей между Git-версиями |
| [`eska-build import`](#command-eska-build-import) | Заменить исходники проекта из файла CF, CFE, EPF или ERF |
| [`eska-build clean`](#command-eska-build-clean) | Удалить служебную базу сборки |
| [`eska-build platform`](#command-eska-build-platform) | Найти установленные платформы 1С |
| [`eska-build config`](#command-eska-build-config) | Управлять глобальными настройками этого компьютера |
| [`eska-build platform list`](#command-eska-build-platform-list) | Показать платформы в текущем раннере |
| [`eska-build config init`](#command-eska-build-config-init) | Создать глобальный конфиг по умолчанию |
| [`eska-build config edit`](#command-eska-build-config-edit) | Открыть глобальный конфиг в редакторе |

<a id="command-eska-build-default"></a>

<details>
<summary>eska-build: синтаксис и все параметры</summary>

```text
eska-build 0.32.0
Сборка и распаковка нативных артефактов 1С

Использование: eska-build [OPTIONS] [COMMAND]

Команды:
  build      Собрать нативный артефакт 1С из Designer XML
  unpack     Распаковать CF/CFE/EPF/ERF в новый каталог Designer XML
  patch      Собрать патч из изменений методов общих модулей между Git-версиями
  import     Заменить исходники проекта из файла CF, CFE, EPF или ERF
  clean      Удалить служебную базу сборки
  platform   Найти установленные платформы 1С
  config     Управлять глобальными настройками этого компьютера
  run        Запустить 1С:Предприятие с профилем test
  probe      Проверить платформу и доступность адресов профиля
  apply      Предпросмотр и применение изменений Designer XML к тестовой базе
  extension  Управление расширениями в тестовой базе.
Параметры:
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
  -V, --version                    Показать версию
```

</details>

<a id="command-eska-build-build"></a>

<details>
<summary>eska-build build: синтаксис и все параметры</summary>

```text
Собрать нативный артефакт 1С из Designer XML

Использование: eska-build build [OPTIONS]

Параметры:
      --output <ПУТЬ>                 Путь артефакта; относительный путь считается от корня проекта или workspace; требует выбора одного проекта
      --base-configuration <ФАЙЛ_CF>  Базовая конфигурация .cf для сборки внешней обработки или отчёта
      --lang <LANG>                   Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>     Начать поиск проекта с этого каталога [default: .]
      --recreate-infobase             Всегда пересоздавать служебную базу перед сборкой
      --ibcmd <ПУТЬ>                  Явный путь к ibcmd (или ESKA_IBCMD)
      --platform-arch <АРХИТЕКТУРА>   Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <КОНТЕЙНЕР>         Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --platform-version <ВЕРСИЯ>     Версия платформы только для текущей сборки
      --select-platform               Интерактивно выбрать установленную платформу только для текущей сборки
      --format <human|json>           Формат вывода [default: human] [possible values: human, json]
  -p, --project <ПРОЕКТ>              Выбрать проект workspace по имени; параметр можно повторять
      --workspace                     Выбрать все проекты workspace
      --dry-run                       Показать полностью проверенный план сборки без создания артефактов
      --manifest                      Собрать артефакт из зафиксированного снимка и записать рядом машиночитаемый паспорт
  -h, --help                          Показать справку
```

</details>

<a id="command-eska-build-unpack"></a>

<details>
<summary>eska-build unpack: синтаксис и все параметры</summary>

```text
Распаковать CF/CFE/EPF/ERF в новый каталог Designer XML

Использование: eska-build unpack [OPTIONS] <INPUT> <DESTINATION>

Аргументы:
  <INPUT>        Исходный файл CF, CFE, EPF или ERF
  <DESTINATION>  Новый каталог XML; родительский каталог должен существовать

Параметры:
      --ibcmd <IBCMD>
          Явный путь к ibcmd (или ESKA_IBCMD)
      --lang <LANG>
          Выбрать язык интерфейса
      --platform-arch <PLATFORM_ARCH>
          Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <DISTROBOX>
          Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --project-dir <PROJECT_DIR>
          Начать поиск проекта с этого каталога [default: .]
      --platform-version <PLATFORM_VERSION>
          Версия платформы 1С; сохраняется при создании проекта, действует однократно при импорте
      --select-platform
          Интерактивно выбрать установленную платформу 1С
      --format <FORMAT>
          Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help
```

</details>

<a id="command-eska-build-patch"></a>

<details>
<summary>eska-build patch: синтаксис и все параметры</summary>

```text
Собрать патч из изменений методов общих модулей между Git-версиями

Использование: eska-build patch [OPTIONS]

Параметры:
      --base <BASE>
          Направление сравнения; по умолчанию integration_target
      --lang <LANG>
          Выбрать язык интерфейса
      --output <OUTPUT>
          Новый файл .cfe; существующие файлы не заменяются
      --dry-run
          Показать план без запуска платформы и записи файлов
      --project-dir <PROJECT_DIR>
          Начать поиск проекта с этого каталога [default: .]
      --ibcmd <IBCMD>
          Путь к ibcmd; рядом требуется 1cv8 той же установки
      --platform-arch <PLATFORM_ARCH>
          Архитектура платформы
      --distrobox <DISTROBOX>
          Контейнер Distrobox
      --platform-version <PLATFORM_VERSION>
          Версия платформы для этого запуска
      --format <FORMAT>
          Формат результата: human или json [default: human] [possible values: human, json]
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-build-import"></a>

<details>
<summary>eska-build import: синтаксис и все параметры</summary>

```text
Заменить исходники проекта из файла CF, CFE, EPF или ERF

Использование: eska-build import [OPTIONS] <ФАЙЛ>

Аргументы:
  <ФАЙЛ>  Файл 1С для распаковки; путь относительно каталога запуска команды

Параметры:
  -p, --project <ПРОЕКТ>
          Выбрать ровно один проект workspace по имени
      --force
          Подтвердить различия имени/UUID и потерю локальных исходников; проверки типа и путей сохраняются
      --lang <LANG>
          Выбрать язык интерфейса
      --dry-run
          Распаковать, проверить и показать план замены без изменения исходников проекта
      --project-dir <PROJECT_DIR>
          Начать поиск проекта с этого каталога [default: .]
      --format <FORMAT>
          Формат вывода [default: human] [possible values: human, json]
      --ibcmd <IBCMD>
          Явный путь к ibcmd (или ESKA_IBCMD)
      --platform-arch <PLATFORM_ARCH>
          Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <DISTROBOX>
          Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --platform-version <PLATFORM_VERSION>
          Версия платформы 1С; сохраняется при создании проекта, действует однократно при импорте
      --select-platform
          Интерактивно выбрать установленную платформу 1С
  -h, --help
          Показать справку
```

</details>

<a id="command-eska-build-clean"></a>

<details>
<summary>eska-build clean: синтаксис и все параметры</summary>

```text
Удалить служебную базу сборки

Использование: eska-build clean [OPTIONS]

Параметры:
  -p, --project <ПРОЕКТ>           Выбрать проект workspace по имени; параметр можно повторять
      --lang <LANG>                Выбрать язык интерфейса
      --workspace                  Удалить служебные базы всех проектов workspace
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-build-platform"></a>

<details>
<summary>eska-build platform: синтаксис и все параметры</summary>

```text
Найти установленные платформы 1С

Использование: eska-build platform [OPTIONS] <COMMAND>

Команды:
  list  Показать платформы в текущем раннере

Параметры:
  -h, --help                       Показать справку
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-build-config"></a>

<details>
<summary>eska-build config: синтаксис и все параметры</summary>

```text
Управлять глобальными настройками этого компьютера

Использование: eska-build config [OPTIONS] <COMMAND>

Команды:
  init  Создать глобальный конфиг по умолчанию
  edit  Открыть глобальный конфиг в редакторе

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-build-platform-list"></a>

<details>
<summary>eska-build platform list: синтаксис и все параметры</summary>

```text
Показать платформы в текущем раннере

Использование: eska-build platform list [OPTIONS]

Параметры:
      --ibcmd <ПУТЬ>                 Явный путь к ibcmd (или ESKA_IBCMD)
      --lang <LANG>                  Выбрать язык интерфейса
      --platform-arch <АРХИТЕКТУРА>  Каталог архитектуры платформы 1С (или ESKA_PLATFORM_ARCH)
      --distrobox <КОНТЕЙНЕР>        Контейнер Distrobox, используемый только при отсутствии ibcmd на хосте (или ESKA_DISTROBOX)
      --project-dir <PROJECT_DIR>    Начать поиск проекта с этого каталога [default: .]
      --format <human|json>          Формат вывода [default: human] [possible values: human, json]
  -h, --help                         Показать справку
```

</details>

<a id="command-eska-build-config-init"></a>

<details>
<summary>eska-build config init: синтаксис и все параметры</summary>

```text
Создать глобальный конфиг по умолчанию

Использование: eska-build config init [OPTIONS]

Параметры:
  -h, --help                       Показать справку
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-build-config-edit"></a>

<details>
<summary>eska-build config edit: синтаксис и все параметры</summary>

```text
Открыть глобальный конфиг в редакторе

Использование: eska-build config edit [OPTIONS]

Параметры:
  -h, --help                       Показать справку
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-build-run"></a>

<details>
<summary>eska-build run: синтаксис и все параметры</summary>

```text
Запустить 1С:Предприятие с профилем test

Использование: eska-build run [OPTIONS]

Параметры:
      --profile <PROFILE>          Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-build-probe"></a>

<details>
<summary>eska-build probe: синтаксис и все параметры</summary>

```text
Проверить платформу и доступность адресов профиля

Использование: eska-build probe [OPTIONS]

Параметры:
      --profile <PROFILE>          Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
      --stdin                      Проверить профиль из JSON на стандартном вводе без сохранения
```

</details>

<a id="command-eska-build-apply"></a>

<details>
<summary>eska-build apply: синтаксис и все параметры</summary>

```text
Предпросмотр и применение изменений Designer XML к тестовой базе

Использование: eska-build apply [OPTIONS]

Параметры:
      --profile <PROFILE>          Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --application <APPLICATION>  Обновить указанное приложение workspace в порядке зависимостей.
      --lang <LANG>                Выбрать язык интерфейса
      --initialize                 Предпросмотр явного полного импорта XML для нового снимка
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --confirm <CONFIRM>          Применить изменения из проверенного предпросмотра
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

Выполните первый запуск без `--confirm` для просмотра базы и состава изменений. Затем повторите команду с полученным токеном и теми же параметрами. Для первого полного импорта добавьте `--initialize`.

</details>

<a id="command-eska-build-extension"></a>

<details>
<summary>eska-build extension: синтаксис и все параметры</summary>

```text
Управление расширениями в тестовой базе.

Использование: eska-build extension [OPTIONS] <COMMAND>

Команды:
  install  Предпросмотр или установка выбранного расширения Designer XML в тестовую базу.
  remove   Предпросмотр или удаление выбранного установленного расширения из тестовой базы.

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-build-extension-install"></a>

<details>
<summary>eska-build extension install: синтаксис и все параметры</summary>

```text
Предпросмотр или установка выбранного расширения Designer XML в тестовую базу.

Использование: eska-build extension install [OPTIONS]

Параметры:
      --profile <PROFILE>          Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --confirm <CONFIRM>          Применить изменения из проверенного предпросмотра
      --lang <LANG>                Выбрать язык интерфейса
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
```

Укажите проект расширения `InfoBase` через `--project-dir` или запустите команду из его каталога. Сначала получите план для профиля `test`, затем передайте его токен в `--confirm`.

</details>

<a id="command-eska-build-extension-remove"></a>

<details>
<summary>eska-build extension remove: синтаксис и все параметры</summary>

```text
Предпросмотр или удаление выбранного установленного расширения из тестовой базы.

Использование: eska-build extension remove [OPTIONS]

Параметры:
      --profile <PROFILE>          Локальный профиль запуска проекта [default: test] [possible values: test, prod]
      --confirm <CONFIRM>          Применить изменения из проверенного предпросмотра
      --lang <LANG>                Выбрать язык интерфейса
      --force                      Явно разрешить удаление и возможную потерю данных расширения.
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

Получите план удаления для профиля `test`. Для удаления повторите команду с `--confirm` и `--force`. Данные расширения могут быть удалены вместе с ним.

</details>


### Запуск и применение изменений

`eska run [--profile test] [--format human|json]` запускает тонкий клиент из
установки 1С, выбранной в профиле проекта. Команда ждёт завершения клиента.
`eska-build run` принимает те же параметры.

`eska apply [--profile test] [--application <имя>] [--initialize] [--confirm <токен>] [--format human|json]`
показывает выбранную базу, версию платформы и список файлов. Сначала запустите
команду без `--confirm`. Для применения повторите команду с полученным токеном и
теми же параметрами. После обновления `eska-build` заново получите токен
предпросмотра. `eska-build apply` принимает те же параметры. В режиме JSON
команда отправляет события `apply_progress` с фазами `import` и `apply` в stderr.
Для частичного импорта `plan.import_files` перечисляет файлы в порядке передачи
в `ibcmd`, включая неизменённые сопутствующие файлы. В остальных режимах
`import_files` равно `null`. Поле `plan.changes` содержит изменения исходников.
При структурном частичном импорте `plan.object_changes` группирует пути
по объектам. В группе указаны `kind` (`language`, `common_form`, `role`,
`common_module` или `common_picture`),
`operation` (`added`, `renamed` или `removed`), `name`, `previous_name`
и `paths`. Для переименования `paths` включает прежние и новые пути.
Если при переименовании или удалении общей картинки в XML выбранного
компонента остаются ссылки на прежнее имя, `plan.mode` равен `unsupported`,
а `plan.picture_references` перечисляет файлы для проверки. Исправьте ссылки
и повторите предпросмотр.

В JSON-плане поле `inspection` содержит отдельные результаты сравнения
сохранённых исходников (`sources`) и фактического поколения базы (`database`)
с последним успешным применением на момент проверки плана. Оба поля принимают значения `unknown`,
`unchanged` или `changed`. `baseline_generation` и `database_generation`
показывают известные токены, `pending_apply` отмечает незавершённую операцию.
`status` принимает значения `up_to_date`, `local_changes`, `database_changed`,
`both_changed`, `unknown_baseline`, `database_unavailable` или `incomplete`.
Изменение поколения говорит об изменении базы. Список файлов описывает
только разницу сохранённых исходников с применённым снимком.
При отказе в предпросмотре из-за снимка, поколения или незавершённого применения
ответ содержит `review.inspection` и `review.changes`. При неизвестном снимке
`review.changes` равно `null`. Коды ошибок и коды выхода сохраняются.

Первый запуск для базы требует отдельного предпросмотра с `--initialize`, который
импортирует всю конфигурацию Designer XML. После успеха в `.eska` сохраняются
контрольные суммы файлов, идентичность дескрипторов и поколение конфигурации.
Частичный режим передаёт существующие BSL, правки свойств языка и форм,
а также существующие XML-макеты управляемых форм. Для свойств справочника
с набором файлов управляемых форм план включает дескриптор справочника,
дескрипторы и макеты его форм. Сопутствующие файлы видны перед подтверждением.
Для XML сверяются идентичность и структура дескриптора с применённым снимком.
Если снимок создан версией без этих сведений, их записывает подтверждённый
полный импорт. Смена ветки учитывается по байтам сохранённых файлов.
Частичный импорт поддерживает добавление, удаление и переименование языков
и общих управляемых форм с XML-макетом и необязательным модулем BSL.
В плане сверяются записи `ChildObjects`, наличие всех файлов объекта и UUID.
При удалении в список
импорта входит неизменённый дескриптор языка по умолчанию. Для этого
сценария нужен применённый снимок со сведениями о корневом дескрипторе.
Изменения других частей `Configuration.xml`, формы с дополнительными
файлами и неподдержанные структурные изменения требуют предпросмотра
полного импорта.

Для общей картинки основной конфигурации или собственного объекта установленного
расширения частичный импорт передаёт `Ext/Picture.xml` и единственный файл
из `Ext/Picture/`, на который ссылается XML. Поддерживаются имена `Picture.svg`,
`Picture.png`, `Picture.bmp`, `Picture.gif`, `Picture.ico`, `Picture.jpg`,
`Picture.tif`, `Picture.zip` и `Picture` без расширения. При смене формата
план показывает удаление старого файла и добавление нового. Импорт получает
обновлённый `Ext/Picture.xml` и новый файл. Изменение свойств добавляет
дескриптор картинки. Добавление и переименование по UUID
передают весь набор с `Configuration.xml`. Удаление передаёт `Configuration.xml`
с неизменённым дескриптором языка или роли по умолчанию.

Для уже заимствованной по имени картинки с тем же UUID частичный план
поддерживает замену картинки, смену формата, очистку и возврат к наследованию.
Замена передаёт два ресурсных файла. Если `Picture` остаётся `Extended`, удаление обоих
ресурсов очищает картинку. Удаление состояния `Picture` возвращает наследование.
Эти переходы передают дескриптор картинки. Восстановление переопределения
из наследуемого состояния передаёт дескриптор и два ресурсных файла. Для
изменения состояния нужен применённый снимок с подтверждённой привязкой.
Изменение UUID, привязки и неподдержанные наборы ресурсов требуют
предпросмотра полного импорта.
Посторонний импорт, частичная ошибка и изменение исходников после предпросмотра
блокируют продолжение по прежнему токену. После ошибки проверьте состояние базы
перед новым предпросмотром полного импорта.

`eska extension install [--profile test] [--confirm <токен>] [--format human|json]`
показывает план установки отсутствующего расширения `InfoBase` из Designer XML
в тестовую базу основной конфигурации. `eska-build extension install` принимает
те же параметры. Запускайте команду из проекта расширения или укажите его через
`--project-dir`. В Explorer используйте команду «Установить расширение в тестовую
базу». План содержит базу, имя, UUID, файлы и поколения установленных зависимостей.

Подтверждение под общей блокировкой `apply` сверяет снимок файлов, поколение
основной конфигурации, зависимости и нативный список расширений. Уже занятое
имя или UUID блокирует установку. Внешние операции с базой выполняйте отдельно
от подтверждения ESKA. Импорт и применение показываются как отдельные фазы.
Снимок `apply` сохраняется после успешного применения и проверки UUID. Если
расширение появилось после прерванной установки, завершите обновление через
подтверждённый `apply --initialize`. В JSON `kind` равен `extension_install`,
`status` принимает `preview`, `installed` или `error`.

`eska extension remove [--profile test] [--confirm <токен> --force] [--format human|json]`
предварительно проверяет удаление установленного расширения `InfoBase` из
тестовой базы основной конфигурации. `eska-build extension remove` принимает
те же параметры. Запустите команду из каталога проекта расширения или передайте
его через `--project-dir`. Сначала получите план, затем повторите команду с
его токеном и `--force`. При удалении можно безвозвратно потерять данные
расширения. Explorer предоставляет отдельную команду «Удалить расширение из
тестовой базы» с выбором проекта, предпросмотром и явным подтверждением.

План содержит целевую базу, версию платформы, имя и UUID расширения и
поколения основной конфигурации и расширения. Перед удалением CLI сверяет
список установленных расширений и точное содержимое текущей выгрузки под
той же блокировкой цели, что и `apply`. Установленная зависимая часть
выбранного workspace-приложения останавливает удаление. Блокировка
координирует операции ESKA. Внешние операции с базой следует выполнять
отдельно от подтверждения ESKA.

При успешном удалении CLI подтверждает отсутствие расширения и очищает его
локальный снимок `apply`. Если платформа удалила расширение, а сохранение
результата прервалось, следующий предпросмотр предложит только очистку
оставшегося локального состояния. В JSON `kind` равен `extension_remove`,
`status` принимает `preview`, `removed` или `error`. При восстановлении
`plan.extension_generation` равен `null`, `plan.pending_operation` равен `true`.

Чтобы обновить установленное расширение, укажите каталог проекта расширения
через `eska-build --project-dir <каталог> apply` или выберите этот проект в
Explorer. Workspace-приложение должно определяться однозначно и содержать
проект основной конфигурации и расширение с областью установки `InfoBase`.
Команда использует настройки
`test` основной конфигурации и проверяет имя и UUID установленного расширения
и его объявленных зависимостей. Снимок каждого расширения хранится отдельно
в `.eska` основной конфигурации.

Если после ошибки чтения метаданных список платформы подтверждает отсутствие
расширения, CLI возвращает `extension_not_installed`. В JSON `error.extension`
содержит имя отсутствующего расширения, в том числе зависимости. Ошибка
получения списка или невозможность прочитать установленное расширение даёт
`extension_unavailable`. Несовпадение UUID имеет код `extension_identity_mismatch`.

Для первого применения нужен предпросмотр с `--initialize`. Он импортирует
исходники только выбранного установленного расширения. После создания снимка
частично применяются изменения существующих BSL и свойств существующей роли,
если у роли нет отдельных файлов. Для XML сверяются UUID и структура дескриптора.
Если снимок ещё не содержит этих сведений либо XML-изменение выходит за границы
поддержанного сценария, нужен предпросмотр полного импорта.
Команда обновляет по одному установленному расширению за раз. Для расширений
в JSON-плане появляется `plan.extension` с полями `name` и `uuid`.
`plan.inspection.configuration_generation` содержит поколение основной
конфигурации, а `dependency_generations` при наличии зависимостей содержит их
поколения по именам из Designer XML. Изменение любого из этих поколений после
предпросмотра требует нового подтверждения.

Чтобы применить всё приложение из workspace, передайте `--application <имя>`.
Имя берётся из `workspace.members`. Сначала применяется основная конфигурация,
затем установленные расширения `InfoBase` в порядке объявленных зависимостей.
План показывает состояние и файлы каждого компонента. Операции с внешними
обработками и отчётами, установка и удаление расширений в этот план не входят.

Подтверждение связывает планы всех компонентов. ESKA перепроверяет их и
последовательно применяет под общей блокировкой базы. При сбое применённые
компоненты сохраняют свои снимки. Для продолжения запросите новый план.
С `--initialize` полный импорт предлагается только там, где частичный
импорт невозможен или предыдущая операция оставила неподтверждённое состояние.
Компоненты без изменений пропускаются.

JSON-ответ для приложения имеет `kind: "application_apply"`.
В предпросмотре `plan.components` содержит планы или причины блокировки
каждого компонента, а `plan.confirmation` содержит общий токен либо `null`.
В событиях `apply_progress` поле `component` обозначает имя участника.
После выполнения `outcomes` перечисляет компоненты со статусами `applied`,
`unchanged`, `not_started` или `failed`. При сбое `error.component` указывает
компонент, на котором возникла ошибка, если он уже известен.

Для автономной тестовой базы в `admin_pid` задаётся PID уже запущенного
`ibsrv`, видимый в выбранном раннере. После перезапуска обновите PID. Профиль
`prod` заблокирован для обеих команд, а кластерный профиль поддерживает только
обычный запуск тестового клиента.

`eska-build probe --profile test|prod [--format human|json]` проверяет наличие
тонкого клиента, файла базы либо TCP-доступность настроенных адресов. Проверка
доступности не открывает сеанс 1С и не использует пароль.

С `--stdin` команда читает один профиль в JSON из стандартного ввода (до 128 КиБ).
Объект содержит `infobase`, `runner` и необязательные `debug` и `authentication`
в том же формате, что профиль в `eska-project settings-local show --format json`.
`--profile` задаёт имя профиля. Файл локальных настроек при этом не читается
и не записывается. Для недопустимого значения JSON-ответ содержит
`error.code = "invalid_field"` и `error.data` с именем профиля и поля.

<a id="cli-eska-debug"></a>

## eska-debug

Используйте `check`, чтобы сверить исходники перед отладкой `prod`.
Для DAP-сессии из проекта укажите профиль и режим `launch` или `attach`.
Команда `standalone` открывает адаптер без проекта ESKA. Список баз из
настроек запуска 1С доступен через `infobases`.

[Запуск и подключение к отладке](run-debug.md) · [Автономный DAP](eska-debug-standalone.md)

| Вход | Назначение |
| --- | --- |
| [`eska-debug`](#command-eska-debug) | Отладка 1С:Предприятия по HTTP |
| [`eska-debug check`](#command-eska-debug-check) | Проверить открытые исходники prod по выбранному workflow |
| [`eska-debug dap`](#command-eska-debug-dap) | Запустить HTTP-отладку с DAP на стандартном вводе и выводе |
| [`eska-debug standalone`](#command-eska-debug-standalone) | Запустить самостоятельную DAP-сессию без проекта ESKA |
| [`eska-debug infobases`](#command-eska-debug-infobases) | Вывести файловые и кластерные базы из списка запуска 1С в JSON |

<a id="command-eska-debug"></a>

<details>
<summary>eska-debug: синтаксис и все параметры</summary>

```text
Отладка 1С:Предприятия по HTTP

Usage: eska-debug [OPTIONS] <COMMAND>

Commands:
  check       Проверить открытые исходники prod по выбранному workflow
  dap         Запустить HTTP-отладку с DAP на стандартном вводе и выводе
  standalone  Запустить самостоятельную DAP-сессию без проекта ESKA
  infobases   Вывести файловые и кластерные базы из списка запуска 1С в JSON

Options:
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
  -h, --help                       Показать справку
  -V, --version                    Print version
```

</details>

<a id="command-eska-debug-check"></a>

<details>
<summary>eska-debug check: синтаксис и все параметры</summary>

```text
Проверить открытые исходники prod по выбранному workflow

Usage: eska-debug check [OPTIONS]

Options:
      --lang <LANG>                Выбрать язык интерфейса
      --unsaved                    В рабочей области редактора есть несохранённые документы
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
```

</details>

<a id="command-eska-debug-dap"></a>

<details>
<summary>eska-debug dap: синтаксис и все параметры</summary>

```text
Запустить HTTP-отладку с DAP на стандартном вводе и выводе

Usage: eska-debug dap [OPTIONS] --profile <PROFILE> --mode <MODE>

Options:
      --lang <LANG>                Выбрать язык интерфейса
      --profile <PROFILE>          Локальный профиль запуска проекта [possible values: test, prod]
      --mode <MODE>                Запуск клиента или подключение к существующему серверу отладки [possible values: launch, attach]
      --project-dir <PROJECT_DIR>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
      --sources <SOURCES>          Явное решение об исходниках для отладки prod [possible values: current, fast-forward, switch-main]
      --unsaved                    В рабочей области редактора есть несохранённые документы
  -h, --help                       Показать справку
```

Для `prod` при расхождении исходников выберите действие через `--sources`. Обновление возможно в чистой рабочей копии без несохранённых буферов и без расхождения истории основной ветки.

</details>

<a id="command-eska-debug-standalone"></a>

<details>
<summary>eska-debug standalone: синтаксис и все параметры</summary>

```text
Запустить самостоятельную DAP-сессию без проекта ESKA

Usage: eska-debug standalone [OPTIONS]

Options:
  -h, --help                       Показать справку
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
```

Адаптер читает параметры запуска или подключения из DAP-запроса. Проект с `eska.toml` для этого режима не требуется.

</details>

<a id="command-eska-debug-infobases"></a>

<details>
<summary>eska-debug infobases: синтаксис и все параметры</summary>

```text
Вывести файловые и кластерные базы из списка запуска 1С в JSON

Usage: eska-debug infobases [OPTIONS]

Options:
      --lang <LANG>                Выбрать язык интерфейса
      --list <LIST>                Путь к файлу списка запуска 1С
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Искать проект из указанного каталога; для clone/new/init — базовый каталог (по умолчанию — текущий) [default: .]
```

С `--list` можно прочитать конкретный файл списка запуска вместо списка пользователя по умолчанию.

</details>

<a id="cli-eska-vcs"></a>

## eska-vcs

| Вход | Назначение |
| --- | --- |
| [`eska-vcs`](#command-eska-vcs) | Git/workflow, полки и смысловое сравнение Designer XML |
| [`eska-vcs clone`](#command-eska-vcs-clone) | Клонировать и проверить существующий проект eska |
| [`eska-vcs diff`](#command-eska-vcs-diff) | Показать изменения файлов проекта |
| [`eska-vcs finish`](#command-eska-vcs-finish) | Завершить активную задачу по правилам workflow |
| [`eska-vcs history`](#command-eska-vcs-history) | Показать локальную историю commit |
| [`eska-vcs save`](#command-eska-vcs-save) | Сохранить текущие изменения проекта в историю Git |
| [`eska-vcs shelve`](#command-eska-vcs-shelve) | Сохранить незакоммиченные изменения текущей ветки в полку |
| [`eska-vcs unshelve`](#command-eska-vcs-unshelve) | Восстановить полку текущей ветки с сохранением index и рабочих файлов |
| [`eska-vcs shelves`](#command-eska-vcs-shelves) | Показать сохранённые полки всех веток репозитория |
| [`eska-vcs start`](#command-eska-vcs-start) | Начать работу над задачей по workflow проекта |
| [`eska-vcs status`](#command-eska-vcs-status) | Показать состояние проекта и workflow |
| [`eska-vcs switch`](#command-eska-vcs-switch) | Переключиться на существующую задачу или базовую ветку workflow |

<a id="command-eska-vcs"></a>

<details>
<summary>eska-vcs: синтаксис и все параметры</summary>

```text
eska-vcs 0.32.0
Git/workflow, полки и смысловое сравнение Designer XML

Использование: eska-vcs [OPTIONS] <COMMAND>

Команды:
  clone     Клонировать и проверить существующий проект eska
  diff      Показать изменения файлов проекта
  finish    Завершить активную задачу по правилам workflow
  history   Показать локальную историю commit
  save      Сохранить текущие изменения проекта в историю Git
  shelve    Сохранить незакоммиченные изменения текущей ветки в полку
  unshelve  Восстановить полку текущей ветки с сохранением index и рабочих файлов
  shelves   Показать сохранённые полки всех веток репозитория
  start     Начать работу над задачей по workflow проекта
  status    Показать состояние проекта и workflow
  switch    Переключиться на существующую задачу или базовую ветку workflow
Параметры:
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
  -V, --version                    Показать версию
```

</details>

<a id="command-eska-vcs-clone"></a>

<details>
<summary>eska-vcs clone: синтаксис и все параметры</summary>

```text
Клонировать и проверить существующий проект eska

Использование: eska-vcs clone [OPTIONS] <РЕПОЗИТОРИЙ> [КАТАЛОГ]

Аргументы:
  <РЕПОЗИТОРИЙ>  URL или локальный путь репозитория
  [КАТАЛОГ]      Новый каталог назначения (по умолчанию — имя репозитория)

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
      --remote <ИМЯ>               Имя для удалённого репозитория [default: origin]
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-vcs-diff"></a>

<details>
<summary>eska-vcs diff: синтаксис и все параметры</summary>

```text
Показать изменения файлов проекта

Использование: eska-vcs diff [OPTIONS] [РЕВИЗИЯ]...

Аргументы:
  [РЕВИЗИЯ]...  Сравнить указанную ревизию с HEAD либо две указанные ревизии

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
      --since-branch-point         Сравнивать с точкой ветвления указанных ревизий
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --raw                        Вывести компактные коды изменений без локализации
      --semantic                   Показать логические объекты, модули, методы, формы и свойства метаданных
      --format <ФОРМАТ>            Формат вывода: human или json [default: human] [possible values: human, json]
  -p, --project <ПРОЕКТ>           Выбрать проект workspace по имени; параметр можно повторять
      --workspace                  Выбрать все проекты и собственные файлы workspace
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-vcs-finish"></a>

<details>
<summary>eska-vcs finish: синтаксис и все параметры</summary>

```text
Завершить активную задачу по правилам workflow

Использование: eska-vcs finish [OPTIONS]

Параметры:
  -h, --help                       Показать справку
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

Если политика удаляет ветку задачи, перед завершением восстановите её полку через `unshelve` и сохраните изменения.

</details>

<a id="command-eska-vcs-history"></a>

<details>
<summary>eska-vcs history: синтаксис и все параметры</summary>

```text
Показать локальную историю commit

Использование: eska-vcs history [OPTIONS]

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
  -n, --limit <КОЛИЧЕСТВО>         Максимальное число commit для вывода (1–1000) [default: 20]
      --format <ФОРМАТ>            Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-vcs-save"></a>

<details>
<summary>eska-vcs save: синтаксис и все параметры</summary>

```text
Сохранить текущие изменения проекта в историю Git

Использование: eska-vcs save [OPTIONS]

Параметры:
      --lang <LANG>                Выбрать язык интерфейса
  -m, --message <СООБЩЕНИЕ>        Использовать указанное сообщение commit без генерации и редактора
  -p, --project <ПРОЕКТ>           Сохранить только один проект workspace по имени
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --workspace                  Сохранить все изменения внутри workspace
      --dry-run                    Показать точный план сохранения без изменения репозитория
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-vcs-shelve"></a>

<details>
<summary>eska-vcs shelve: синтаксис и все параметры</summary>

```text
Сохранить незакоммиченные изменения текущей ветки в полку

Использование: eska-vcs shelve [OPTIONS]

Параметры:
      --dry-run                    Показать пути и состояния index/worktree без изменения репозитория
      --lang <LANG>                Выбрать язык интерфейса
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-vcs-unshelve"></a>

<details>
<summary>eska-vcs unshelve: синтаксис и все параметры</summary>

```text
Восстановить полку текущей ветки с сохранением index и рабочих файлов

Использование: eska-vcs unshelve [OPTIONS] [ID]

Аргументы:
  [ID]  ID полки; по умолчанию полка текущей ветки

Параметры:
      --dry-run                    Показать пути и состояния index/worktree без изменения репозитория
      --lang <LANG>                Выбрать язык интерфейса
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-vcs-shelves"></a>

<details>
<summary>eska-vcs shelves: синтаксис и все параметры</summary>

```text
Показать сохранённые полки всех веток репозитория

Использование: eska-vcs shelves [OPTIONS]

Параметры:
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <LANG>                Выбрать язык интерфейса
  -h, --help                       Показать справку
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-vcs-start"></a>

<details>
<summary>eska-vcs start: синтаксис и все параметры</summary>

```text
Начать работу над задачей по workflow проекта

Использование: eska-vcs start [OPTIONS] <ЗАДАЧА>

Аргументы:
  <ЗАДАЧА>  Идентификатор задачи для имени рабочей ветки

Параметры:
  -h, --help                       Показать справку
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
```

</details>

<a id="command-eska-vcs-status"></a>

<details>
<summary>eska-vcs status: синтаксис и все параметры</summary>

```text
Показать состояние проекта и workflow

Использование: eska-vcs status [OPTIONS]

Параметры:
      --format <ФОРМАТ>            Формат вывода: human или json [default: human] [possible values: human, json]
      --lang <LANG>                Выбрать язык интерфейса
  -p, --project <ПРОЕКТ>           Выбрать проект workspace по имени; параметр можно повторять
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --workspace                  Выбрать все проекты и собственные файлы workspace
  -h, --help                       Показать справку
```

</details>

<a id="command-eska-vcs-switch"></a>

<details>
<summary>eska-vcs switch: синтаксис и все параметры</summary>

```text
Переключиться на существующую задачу или базовую ветку workflow

Использование: eska-vcs switch [OPTIONS] <ЗАДАЧА|--base>

Аргументы:
  [ЗАДАЧА]  Идентификатор существующей задачи

Параметры:
      --base                       Переключиться на базовую ветку workflow без завершения задачи
      --lang <LANG>                Выбрать язык интерфейса
      --dry-run                    Показать пути и состояния index/worktree без изменения репозитория
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
```

</details>


<a id="cli-eska-fmt"></a>

## eska-fmt

| Вход | Назначение |
| --- | --- |
| [`eska-fmt`](#command-eska-fmt-default) | Настраиваемое форматирование BSL и запросов без анализатора |

<a id="command-eska-fmt-default"></a>

<details>
<summary>eska-fmt: синтаксис и все параметры</summary>

```text
Настраиваемое форматирование BSL и запросов без анализатора

Usage: eska-fmt [OPTIONS] [SCOPE]

Arguments:
  [SCOPE]  Файл или каталог объекта относительно project-dir; по умолчанию исходники проекта

Options:
      --lang <LANG>                Выбрать язык интерфейса
      --project-dir <PROJECT_DIR>  Начать поиск проекта с этого каталога [default: .]
      --member <MEMBER>            Выбранный member workspace; можно указать несколько
      --workspace                  Все members workspace
      --check                      Проверить без записи; код 1 при необходимых изменениях
      --diff                       Показать исходную и отформатированную версии без записи
      --dump-config                Полная политика с происхождением значений без записи
      --style <STYLE>              Sparse override операции в JSON
      --query-at <QUERY_AT>        Явно форматировать статический BSL-запрос по UTF-8 byte offset
      --query-index <QUERY_INDEX>  Явно форматировать XML-запрос по индексу с нуля
      --migrate-config             Предпросмотр миграции настроек в секцию format
      --apply-migration            Применить миграцию настроек с резервной копией
      --format <FORMAT>            Формат вывода: human или json [default: human] [possible values: human, json]
  -h, --help                       Показать справку
  -V, --version                    Показать версию
```

</details>
