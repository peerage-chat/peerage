package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test

class LinksTest {
    private fun found(text: String) = Links.find(text).map { text.substring(it.start, it.end) to it.url }

    @Test
    fun schemeAndWwwAndBareDomains() {
        assertEquals(
            listOf(
                "https://example.org/a?b=1#c" to "https://example.org/a?b=1#c",
                "www.example.com/path" to "https://www.example.com/path",
                "peerage.chat" to "https://peerage.chat",
            ),
            found("see https://example.org/a?b=1#c and www.example.com/path or peerage.chat"),
        )
    }

    @Test
    fun trailingPunctuationIsNotPartOfLink() {
        assertEquals(listOf("https://a.com/x" to "https://a.com/x"), found("look: https://a.com/x."))
        assertEquals(listOf("https://a.com" to "https://a.com"), found("(https://a.com)"))
        assertEquals(listOf("https://a.com" to "https://a.com"), found("«https://a.com»!"))
        // Скобки внутри адреса — часть адреса (Википедия).
        assertEquals(
            listOf("https://en.wikipedia.org/wiki/Mesh_(network)" to "https://en.wikipedia.org/wiki/Mesh_(network)"),
            found("https://en.wikipedia.org/wiki/Mesh_(network)"),
        )
    }

    @Test
    fun notLinks() {
        // Имена файлов, версии, многоточия, почта и чужие схемы — не ссылки.
        assertEquals(emptyList<Pair<String, String>>(), found("file.txt v1.2.3 ну... a@b.com javascript:alert(1) ftp://x.org"))
        assertEquals(emptyList<Pair<String, String>>(), found("https://"))
    }

    @Test
    fun uppercaseSchemeIsNormalized() {
        assertEquals(listOf("HTTPS://A.COM" to "https://A.COM"), found("HTTPS://A.COM"))
    }
}
