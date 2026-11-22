package chat.peerage

/**
 * «Комната ожила» (notifications.md, п. 7): в пустой голосовой комнате группы кто-то появился.
 *
 * Тикер раз в несколько секунд сообщает, есть ли кто в комнате. Ожила — только переход «пусто →
 * есть», и не всякий: в первые [WARMUP_MS] после запуска маячки ещё не дошли (уже идущая комната
 * выглядела бы ожившей), а комната, где кто-то был меньше [QUIET_MS] назад, не ожила, а мигнула
 * (маячок задержался дольше срока присутствия). Свою комнату тикер отмечает живой — после выхода
 * из неё оставшиеся внутри «оживлением» не считаются.
 *
 * Один писатель — поток тикера.
 */
class RoomWatch(private val startMs: Long) {
    private val alive = HashMap<String, Boolean>()
    private val lastAliveMs = HashMap<String, Long>()

    /** Состояние комнаты [gid] на момент [nowMs]; true — ожила. */
    fun observe(gid: String, someone: Boolean, nowMs: Long): Boolean {
        val was = alive.put(gid, someone) ?: false
        if (!someone) return false
        val last = lastAliveMs.put(gid, nowMs)
        return !was && nowMs - startMs > WARMUP_MS && (last == null || nowMs - last > QUIET_MS)
    }

    companion object {
        /** Срок присутствия маячка в движке (12 с) с запасом на тик. */
        const val WARMUP_MS = 20_000L
        const val QUIET_MS = 120_000L
    }
}
