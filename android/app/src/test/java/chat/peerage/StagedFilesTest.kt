package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.io.File
import java.nio.file.Files

/** Пара файлов ключа хранилища меняется целиком или не меняется вовсе — при
 *  любом месте, где Android мог убить процесс. */
class StagedFilesTest {
    private val names = listOf("vault.meta", "vault.key.enc")

    private fun dirWithOld(): File {
        val d = Files.createTempDirectory("staged").toFile()
        File(d, "vault.meta").writeText("old-meta")
        File(d, "vault.key.enc").writeText("old-key")
        return d
    }

    private fun read(d: File) = names.map { File(d, it).readText() }

    @Test fun replace_swaps_both() {
        val d = dirWithOld()
        StagedFiles.replaceAll(d, mapOf("vault.meta" to "new-meta".toByteArray(), "vault.key.enc" to "new-key".toByteArray()))
        assertEquals(listOf("new-meta", "new-key"), read(d))
        assertEquals(names.toSet(), d.list()!!.toSet())
    }

    @Test fun crash_before_commit_keeps_old_pair() {
        // Убиты, когда новые версии записаны, а маркера ещё нет.
        val d = dirWithOld()
        File(d, "vault.meta.new").writeText("new-meta")
        File(d, "vault.key.enc.new").writeText("new-ke") // мог быть и обрезан
        StagedFiles.finish(d, names)
        assertEquals(listOf("old-meta", "old-key"), read(d))
        assertFalse(File(d, "vault.key.enc.new").exists())
    }

    @Test fun crash_after_commit_completes_new_pair() {
        // Убиты между переименованиями: мета уже новая, обёртка ещё ждёт.
        val d = dirWithOld()
        File(d, "vault.meta").writeText("new-meta")
        File(d, "vault.key.enc.new").writeText("new-key")
        File(d, "vault.commit").writeText("")
        StagedFiles.finish(d, names)
        assertEquals(listOf("new-meta", "new-key"), read(d))
        assertFalse(File(d, "vault.commit").exists())
    }
}
