package chat.peerage

/**
 * Выбор протяжкой (как в Telegram): долгий тап по сообщению выбирает его, и, не отпуская
 * пальца, ведут по ленте — выбрано всё подряд от первого сообщения до того, что под пальцем.
 * Если первое уже было выбрано, протяжка так же подряд снимает выбор. Вернулся палец назад —
 * пройденное возвращается к тому, что было до жеста. Чистая функция, проверяется юнит-тестом.
 *
 * Единица выбора в ленте — ряд: обычно одно сообщение, у альбома (п. 69) — все его снимки.
 * Отдельный снимок альбома выбирается только своей ячейкой.
 */
object DragRange {
    /**
     * Выбор во время протяжки: [base] — выбор до жеста, [rows] — выбираемые ряды ленты в
     * хронологии, [anchor] — с которого начали, [current] — под пальцем, [adding] — жест
     * выбирает (иначе снимает).
     */
    fun apply(base: Set<MsgRef>, rows: List<List<MsgRef>>, anchor: MsgRef, current: MsgRef, adding: Boolean): Set<MsgRef> {
        val a = rows.indexOfFirst { anchor in it }
        val c = rows.indexOfFirst { current in it }
        val range = when {
            a < 0 -> listOf(anchor)
            c < 0 -> rows[a]
            else -> rows.subList(minOf(a, c), maxOf(a, c) + 1).flatten()
        }
        return if (adding) base + range else base - range.toSet()
    }

    /** Ряд, в котором [ref]; сам [ref], если его нет в [rows]. */
    fun rowOf(rows: List<List<MsgRef>>, ref: MsgRef): List<MsgRef> = rows.firstOrNull { ref in it } ?: listOf(ref)

    /** Ряд выбран, когда выбраны все его сообщения. */
    fun rowSelected(selection: Set<MsgRef>, row: List<MsgRef>): Boolean = row.isNotEmpty() && selection.containsAll(row)

    /** Тап по ряду в режиме выбора: выбран целиком — снимается целиком, иначе добирается. */
    fun toggleRow(selection: Set<MsgRef>, row: List<MsgRef>): Set<MsgRef> =
        if (rowSelected(selection, row)) selection - row.toSet() else selection + row
}
