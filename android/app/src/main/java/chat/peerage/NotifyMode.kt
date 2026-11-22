package chat.peerage

/**
 * Режим уведомлений чата (notifications.md, «Режим чата как в Discord»). Свой выбор живёт в
 * движке со сроком; умолчание группы ставит админ, у личного чата — «все». Звенеть ли —
 * решается здесь, одним местом для шторки и звука в чате.
 */
enum class NotifyMode(val wire: String) {
    All("all"),
    Mentions("mentions"),
    Nothing("none"),
    ;

    /** Пускает ли режим входящее: «упоминания» — только с упоминанием меня. */
    fun lets(mentionsMe: Boolean): Boolean = this == All || (this == Mentions && mentionsMe)

    companion object {
        fun parse(s: String?): NotifyMode? = entries.firstOrNull { it.wire == s }

        /**
         * Действующий режим: свой, пока не истёк (`untilSec` = 0 — бессрочно), иначе умолчание
         * чата. Истёкший срок — своего нет: «по истечении сам возвращается» без записи в базу.
         */
        fun effective(own: NotifyMode?, untilSec: Long, default: NotifyMode, nowSec: Long): NotifyMode =
            if (own != null && (untilSec == 0L || nowSec < untilSec)) own else default

        /** «Заглушить на час» из шторки: в группе — только упоминания, в личном — ничего. */
        fun hourMute(group: Boolean): NotifyMode = if (group) Mentions else Nothing

        /** Сроки выбора (с); 0 — пока не включу. */
        val DURATIONS = listOf(3_600L, 8 * 3_600L, 24 * 3_600L, 0L)
    }
}
