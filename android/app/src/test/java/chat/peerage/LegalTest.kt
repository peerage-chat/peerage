package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class LegalTest {
    @Test
    fun parsesHeadingsParagraphsAndBullets() {
        val md = """
            # Title

            First line
            continues here.

            ## Section
            - one
              still one
            - two
            Tail.
        """.trimIndent()
        assertEquals(
            listOf(
                DocBlock.Para("First line continues here."),
                DocBlock.Heading("Section"),
                DocBlock.Bullet("one still one"),
                DocBlock.Bullet("two"),
                DocBlock.Para("Tail."),
            ),
            Legal.parse(md),
        )
    }

    /** Обе редакции — с одинаковым набором разделов: перевод не теряет кусок политики. */
    @Test
    fun bothLanguagesHaveTheSameSections() {
        val dir = File("src/main/assets/legal")
        val shape = { lang: String ->
            Legal.parse(File(dir, "privacy.$lang.md").readText()).map { it::class.simpleName }
        }
        val ru = shape("ru")
        assertTrue(ru.count { it == "Heading" } >= 5)
        assertEquals(ru, shape("en"))
    }
}
