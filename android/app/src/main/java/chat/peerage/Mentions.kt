package chat.peerage

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import kotlinx.serialization.builtins.ListSerializer

/**
 * Упоминания в группах: перевод участков между движком и Compose.
 *
 * Движок хранит участки в байтах UTF-8 (так их проверяет `peerage_core::mention`), а
 * строки Kotlin индексируются в UTF-16. Переводить надо аккуратно: кириллица — два
 * байта на один char, эмодзи — четыре байта на два char.
 */
object Mentions {
    /** Тег аннотации с ключом упомянутого (hex) — по нему пузырь ловит тап по «@». */
    const val TAG = "mention"

    /** Упоминания на провод JNI: `[{"at","len","key"}]`. */
    fun toJson(m: List<Mention>): String =
        WireJson.encodeToString(ListSerializer(Mention.serializer()), m)

    /** Символ в позиции `i`: (байтов UTF-8, char-ов UTF-16). Суррогатная пара — один
     *  символ из 4 байт на два char. */
    private fun step(text: String, i: Int): Pair<Int, Int> {
        val c = text[i]
        return when {
            Character.isHighSurrogate(c) && i + 1 < text.length -> 4 to 2
            c.code < 0x80 -> 1 to 1
            c.code < 0x800 -> 2 to 1
            else -> 3 to 1
        }
    }

    /** Сколько байтов UTF-8 занимает `text[0, end)` (end — индекс UTF-16). */
    fun utf8Len(text: String, end: Int): Int {
        var bytes = 0
        var i = 0
        while (i < end) {
            val (b, n) = step(text, i)
            bytes += b
            i += n
        }
        return bytes
    }

    /** Индекс UTF-16, на котором кончаются `bytes` байт UTF-8; null — посреди символа
     *  или за концом текста. */
    fun utf16Index(text: String, bytes: Int): Int? {
        var b = 0
        var i = 0
        while (b < bytes && i < text.length) {
            val (sb, n) = step(text, i)
            b += sb
            i += n
        }
        return if (b == bytes) i else null
    }

    /** Участки упоминаний в индексах Kotlin-строки. Кривые (за текстом, посреди
     *  символа) молча пропускаются — движок их уже отсеял, это страховка. */
    fun ranges(text: String, mentions: List<Mention>): List<Pair<IntRange, String>> =
        mentions.mapNotNull { m ->
            val from = utf16Index(text, m.at) ?: return@mapNotNull null
            val to = utf16Index(text, m.at + m.len) ?: return@mapNotNull null
            if (to <= from) null else (from until to) to m.key
        }.sortedBy { it.first.first }

    /**
     * Упоминания для отправки. `picked` — кого человек выбрал из подсказки (ключ и
     * вставленная метка без «@»). Ищем каждое «@метка» в итоговом тексте: метку могли
     * стереть или поправить — тогда упоминания нет, и это честно, потому что в тексте
     * его тоже больше нет. Длинные метки ищем первыми: «@Аня» не должна съесть
     * начало «@Аня К.».
     */
    fun collect(text: String, picked: Map<String, String>): List<Mention> {
        val taken = mutableListOf<IntRange>()
        val out = mutableListOf<Mention>()
        for ((key, label) in picked.entries.sortedByDescending { it.value.length }) {
            if (label.isEmpty()) continue
            val needle = "@$label"
            var from = text.indexOf(needle)
            while (from >= 0) {
                val r = from until from + needle.length
                // За меткой не должна продолжаться буква: «@Аня» внутри «@Анязавр» —
                // уже не то имя.
                val after = text.getOrNull(r.last + 1)
                val whole = after == null || !after.isLetterOrDigit()
                if (whole && taken.none { it.first <= r.last && r.first <= it.last }) {
                    taken += r
                    out += Mention(utf8Len(text, r.first), utf8Len(text, r.last + 1) - utf8Len(text, r.first), key)
                }
                from = text.indexOf(needle, from + 1)
            }
        }
        return out.sortedBy { it.at }
    }

    /** Выбранные ранее упоминания своего сообщения — для правки: метка = текст участка. */
    fun picked(text: String, mentions: List<Mention>): Map<String, String> =
        ranges(text, mentions).associate { (r, key) -> key to text.substring(r).removePrefix("@") }

    /**
     * Набирается ли упоминание: «@» в начале слова перед курсором и без пробелов
     * после. Возвращает индекс «@» и набранное после него, иначе null.
     */
    fun query(text: String, cursor: Int): Pair<Int, String>? {
        if (cursor <= 0 || cursor > text.length) return null
        var i = cursor - 1
        while (i >= 0 && text[i] != '@') {
            if (text[i].isWhitespace()) return null
            i--
        }
        if (i < 0) return null
        if (i > 0 && !text[i - 1].isWhitespace()) return null // e-mail, а не упоминание
        return i to text.substring(i + 1, cursor)
    }

    /**
     * Текст пузыря с упоминаниями: участок рисуется как «@» + имя, которое знает ЭТО
     * устройство (`nameOf`), а не как текст отправителя. Упоминание меня выделено
     * фоном — его видно при пролистывании ленты.
     */
    fun render(
        text: String,
        mentions: List<Mention>,
        me: String?,
        nameOf: (String) -> String,
        accent: Color,
        meBg: Color,
    ): AnnotatedString {
        val spans = ranges(text, mentions)
        if (spans.isEmpty()) return AnnotatedString(text)
        return buildAnnotatedString {
            var pos = 0
            for ((r, key) in spans) {
                if (r.first < pos) continue
                append(text, pos, r.first)
                val style = if (key == me) {
                    SpanStyle(color = accent, fontWeight = FontWeight.Medium, background = meBg)
                } else {
                    SpanStyle(color = accent, fontWeight = FontWeight.Medium)
                }
                pushStringAnnotation(TAG, key)
                withStyle(style) { append("@" + nameOf(key)) }
                pop()
                pos = r.last + 1
            }
            append(text, pos, text.length)
        }
    }
}
