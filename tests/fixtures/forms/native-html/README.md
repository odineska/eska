# Пустое поле HTML-документа: native-матрица

Десять XML-входов, 20 native-снимков ru/en, без BSL. Полный виджет — белая
поверхность с квадратной рамкой. Все native-пиксели проверены.
[Приёмка Solid Linux](../../../../docs/measurements/t160-native-html-linux.json)
сравнивает по 1 824 500 пикселей всех 17 состояний ru/en, включая dirty XML,
identity/DOM и настоящее отдельное окно, без масок и допусков.

F_Background/I_Toolbar/J_NoToolbar сохраняют запросы BackColor/ShowToolbar,
которые native export удаляет; canonical/ содержит исходные байты выгрузок.
Поверхность остаётся белой без панели инструментов. Остальные XML и source/ —
native export. ReadOnly сохраняет обычную рамку; Enabled=false осветляет её.
Просмотр не выполняет содержимое HTML. Другие свойства/шрифты/содержимое/DPI,
полные формы и Windows остаются открытыми.
