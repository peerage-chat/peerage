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

    /** Снимок движка — построчно, по разделам; одни числа. */
    @Test
    fun snapshotIsReadable() {
        val json = """{"net":{"peers":3,"peers_up":2,"tree":7,"ping_ms":41.5},"sessions":{"live":2,"confirmed":1},""" +
            """"outbox":{"pending":0},"budget_drops":{"packets":12,"bytes":4096}}"""
        val out = LogExport.snapshot(json)
        assertTrue(out.contains("net: peers 3 · peers_up 2 · tree 7 · ping_ms 41.5"))
        assertTrue(out.contains("sessions: live 2 · confirmed 1"))
        assertTrue(out.contains("budget_drops: packets 12 · bytes 4096"))
        assertEquals("движок не поднят\n", LogExport.snapshot(null))
    }

    /**
     * Сторож обезличивания (7n): вызовы `Log.*` в коде не подставляют то, что человек
     * пересылать не обязан, — имена, текст, подписи, пароли, ключи. Движок проверяет то же
     * своим тестом (`engine/tests/log_privacy.rs`); здесь — сторона Kotlin.
     */
    @Test
    fun kotlinLogsDoNotInterpolatePrivateFields() {
        val banned = setOf(
            "text", "name", "displayName", "selfName", "caption", "greeting", "password",
            "key", "pubkey", "peer", "contact", "address", "addr", "body", "input", "words",
        )
        // Совпало по имени, но не личное: имя декодера устройства.
        val allowed = setOf("codec.name")
        val leaks = mutableListOf<String>()
        java.io.File("src/main/java").walkTopDown().filter { it.extension == "kt" }.forEach { f ->
            val src = f.readText()
            Regex("""Log\.[vdiwe]\(""").findAll(src).forEach { m ->
                // Аргументы вызова — до парной закрывающей скобки, хоть на нескольких строках.
                var depth = 1
                var i = m.range.last + 1
                while (i < src.length && depth > 0) {
                    when (src[i]) {
                        '(' -> depth++
                        ')' -> depth--
                    }
                    i++
                }
                val call = src.substring(m.range.first, i)
                Regex("""\$\{?([A-Za-z_][A-Za-z0-9_.]*)""").findAll(call).forEach { v ->
                    val last = v.groupValues[1].substringAfterLast('.')
                    if (last in banned && v.groupValues[1] !in allowed) leaks += "${f.name}: ${v.value}"
                }
            }
        }
        assertTrue("в лог подставлено личное:\n" + leaks.joinToString("\n"), leaks.isEmpty())
    }
}
