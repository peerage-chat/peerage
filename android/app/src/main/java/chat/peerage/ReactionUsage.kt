package chat.peerage

/**
 * Быстрый ряд реакций над пузырём — по частоте своих реакций (п. 24 с телефона): в ходу у
 * человека обычно несколько символов, и им место в ряду, а не в полной панели за «+».
 *
 * Счёт живёт в prefs клиента (как «недавние эмодзи»): это предпочтение этого устройства, не
 * часть переписки. Здесь только чистая логика — состав ряда и формат хранения.
 */
internal object ReactionUsage {
    /** Ряд без истории (макет `chatact/ActMenu`); им же добирается ряд, пока своих меньше шести. */
    val DEFAULTS = listOf("👍", "❤️", "😂", "😮", "😢", "🙏")
    private const val SLOTS = 6

    /** Шесть реакций ряда: чаще — левее; при равном счёте — порядок ряда по умолчанию, потом по символу. */
    fun quick(counts: Map<String, Int>): List<String> {
        val rank = { e: String -> DEFAULTS.indexOf(e).let { if (it < 0) Int.MAX_VALUE else it } }
        val used = counts.filterValues { it > 0 }.keys
            .sortedWith(compareByDescending<String> { counts.getValue(it) }.thenBy(rank).thenBy { it })
        return (used + DEFAULTS).distinct().take(SLOTS)
    }

    /** Строка prefs → счёт. Эмодзи не содержат ни пробела, ни «=»: «👍=4 ❤️=1». Кривое — пропускаем. */
    fun parse(s: String?): Map<String, Int> = s.orEmpty().split(' ').mapNotNull { part ->
        val i = part.lastIndexOf('=')
        val n = part.substring(i + 1).toIntOrNull()
        if (i <= 0 || n == null) null else part.substring(0, i) to n
    }.toMap()

    fun format(counts: Map<String, Int>): String = counts.entries.joinToString(" ") { "${it.key}=${it.value}" }
}
