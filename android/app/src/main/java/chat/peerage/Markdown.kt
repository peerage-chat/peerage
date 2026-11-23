package chat.peerage

/**
 * Разметка в тексте сообщения (п. 7 списка с телефона): `**жирный**`, `*курсив*` и `_курсив_`,
 * `~~зачёркнутый~~`, `` `код` ``, блок ```` ``` ```` , `||скрытый||`, строки `> цитата` и списки
 * `- пункт` / `* пункт`. Уходит текст как набран — разметку разбирает показ, поэтому у старого
 * клиента сообщение читается как есть.
 *
 * Разбор не переписывает текст, а говорит, какие куски исходника оставить и что вставить на месте
 * значков ([Rendered.pieces]): так в пузыре сохраняются упоминания и прочие пометки исходной строки.
 * Внутри кода разметка не действует. Курсив — только на границе слова: `snake_case` и `2*3*4`
 * не курсив. Чистая логика, проверяется юнит-тестом.
 */
object Markdown {
    enum class Kind { Bold, Italic, Strike, Code, CodeBlock, Spoiler, Quote }

    /** Участок показа [start, end) в координатах ИТОГОВОГО текста. */
    data class Span(val kind: Kind, val start: Int, val end: Int)

    sealed interface Piece {
        /** Кусок исходника [start, end). */
        data class Keep(val start: Int, val end: Int) : Piece

        /** Вставка на месте значка (маркер списка, черта цитаты). */
        data class Insert(val text: String) : Piece
    }

    data class Rendered(val text: String, val pieces: List<Piece>, val spans: List<Span>)

    /** Черта перед строкой цитаты. */
    const val QUOTE_BAR = "▎"
    private const val BULLET = "•"

    private class Edit(val start: Int, val end: Int, val insert: String)

    private class Raw(val kind: Kind, val start: Int, val end: Int)

    private val codeBlock = Regex("```(?:[\\w+#.-]*\\n)?([\\s\\S]+?)\\n?```")
    private val inlineCode = Regex("`([^`\\n]+)`")
    private val paired = listOf(
        Kind.Bold to Regex("\\*\\*(?=\\S)(.+?)(?<=\\S)\\*\\*"),
        Kind.Strike to Regex("~~(?=\\S)(.+?)(?<=\\S)~~"),
        Kind.Spoiler to Regex("\\|\\|(?=\\S)(.+?)(?<=\\S)\\|\\|"),
    )
    private val italic = Regex("(?<![\\p{L}\\p{N}])([*_])(?![\\s*_])(.+?)(?<![\\s])\\1(?![\\p{L}\\p{N}])")
    private val quoteLine = Regex("(?m)^> ?")
    private val bulletLine = Regex("(?m)^([ \\t]*)[-*](?= )")

    fun render(src: String): Rendered {
        val edits = ArrayList<Edit>()
        val raws = ArrayList<Raw>()
        // Маска: занятое кодом и уже разобранными значками больше не участвует в разборе.
        val mask = src.toCharArray()
        fun claim(a: Int, b: Int, c: Char = '\u0001') {
            for (i in a until b) mask[i] = c
        }
        fun free(a: Int, b: Int) = (a until b).none { mask[it] == '\u0001' || mask[it] == '\u0002' }

        for (m in codeBlock.findAll(src)) {
            val inner = m.groups[1]!!.range
            edits += Edit(m.range.first, inner.first, "")
            edits += Edit(inner.last + 1, m.range.last + 1, "")
            raws += Raw(Kind.CodeBlock, inner.first, inner.last + 1)
            claim(m.range.first, inner.first)
            claim(inner.first, inner.last + 1, '\u0002')
            claim(inner.last + 1, m.range.last + 1)
        }
        for (m in inlineCode.findAll(String(mask))) {
            if (!free(m.range.first, m.range.last + 1)) continue
            edits += Edit(m.range.first, m.range.first + 1, "")
            edits += Edit(m.range.last, m.range.last + 1, "")
            raws += Raw(Kind.Code, m.range.first + 1, m.range.last)
            claim(m.range.first, m.range.first + 1)
            claim(m.range.first + 1, m.range.last, '\u0002')
            claim(m.range.last, m.range.last + 1)
        }
        // Строчные: цитата и маркер списка — в начале строки вне кода.
        for (m in quoteLine.findAll(String(mask))) {
            if (!free(m.range.first, m.range.last + 1)) continue
            val lineEnd = src.indexOf('\n', m.range.first).let { if (it < 0) src.length else it }
            edits += Edit(m.range.first, m.range.last + 1, "$QUOTE_BAR ")
            raws += Raw(Kind.Quote, m.range.first, lineEnd)
            claim(m.range.first, m.range.last + 1)
        }
        for (m in bulletLine.findAll(String(mask))) {
            val at = m.range.last
            if (!free(at, at + 1)) continue
            edits += Edit(at, at + 1, BULLET)
            claim(at, at + 1)
        }
        for ((kind, re) in paired) {
            for (m in re.findAll(String(mask))) {
                val inner = m.groups[1]!!.range
                if (!free(m.range.first, inner.first) || !free(inner.last + 1, m.range.last + 1)) continue
                edits += Edit(m.range.first, inner.first, "")
                edits += Edit(inner.last + 1, m.range.last + 1, "")
                raws += Raw(kind, inner.first, inner.last + 1)
                claim(m.range.first, inner.first)
                claim(inner.last + 1, m.range.last + 1)
            }
        }
        for (m in italic.findAll(String(mask))) {
            val inner = m.groups[2]!!.range
            edits += Edit(m.range.first, m.range.first + 1, "")
            edits += Edit(m.range.last, m.range.last + 1, "")
            raws += Raw(Kind.Italic, inner.first, inner.last + 1)
            claim(m.range.first, m.range.first + 1)
            claim(m.range.last, m.range.last + 1)
        }

        edits.sortBy { it.start }
        val pieces = ArrayList<Piece>()
        val out = StringBuilder()
        var at = 0
        for (e in edits) {
            if (e.start > at) {
                pieces += Piece.Keep(at, e.start)
                out.append(src, at, e.start)
            }
            if (e.insert.isNotEmpty()) {
                pieces += Piece.Insert(e.insert)
                out.append(e.insert)
            }
            at = e.end
        }
        if (at < src.length) {
            pieces += Piece.Keep(at, src.length)
            out.append(src, at, src.length)
        }
        // Позиция исходника → позиция показа: сдвиг на все правки, закончившиеся до неё.
        fun map(p: Int): Int = p + edits.sumOf { if (it.end <= p) it.insert.length - (it.end - it.start) else 0 }
        val spans = raws.map { Span(it.kind, map(it.start), map(it.end)) }.filter { it.end > it.start }
        return Rendered(out.toString(), pieces, spans)
    }

    /**
     * Текст без значков для превью, уведомления и цитаты ответа: скрытое заменено точками —
     * спойлер не должен читаться из списка чатов или с экрана блокировки.
     */
    fun plain(src: String): String {
        val r = render(src)
        val spoilers = r.spans.filter { it.kind == Kind.Spoiler }.sortedByDescending { it.start }
        if (spoilers.isEmpty()) return r.text
        val sb = StringBuilder(r.text)
        for (s in spoilers) sb.replace(s.start, s.end, "•••")
        return sb.toString()
    }
}
