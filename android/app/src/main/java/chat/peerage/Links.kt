package chat.peerage

/**
 * Ссылки в тексте сообщения: где они и куда ведут. Только http(s): чужие схемы
 * (`javascript:`, `intent:`, `file:`) из чужого сообщения открывать нельзя. Кроме адресов со
 * схемой — `www.…` и голые домены из короткого списка зон: «file.txt» и «v1.2» ссылками не
 * становятся. Разбор чистый — без Android, проверяется юнит-тестом.
 */
object Links {
    /** Тег аннотации с адресом — по нему пузырь ловит тап. */
    const val TAG = "url"

    /** Ссылка: диапазон [start, end) в тексте и адрес для открытия (всегда http/https). */
    data class Link(val start: Int, val end: Int, val url: String)

    private const val BODY = "[^\\s<>\"«»]"
    private const val ZONES = "com|org|net|ru|io|dev|app|me|info|chat|de|uk|fr|eu|us|tv|co|ai|gg|" +
        "xyz|su|by|kz|ua|edu|gov|site|online|tech|blog|page|link|news|cc"

    // Перед адресом без схемы — не буква, не «@» (почта), не «/», «.» и «:» (хвост чужого адреса).
    private val PATTERN = Regex(
        "(?<![\\w])(?i:https?)://[^\\s<>\"«»/]$BODY*" +
            "|(?<![\\w@./:-])(?i:www\\.)[a-z0-9-]+$BODY*" +
            "|(?<![\\w@./:-])(?i:[a-z0-9-]+(?:\\.[a-z0-9-]+)*\\.(?:$ZONES))(?![\\w-])(?:/$BODY*)?",
        RegexOption.IGNORE_CASE,
    )

    private const val TRAILING = ".,;:!?'\")]}"

    fun find(text: String): List<Link> = PATTERN.findAll(text).mapNotNull { m ->
        val raw = trimTail(m.value)
        if (raw.isEmpty()) return@mapNotNull null
        val url = when {
            raw.contains("://") -> raw.substringBefore("://").lowercase() + "://" + raw.substringAfter("://")
            else -> "https://$raw"
        }
        if (url.substringAfter("://").isEmpty()) null else Link(m.range.first, m.range.first + raw.length, url)
    }.toList()

    /** Знаки препинания в конце — от фразы, не от адреса; «)» — от адреса, если скобки в нём парные. */
    private fun trimTail(s: String): String {
        var end = s.length
        while (end > 0 && s[end - 1] in TRAILING) {
            if (s[end - 1] == ')' && s.substring(0, end).count { it == '(' } >= s.substring(0, end).count { it == ')' }) break
            end--
        }
        return s.substring(0, end)
    }
}
