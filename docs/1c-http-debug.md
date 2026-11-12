# HTTP-отладка 1С

VS Code обменивается с `eska-debug` сообщениями DAP по stdin/stdout. Rust-адаптер
соединяется с `dbgs` по HTTP. Клиент 1С подключается к своей информационной базе
через настроенный runner. Адрес базы и `debug.url` задаются независимо.

## Контракт с dbgs

Проверенная версия платформы — 8.3.27.2342. Запросы идут методом POST
на `/e1crdbg/rdbg?cmd=…`. XML передаётся в UTF-8. Корневой `request` без
пространства имён, его поля — в `debugRDBGRequestResponse`. Ответ
`response` находится в `debugBaseData`; вложенные поля принадлежат
пространствам операции, идентификаторов, точек или вычислений. Сервер может
добавить UTF-8 BOM и прислать пустое тело для подтверждения или пустого опроса.

| Действие DAP | Обмен с dbgs | Подтверждённое поведение |
| --- | --- | --- |
| initialize | локальный обмен | Адаптер объявляет конфигурирование, Evaluate и отмену ожидающего результата |
| launch / attach | `attachDebugUI`, `setAutoAttachSettings` | `registered` открывает сессию. Файловый alias — `DefAlias`. Новый клиент запускается после `configurationDone` |
| setBreakpoints | `setBreakpoints`, `bpWorkspace` | Пересылается весь набор точек, пустой список снимает точки модуля |
| Выбор предмета | `getDbgTargets`, `clearBreakOnNextStatement`, `attachDetachDbgTargets` | Новый предмет поступает также через `targetStarted`. Существующий можно выбрать по ID из списка |
| threads / stackTrace | `pingDebugUIParams&dbgui=…` | `callStackFormed` содержит кадры от вызывающего к текущему. DAP разворачивает порядок |
| scopes / variables | `evalLocalVariables` или `evalExpr` | Локальные переменные относятся к выбранному `stackLevel`. Составные значения раскрываются отдельным запросом |
| evaluate | `evalExpr` | Запрос включает UUID предмета, уровень кадра и UUID вычисления. Результат приходит синхронно либо событием `exprEvaluated` с тем же UUID |
| continue, next, stepIn, stepOut | `step` с `Continue`, `Step`, `StepIn`, `StepOut` | Ответ подтверждает выполнение команды. Следующая остановка приходит отдельным событием |
| disconnect | продолжение остановленных предметов, `detachDebugUI` | Завершает регистрацию и освобождает остановленные предметы. Адаптер управляет процессом клиента, только если сам его запустил |

У идентификатора UI время жизни одной регистрации, у идентификатора предмета —
одного сеанса 1С, у DAP `frameId` и `variablesReference` — одной остановки
этого предмета. После продолжения старые ссылки недействительны. `pres` и
`exceptionStr` передаются в Base64 с текстом UTF-8. Ошибки транспорта
локализованы. Результаты пользовательских выражений возвращаются в DAP.

Точки и стек сопоставляются с Designer XML через UUID владельца из общей
модели проекта и идентификатор свойства модуля платформы. Для выбранного
проекта расширения дополнительно используется его имя. Неоднозначные UUID
не разрешаются автоматически. Приёмка сопоставления на живом стенде
охватывает конфигурацию с модулем управляемого приложения. Расширения,
внешние обработки, кластерная публикация, VPN, Windows и macOS требуют
отдельных стендов. Условные точки, logpoints и pause пока отсутствуют.

Explorer использует `eska/listDebugTargets` с ответом
`{ "targets": [{ "id": "...", "kind": "...", "session": "...", "user": "..." }] }`
и `eska/attachDebugTarget` с параметром `{ "id": "..." }`. Для основной
ветки `prod` адаптер вызывает установленный `eska-vcs` по внутреннему
контракту компонентов. Git-проверка и HTTP-сессия используют разные процессы
только при проверке Git. Запущенный отладчик обращается к `dbgs` сам.

См. [спецификацию DAP](https://microsoft.github.io/debug-adapter-protocol/specification).

## Воспроизведение

В собственный уникальный каталог соседнего `eska-playground` скопируйте
`tests/fixtures/forms/native-dendrogram/source` как `src`. Добавьте
`src/Ext/ManagedApplicationModule.bsl`.

```bsl
Процедура ПриНачалеРаботыСистемы()
    A = 41;
    B = Увеличить(A);
    Сообщить(B);
КонецПроцедуры

Функция Увеличить(Число)
    Результат = Число + 1;
    Возврат Результат;
КонецФункции
```

Подготовьте манифест проекта типа `configuration` и файловую базу командами
`ibcmd infobase create --data=base`, `ibcmd infobase config import --data=base src`
и `ibcmd infobase config apply --data=base`. Укажите `base/db-data`, свой
runner и локальный HTTP endpoint в профиле `test`. Запустите `dbgs` на этом
порту. Для графического клиента требуется рабочий дисплей.

```sh
export ESKA_TEST_ROOT="$(realpath ../eska-playground)"
export ESKA_DEBUG_LIVE_PROJECT="<абсолютный путь к своему проекту>"
cargo test -p eska-debug direct_http_breakpoint_and_selected_frame -- --ignored
ESKA_DEBUG_ROUNDS=8 ESKA_DEBUG_TIMINGS=1 cargo test --release -p eska-debug \
  --test native_dap -- --ignored --nocapture
```

Для серии с одним профилем `prod` назовите профиль `prod` в собственном демо и
задайте `ESKA_NATIVE_PROFILE=prod`. Тест использует явное решение
`--sources current`. В тесте Explorer `test/debug-http-host.cjs` стенд
передаёт `ESKA_HOST_HTTP_FIXTURE` с путями `binary`, `settings` и `source`.
Для проверки attach до запуска теста запустите отдельный клиент с
`/DEBUG -http -attach /DEBUGGERURL <URL dbgs>`.

`ESKA_DEBUG_TIMINGS` выводит имена HTTP-операций и миллисекунды в stderr.
DAP-время включает обработку адаптера, HTTP и ожидание асинхронного результата.
`stackTrace` отдаётся из события остановки и не требует отдельного запроса.
