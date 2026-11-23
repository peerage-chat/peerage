package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

class ProfilesTest {
    @get:Rule val tmp = TemporaryFolder()

    /** Прежняя установка: данные переезжают в `profiles/main`, общее остаётся в корне. */
    @Test
    fun oldInstallMovesIntoMain() {
        val files = tmp.newFolder("files")
        File(files, "messenger.db").writeText("db")
        File(files, "blobs").mkdirs()
        File(files, "blobs/x").writeText("blob")
        File(files, "last-crash.txt").writeText("crash")
        val main = Profiles.migrate(files)
        assertEquals(File(files, "profiles/main"), main)
        assertEquals("db", File(main, "messenger.db").readText())
        assertEquals("blob", File(main, "blobs/x").readText())
        assertTrue(File(files, "last-crash.txt").exists())
        assertEquals(setOf("profiles", "last-crash.txt"), files.list()!!.toSet())
    }

    /** Убит посреди переноса: следующий старт доносит остальное и собирает `main`. */
    @Test
    fun interruptedMoveResumes() {
        val files = tmp.newFolder("files")
        File(files, "profiles/main.part").mkdirs()
        File(files, "profiles/main.part/messenger.db").writeText("db")
        File(files, "vault.meta").writeText("meta")
        val main = Profiles.migrate(files)
        assertEquals("db", File(main, "messenger.db").readText())
        assertEquals("meta", File(main, "vault.meta").readText())
        assertFalse(File(files, "profiles/main.part").exists())
    }

    /** Уже разложено — ничего не трогаем; новая установка получает пустой `main`. */
    @Test
    fun laidOutOrFreshIsLeftAlone() {
        val files = tmp.newFolder("files")
        val main = Profiles.migrate(files)
        assertTrue(main.isDirectory)
        File(files, "stray").writeText("x")
        assertEquals(main, Profiles.migrate(files))
        assertTrue("после раскладки корень не разбирается", File(files, "stray").exists())
    }
}
