# Создание конфигураций, объектов и форм через CLI

Человек, ИИ и Explorer используют одно Rust-ядро. CLI принимает семантические
запросы: вид объекта, владельца, свойства и операции формы. Пути файлов,
служебный XML, UUID и внутренние идентификаторы определяет backend.
Для изменения исходников установка платформы 1С не требуется.

Поддерживается Designer XML. Доступные операции и виды нужно получать через
`object-schema` для выбранного проекта и родителя. Структурные операции над
объектами расширений пока не поддержаны. Полноценный графический конструктор
форм остаётся отдельной задачей; описанное здесь редактирование формы выполняет CLI.

## Начать с пустого проекта

Для нового самостоятельного проекта:

```bash
eska new demo-metadata --type configuration --workflow trunk --platform-version '' --no-vcs
cd demo-metadata
eska metadata object-schema --format json
eska metadata create --kind configuration --name Demo --no-input --format json
```

`new` создаёт каркас проекта. Следующая команда создаёт начальную управляемую
конфигурацию с русским языком и собственными UUID, включая семь внутренних
объектов платформы. Каталог исходников должен быть пустым; допускается пустой
`.gitkeep`. Повторное создание не перезаписывает существующую конфигурацию.
Свойства конфигурации затем меняются через `metadata inspect/check/apply`.

Для существующей выгрузки используйте обычное подключение проекта, а для CF —
[`eska new --from`](user-guide.md). Не копируйте служебные UUID из другой конфигурации.

## Найти объекты и допустимые операции

```bash
eska metadata object-schema --format json
eska metadata object-schema --kind event-subscription --format json
eska metadata list --kind document --limit 50 --format json
eska metadata list --kind document --search Заём --format json
eska metadata object-schema --parent 'document:ВыдачаЗайма' --format json
eska metadata list --parent 'document:ВыдачаЗайма' --kind form --format json
eska metadata inspect --object 'document:ВыдачаЗайма'
```

`list` возвращает непосредственные объекты выбранного владельца, раскрывая
служебные группы дерева. Он не обходит рекурсивно все файлы конфигурации.
`nextAfter` передаётся следующему вызову как `--after`; лимит страницы — 1–500.
Для выбора другого проекта workspace используется `--project NAME`.
Неоднозначный выбор проекта завершается ошибкой.

`object-schema` содержит:

- `operations`, `kinds` и `kindOptions`: разрешённые операции и виды в этом контексте;
- `requiredProperties`, `referenceProperties`, `typeProperties` и `propertyCaptions`:
  обязательные поля, свойства со списками ссылок и типов, подписи из общих словарей платформы;
- `uniqueProperties`: свойства, которые копия должна изменить при совпадении
  с существующим объектом, например `RootURL` HTTP-сервиса;
- `requestSchemas`: JSON Schema запросов `create`, `copy`, `delete` и `form-edit`;
- `preparedPlans`: возможность проверить и затем применить точный сохранённый план.

В CLI ключ вида имеет форму `common-module`, `document`, `catalog`.
В JSON `request.kind` используется стабильное значение модели, например
`CommonModule`, `Document`, `Catalog`: оба значения явно указаны в `kindOptions`.
Имена объектов, идентификаторы, ключи и значения JSON не переводятся.
Подписи возвращаются сразу для `ru-RU` и `en-US`.

Создание, копирование и переименование используют общую проверку имён.
Имена стандартных полей, например `Ref`/`Ссылка` и `Period`/`Период`,
зарезервированы у соответствующего владельца даже при отключении поля.
Реквизиты, табличные части, измерения, ресурсы, реквизиты адресации и признаки
учёта одного владельца используют общее пространство имён. Признаки учёта
субконто, команды и формы имеют отдельные пространства имён.

После выбора вида запросите `object-schema --kind KIND`. В `initialFields`
появятся допустимые варианты обязательных полей и полей с уникальностью:
типы источников подписки, ссылки на существующие документы, задачи и реквизиты,
группы команд и HTTP-методы.
`kind` поля определяет форму значения, `multiple` — возможность выбрать
несколько вариантов, `options` содержит машинные ключи и обе подписи.
Варианты вычисляются только для выбранного вида, общий запрос схемы не
перечитывает все объекты ради всех возможных списков.

## Создать объект и его реквизиты

```bash
eska metadata create --kind document --name Order --no-input --format json
eska metadata add attribute --owner 'document:Order' --name Comment \
  --type string --length 100 --no-input --format json
eska metadata add table-part --owner 'document:Order' --name Lines --no-input --format json
eska metadata add attribute --owner 'document:Order/tabular-section:Lines' --name Amount \
  --type number --digits 15 --fraction-digits 2 --no-input --format json
eska metadata create --kind form --parent 'document:Order' --name Main --no-input --format json
```

