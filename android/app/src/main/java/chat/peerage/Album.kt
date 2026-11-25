package chat.peerage

/**
 * Альбом (п. 69): снимки и видео, отправленные разом, — одним сообщением на вид. В протоколе
 * каждое вложение по-прежнему своё сообщение, общий у них только номер (`album`); старый
 * клиент номера не знает и показывает их по одному.
 */
object Album {
    /** Больше в одной сетке не кладём: длинная серия режется на альбомы по столько. */
    const val MAX = 10

    /** Номер нового альбома: случайный и ненулевой (0 — «не в альбоме»). */
    fun newId(): Long = kotlin.random.Random.nextLong(1, Long.MAX_VALUE)

    /**
     * Номер альбома каждому вложению серии. [visual] — снимок или видео; файлы и аудио в
     * альбом не идут. Альбом — от двух вложений; серия длиннее [MAX] — несколько альбомов,
     * одиночный остаток — без номера. [on] выключен — номеров нет вовсе.
     */
    fun ids(visual: List<Boolean>, on: Boolean, next: () -> Long): List<Long> {
        val out = MutableList(visual.size) { 0L }
        if (!on) return out
        visual.indices.filter { visual[it] }.chunked(MAX).filter { it.size >= 2 }.forEach { chunk ->
            val id = next()
            chunk.forEach { out[it] = id }
        }
        return out
    }

    /**
     * Раскладка сетки: сколько ячеек в каждом ряду, сверху вниз. Рядов по три, неполный —
     * сверху и не из одной ячейки (кроме трёх: одна крупная над двумя).
     */
    fun rows(n: Int): List<Int> = when {
        n <= 0 -> emptyList()
        n <= 2 -> listOf(n)
        n == 3 -> listOf(1, 2)
        n % 3 == 0 -> List(n / 3) { 3 }
        n % 3 == 1 -> listOf(2, 2) + List((n - 4) / 3) { 3 }
        else -> listOf(2) + List(n / 3) { 3 }
    }

    /**
     * Где в ленте альбом, к которому относится [i]: диапазон индексов или null (не в
     * альбоме). [key] — общий признак альбома (номер + автор) или null у того, что в альбом
     * не входит. Подряд идущие с одним ключом — один альбом, но не длиннее [MAX].
     */
    fun <T> span(items: List<T>, i: Int, key: (T) -> Any?): IntRange? {
        val k = key(items[i]) ?: return null
        var from = i
        while (from > 0 && key(items[from - 1]) == k) from--
        var to = i
        while (to < items.lastIndex && key(items[to + 1]) == k) to++
        val start = from + (i - from) / MAX * MAX
        val end = minOf(start + MAX - 1, to)
        return if (end > start) start..end else null
    }

    /** Ряды ленты по порядку: альбом — один ряд ([span]), остальное — по сообщению. */
    fun <T> feedRows(items: List<T>, key: (T) -> Any?): List<IntRange> {
        val out = ArrayList<IntRange>()
        var i = 0
        while (i < items.size) {
            val r = span(items, i, key) ?: i..i
            out += r
            i = r.last + 1
        }
        return out
    }

    /** Признак альбома у сообщения ленты: снимок или видео с номером; автор — часть ключа. */
    fun key(m: Message): String? =
        if (m.album != 0L && !m.deleted && (m.kind == "image" || m.kind == "video")) "${m.sender}:${m.mine}:${m.album}" else null
}
