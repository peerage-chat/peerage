package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test
import java.nio.file.Files

/** Имя в «Загрузках» до Android 10 (п. 60): занятое не перезаписывается. */
class DownloadsTest {
    @Test
    fun taken_name_gets_a_number_before_the_extension() {
        val dir = Files.createTempDirectory("dl").toFile()
        assertEquals("отчёт.pdf", Downloads.freeName(dir, "отчёт.pdf").name)
        dir.resolve("отчёт.pdf").writeText("x")
        assertEquals("отчёт (1).pdf", Downloads.freeName(dir, "отчёт.pdf").name)
        dir.resolve("отчёт (1).pdf").writeText("x")
        assertEquals("отчёт (2).pdf", Downloads.freeName(dir, "отчёт.pdf").name)
        dir.resolve("README").writeText("x")
        assertEquals("README (1)", Downloads.freeName(dir, "README").name)
        dir.deleteRecursively()
    }
}
