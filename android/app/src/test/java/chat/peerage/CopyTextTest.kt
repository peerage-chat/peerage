package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test

class CopyTextTest {
    private val stamp = { t: Long -> "t$t" }

    @Test
    fun oneMessageIsJustItsText() {
        assertEquals("привет", CopyText.format(listOf(CopyText.Entry("Боб", 1, "привет")), stamp))
    }

    @Test
    fun severalCarryAuthorAndTime() {
        val out = CopyText.format(
            listOf(CopyText.Entry("Боб", 1, "привет"), CopyText.Entry("Алиса", 2, "как дела?\nвторая строка")),
            stamp,
        )
        assertEquals("Боб, [t1]\nпривет\n\nАлиса, [t2]\nкак дела?\nвторая строка", out)
    }

    @Test
    fun nothingSelectedIsEmpty() {
        assertEquals("", CopyText.format(emptyList(), stamp))
    }
}
