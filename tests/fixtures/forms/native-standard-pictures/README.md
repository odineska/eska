# Стандартные картинки Linux 8.3.27.2342

Первый набор включает `MoveUp`, `MoveDown`, `CloneListItem`, `CloneObject`,
`Find`, `SearchControl`. Три формы содержат картинки на белом, чёрном
и красном фоне `UsualGroup`. `White.xml`, `Black.xml`,
`Red.xml` — исходные байты native-выгрузки, включая BOM/CRLF. Метаданные в
`source/` позволяют собрать конфигурацию в собственном каталоге playground:
перенести формы в CommonForms/P_AWhite, P_BBlack, P_CColor/Ext/Form.xml.
Не изменять существующие пользовательские базы.

Второй набор включает `Refresh`, `Post`, `EventLog`, `CheckAll`, `UncheckAll`,
`DeleteListItemDirectly`. Для него используются импортированные в Конфигуратор
`CommandWhite.xml`, `CommandBlack.xml` и `CommandRed.xml`. Для повторного
снимка использовать эти три XML в тех же каталогах отдельной копии.

Из чёрного фона взят premultiplied RGB; разность белого и чёрного даёт alpha.
Независимый красный фон совпал во всех 3 072 пикселях. Всего измерено 9 216
пикселей двенадцати областей 16×16 на трёх фонах. Для `White`, `Black`, `Red`
снимки сделаны в en, для `CommandWhite`, `CommandBlack`, `CommandRed` в ru.
В VSIX обе группы проверены в обеих локалях. Полная геометрия PictureDecoration
проверяется отдельно.

В XML нет `StdPicture.Copy`: native import отклоняет эту ссылку. Автоматическое
копирование DynamicList совпадает с `CloneListItem`; `CloneObject` отличается.
`Find` и `SearchControl` в этом размере совпадают. Кнопки DynamicList проверены
также на native-градиенте; метрики содержат полную панель со строкой тени.
Автоматические стрелки таблицы отличаются от явных MoveUp/MoveDown;
эта разновидность остаётся незавершённой.

[Отчёт Linux](../../../../docs/measurements/t156-native-standard-pictures-linux.json)
для первого набора проверяет по 13 824 пикселя картинок и по пять полных панелей DynamicList:
61 290 пикселей en / 56 430 ru, включая dirty XML и отдельное окно. Масок и
допусков нет. Полные остальные панели, вариант поиска 11 px, другие свойства,
DPI и Windows остаются открытыми.

[Отчёт для картинок команд](../../../../docs/measurements/t158-command-pictures-linux.json)
содержит результаты расширенной матрицы и проверки формы «Заказ» из `.dt`.
