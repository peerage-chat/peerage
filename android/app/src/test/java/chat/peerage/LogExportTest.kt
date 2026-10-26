package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LogExportTest {
    private val key = "3f2a9c" + "0123456789abcdef".repeat(3) + "0123456789"

    @Test
    fun keysAreShortened() {
        val out = LogExport.sanitize("group $key: resync")
        assertEquals("group 3f2a9c…: resync", out)
    }

    /** Адрес сети — то, что прячет смена адреса (ADR-007): в файле его быть не должно. */
    @Test
    fun networkAddressesAreRemoved() {
        val out = LogExport.sanitize("смена адреса завершена: 200:6f3a:91c2::1 (оповещено 3)")
        assertFalse(out.contains("6f3a"))
        assertTrue(out.contains("[адрес]"))
    }

    /** Обычные строки logcat не портятся: время, номера строк, короткие числа. */
    @Test
    fun ordinaryLinesSurvive() {
        val line = "10-22 20:29:10.596 22262 22305 I core    : outbox: ждёт 0 · Model.kt:1001 id=4096 abcdef"
        assertEquals(line, LogExport.sanitize(line))
    }
}
