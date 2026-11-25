package chat.peerage

/**
 * Текст копии выделенных сообщений (п. 75). Одно — как есть; несколько — у каждого «Автор,
 * [дата время]» строкой над текстом, блоки через пустую строку: иначе в склеенном тексте не
 * различить, кто что сказал и когда (так же копирует Telegram).
 */
object CopyText {
    data class Entry(val author: String, val time: Long, val text: String)

    fun format(entries: List<Entry>, stamp: (Long) -> String): String =
        entries.singleOrNull()?.text
            ?: entries.joinToString("\n\n") { "${it.author}, [${stamp(it.time)}]\n${it.text}" }
}
