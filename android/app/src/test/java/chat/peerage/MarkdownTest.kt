package chat.peerage

import chat.peerage.Markdown.Kind
import org.junit.Assert.assertEquals
import org.junit.Test

class MarkdownTest {
    /** Итоговый текст и размеченные куски — как «вид:текст». */
    private fun shown(src: String): Pair<String, List<String>> {
        val r = Markdown.render(src)
        return r.text to r.spans.sortedBy { it.start }.map { "${it.kind}:${r.text.substring(it.start, it.end)}" }
    }

    @Test
    fun inlineMarks() {
        assertEquals(
            "bold it strike code secret" to listOf("Bold:bold", "Italic:it", "Strike:strike", "Code:code", "Spoiler:secret"),
            shown("**bold** _it_ ~~strike~~ `code` ||secret||"),
        )
        assertEquals("a b" to listOf("Italic:a b"), shown("*a b*"))
    }

    @Test
    fun nestedBoldItalic() {
        assertEquals("very nice" to listOf("Bold:very nice", "Italic:nice"), shown("**very _nice_**"))
    }

    @Test
    fun notMarkup() {
        // Внутрисловное, пробелы у значков, арифметика, одиночные значки — как есть.
        for (s in listOf("snake_case_name", "2*3*4", "a * b * c", "** x **", "price ~~", "`unclosed", "file_v1_final.txt")) {
            assertEquals(s, Markdown.render(s).text)
            assertEquals(emptyList<Markdown.Span>(), Markdown.render(s).spans)
        }
    }

    @Test
    fun codeIsLiteral() {
        assertEquals("**x** _y_" to listOf("Code:**x** _y_"), shown("`**x** _y_`"))
        assertEquals("val a = b*c*d\n**no**" to listOf("CodeBlock:val a = b*c*d\n**no**"), shown("```kotlin\nval a = b*c*d\n**no**\n```"))
    }

    @Test
    fun quotesAndLists() {
        val (text, spans) = shown("> said\n- one\n* two\n1. three")
        assertEquals("${Markdown.QUOTE_BAR} said\n• one\n• two\n1. three", text)
        assertEquals(listOf("Quote:${Markdown.QUOTE_BAR} said"), spans)
    }

    @Test
    fun piecesKeepSourceRanges() {
        // Из кусков исходника и вставок собирается тот же текст — по ним пузырь переносит пометки.
        val src = "hi **@bob** > x\n> q"
        val r = Markdown.render(src)
        val rebuilt = r.pieces.joinToString("") {
            when (it) {
                is Markdown.Piece.Keep -> src.substring(it.start, it.end)
                is Markdown.Piece.Insert -> it.text
            }
        }
        assertEquals(r.text, rebuilt)
        assertEquals(listOf(Kind.Bold, Kind.Quote), r.spans.sortedBy { it.start }.map { it.kind })
    }

    @Test
    fun plainHidesSpoiler() {
        assertEquals("pin is •••, ok", Markdown.plain("pin is ||1234||, **ok**"))
        assertEquals("bold", Markdown.plain("**bold**"))
    }
}
