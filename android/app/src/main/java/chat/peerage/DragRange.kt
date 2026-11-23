package chat.peerage

/**
 * Выбор протяжкой (как в Telegram): долгий тап по сообщению выбирает его, и, не отпуская
 * пальца, ведут по ленте — выбрано всё подряд от первого сообщения до того, что под пальцем.
 * Если первое уже было выбрано, протяжка так же подряд снимает выбор. Вернулся палец назад —
 * пройденное возвращается к тому, что было до жеста. Чистая функция, проверяется юнит-тестом.
 */
object DragRange {
    /**
     * Выбор во время протяжки: [base] — выбор до жеста, [order] — выбираемые сообщения в
     * хронологии, [anchor] — с которого начали, [current] — под пальцем, [adding] — жест
     * выбирает (иначе снимает).
     */
    fun apply(base: Set<MsgRef>, order: List<MsgRef>, anchor: MsgRef, current: MsgRef, adding: Boolean): Set<MsgRef> {
        val a = order.indexOf(anchor)
        val c = order.indexOf(current)
        val range = if (a < 0 || c < 0) listOf(anchor) else order.subList(minOf(a, c), maxOf(a, c) + 1)
        return if (adding) base + range else base - range.toSet()
    }
}