Уточняйте фактический `objectId` по результату каждой операции: его можно сразу
передать следующей команде. Для табличных частей используйте идентификатор,
возвращённый `add`, а не самостоятельно собранный путь.
Подробности типов, квалификаторов и составных типов — в
[руководстве по реквизитам](metadata-creation.md).

`--set PROPERTY=VALUE` задаёт проверяемое скалярное свойство; параметр можно
повторять. `--reference PROPERTY=VALUE` задаёт элементы списка ссылок:

```bash
eska metadata create --kind functional-option-parameter --name UseItems \
  --reference Use=Catalog.Items --no-input --format json
```

Ссылка должна существовать и подходить к свойству. `Name`, UUID и служебные
identity нельзя подменить через свойства. Некоторые объекты требуют дополнительных
полей, например обработчика или группы команды; они перечислены в схеме.

`--type PROPERTY=TYPE` задаёт состав типа свойства. При повторении параметра
получается составной тип; `xs:` обозначает встроенные типы, `cfg:` — типы текущей
конфигурации. Значения проверяются общим редактором типов:

```bash
eska metadata create --kind defined-type --name Identifier \
  --type Type=xs:string --type Type=xs:decimal --no-input --format json
eska metadata create --kind event-subscription --name BeforeOrderWrite \
  --type Source=cfg:DocumentObject.Order --set Event=BeforeWrite \
  --set Handler=CommonModule.Handlers.BeforeOrderWrite --no-input --format json
```

Объект `Order` и общий модуль обработчика должны существовать. CLI не создаёт
тело обработчика по имени. Для всех документов используется
`Source=cfg:DocumentObject`: backend записывает нативный `TypeSet`.
В JSON то же значение задаётся через `propertyTypes` как список
`{namespace:"http://v8.1c.ru/8.1/data/enterprise/current-config",name:"DocumentObject.Order"}`.

## Полная копия

```bash
eska metadata copy --source 'document:Order' --name OrderCopy --dry-run --no-input --format json > copy-plan.json
eska metadata copy --plan copy-plan.json --no-input --format json
```

Можно также использовать `create --kind document --template 'document:Order'`.
Копируются дочерние объекты, формы, модули и ресурсы. Собственные UUID и
идентификаторы генерируемых типов заменяются согласованно; внутренние ссылки
перенаправляются на копию. Внешние ссылки сохраняют смысл. Общие идентификаторы
классов платформы не становятся случайными UUID.

Непрозрачные ресурсы сохраняются побайтно. Исходные BOM, окончания строк,
комментарии и незатронутые фрагменты XML/BSL сохраняются.
Переписываются распознанные семантические ссылки. Произвольные строковые GUID
и динамически вычисляемые ссылки в BSL автоматически не заменяются; их смысл
проверяет разработчик.

Свойства с отдельной уникальностью нужно менять явно:

```bash
eska metadata copy --source 'http-service:Api' --name ApiCopy --set RootURL=api-copy --no-input --format json
eska metadata copy --source 'web-service:Api' --name ApiCopy --set DescriptorFileName=ApiCopy.1cws --no-input --format json
eska metadata copy --source 'http-service:Api/url-template:Route' --name AnotherRoute --set Template=/another --no-input --format json
eska metadata copy --source 'http-service:Api/url-template:Route/method:Read' --name Write --set HTTPMethod=POST --no-input --format json
```

Шаблон `Template` уникален внутри своего HTTP-сервиса, а `HTTPMethod` — внутри
своего шаблона URL. У разных владельцев одинаковые значения разрешены.
Регистр символов шаблона учитывается: `/path` и `/PATH` различаются.
Если свойство отсутствует в XML, платформа использует `Template=/*` и
`HTTPMethod=GET`; проверка коллизий учитывает эти значения. Явно заданный
пустой `Template` остаётся пустым и также не может повторяться у того же владельца.
При конфликте CLI возвращает `object_property_conflict` до записи файлов.

Копия регистра не включается автоматически в движения чужих документов.
Список `RegisterRecords` меняется отдельным проверяемым запросом
[`change.kind = references`](metadata-editing.md). Аналогично явно изменяется
состав журналов, последовательностей и подсистем.

Для метода HTTP-сервиса нужен `Handler`, для операции Web-сервиса —
`ProcedureName`, для колонки журнала — `References`. Эти поля также перечислены
в схеме выбранного вида. Новый параметр Web-сервиса получает `XDTOValueType`
со значением `xs:string`. Произвольный тип XDTO пока не имеет отдельного
редактора QName; полное копирование сохраняет исходный тип и его пространства имён.

## Запросы и планы для ИИ

Тот же запрос можно передать файлом или через stdin:

```json
{
  "schemaVersion": 1,
  "request": {
    "operation": "copy",
    "source": "document:Order",
    "name": "OrderCopy",
    "propertyValues": {},
    "propertyReferences": {},
    "propertyTypes": {}
  }
}
```

```bash
eska metadata copy --input request.json --dry-run --no-input --format json > plan.json
eska metadata copy --plan plan.json --no-input --format json
```

