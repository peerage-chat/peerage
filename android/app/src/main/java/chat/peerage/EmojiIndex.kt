package chat.peerage

/**
 * Набор эмодзи панели и поиск по нему (`assets/emoji.tsv`, собирает `scripts/gen-emoji.py` из
 * Unicode и CLDR): группа, символ, названия и ключевые слова на русском и английском. Поиск — по
 * началу слов: «кот» находит 🐈 и 😺, «heart» — сердца; ё и е не различаются. Чистая логика,
 * проверяется юнит-тестом; что из набора шрифт телефона не рисует, отсеивает панель.
 */
object EmojiIndex {
    data class Entry(val group: Int, val emoji: String, val names: List<String>, val words: List<String>)

    fun parse(tsv: String): List<Entry> = tsv.lineSequence().mapNotNull { line ->
        if (line.isBlank() || line.startsWith("#")) return@mapNotNull null
        val parts = line.split('\t')
        if (parts.size < 3) return@mapNotNull null
        val g = parts[0].toIntOrNull() ?: return@mapNotNull null
        fun list(i: Int) = parts.getOrNull(i).orEmpty().split('|').map(::norm).filter { it.isNotEmpty() }
        Entry(g, parts[1], list(2), list(3))
    }.toList()

    private fun norm(s: String) = s.lowercase().replace('ё', 'е').trim()

    /**
     * Эмодзи по запросу [query], лучшие первыми: название целиком → начало названия → ключевое
     * слово целиком → начало ключевого слова → начало любого слова внутри. Несколько слов в
     * запросе — каждое должно найтись; запрос, целиком входящий в название, — выше.
     */
    fun search(entries: List<Entry>, query: String, limit: Int = 200): List<String> {
        val full = norm(query)
        val tokens = full.split(' ', '\t', '\n').filter { it.isNotEmpty() }
        if (tokens.isEmpty()) return emptyList()
        return entries.asSequence().mapIndexedNotNull { i, e ->
            var score = 0
            for (t in tokens) score += tokenScore(e, t) ?: return@mapIndexedNotNull null
            if (tokens.size > 1 && (e.names + e.words).any { it.contains(full) }) score -= 5
            Triple(score, i, e.emoji)
        }.sortedWith(compareBy({ it.first }, { it.second })).map { it.third }.take(limit).toList()
    }

    private fun tokenScore(e: Entry, t: String): Int? = when {
        e.names.any { it == t } -> 0
        e.names.any { it.startsWith(t) } -> 1
        e.words.any { it == t } -> 2
        e.words.any { it.startsWith(t) } -> 3
        (e.names + e.words).any { w -> w.split(' ', '-', ':').any { it.startsWith(t) } } -> 4
        else -> null
    }
}