`--input` и `--plan` нельзя смешивать с флагами, меняющими смысл запроса.
`--format json` никогда не включает интерактивные вопросы; для скриптов также
рекомендуется явный `--no-input`. Семантический запрос ограничен 1 МиБ,
сохранённый план с перечнем файлов и фрагментами изменений — 64 МиБ.
Неизвестные JSON-поля отклоняются, включая вложенные ключи свойств и значений:
опечатка в запросе не должна превращаться в молча проигнорированную настройку.

Оболочка ответа: `{schema_version:1,result:...}` либо
`{schema_version:1,error:{kind,details}}`. Успех — exit code 0, ошибка операции —
1, ошибка синтаксиса CLI — 2. JSON операции не зависит от `--lang`.

План содержит `prepared` с запросом, снимком входов, выделенными идентификаторами
и ожидаемым снимком результата. `files` перечисляет изменения с размерами и
SHA-256; `inputs` — прочитанные зависимости; `references` — входящие ссылки
и места, требующие проверки при удалении. Большие ресурсы не вкладываются в JSON.

Применение заново вычисляет результат из семантического запроса и проверяет
снимки. Отредактировать показанный путь или список файлов, чтобы записать
произвольный файл, нельзя. Повторный `--dry-run --plan` сохраняет выделенные UUID.
После обрыва ответа сначала выполните `inspect`: мутации нельзя автоматически
повторять. Ошибка `object_committed_refresh_required` означает, что запись уже
произошла и требуется обновление сессии.

## Изменение формы

```bash
eska metadata form-schema --form 'document:Order/form:Main' --format json
eska metadata form-edit --input form-edit.json --dry-run --no-input --format json > form-plan.json
eska metadata form-edit --plan form-plan.json --no-input --format json
```

`form-schema` возвращает `snapshot`, доступность записи, узлы с их целями,
текущие свойства, каталоги видов элементов и определений. Каталоги свойств
и событий общие с просмотрщиком; они не копируются для каждого узла.
В `request.snapshot` подставляется снимок именно этой формы.

Пример добавления вложенных элементов:

```json
{
  "schemaVersion": 1,
  "request": {
    "form": "document:Order/form:Main",
    "snapshot": "СНИМОК ИЗ form-schema",
    "operations": [{
      "kind": "addElement",
      "parent": {"kind": "root"},
      "element": {
        "kind": "UsualGroup", "name": "Totals",
        "children": [{"kind": "InputField", "name": "Amount"}]
      }
    }]
  }
}
```

Для рабочего поля также создайте реквизит формы и задайте `DataPath`.
Поддержаны `addElement`, `copyElement`, `moveElement`, `remove`, `setProperty`,
`removeProperty`, `setEvent` и `addDefinition` для реквизитов, команд и параметров.
Колонка табличного реквизита адресуется как `{"kind":"attribute","name":"Rows.Amount"}`.
`before` позволяет вставить элемент перед выбранным соседом.
Один запрос содержит не более 1024 операций; пустой список проверяет снимок
и возвращает `changed:false` без записи.

Локальные ID выделяет ядро и возвращает в `assignedIds`. Не задавайте их вручную
при создании. Перемещение сохраняет identity; копирование выделяет новые ID
и обновляет внутренние ссылки. Удаление проверяет зависимости в форме и её модуле.
План перечисляет эти входы в `inputs`; `prepared.sourceSnapshot` включает их
содержимое. Изменение модуля между проверкой удаления и записью даёт конфликт.
Форму целиком копирует обычная команда `copy`, включая модуль и ресурсы.

## Удаление и восстановление

```bash
eska metadata delete --object 'document:OrderCopy' --dry-run --no-input --format json > delete-plan.json
eska metadata delete --plan delete-plan.json --no-input --format json
```

При прямом неинтерактивном удалении нужен `--yes`. Он подтверждает намерение,
но не обходит ссылки. `--force` отдельно разрешает удаление с обнаруженными
входящими ссылками; оставшиеся ссылки не исправляются молча.
В интерактивном режиме перед записью показывается план и требуется `y/n` или `д/н`.

Общий журнал хранит исходные и новые байты многофайловой операции. Пока требуется
восстановление, другие операции записи заблокированы. Проверка доступна даже
если прерванное создание ещё не оставило читаемый `Configuration.xml`:

```json
{"schemaVersion":1,"action":"inspect"}
```

```bash
eska metadata recover recovery.json --format json
```

Для восстановления передайте `action: "restore"` и `snapshot` из проверки.
Файлы, изменённые пользователем после прерывания, не затираются; конфликт требует
разбора. Завершённая операция при восстановлении только очищает журнал.
Скалярный Undo/Redo не заменяет удаление объекта и проверку зависимостей.

Команды меняют исходники. Применение к тестовой базе — отдельная операция.
Результаты и ограничения платформенной проверки описаны в
[отчёте нативной приёмки](measurements/metadata-authoring-native-2026-11-16.md).
