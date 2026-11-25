package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Журнал изменений в приложении (п. 51, app-ui.md → «Что нового»). */
class ChangelogTest {
    private val json = """
        {"versions": [
          {"version": "0.3.0", "date": "2026-11-24", "summary": "Третья бета.",
           "new": ["Очистка вложений по чатам"], "fixed": ["Падение ленты"]},
          {"version": "0.4.0", "date": "2026-11-25", "summary": "Обновления приходят сами.",
           "new": ["Свайп вправо — назад к списку"], "fixed": [], "future": 1},
          {"version": "не версия", "date": "2026-11-25", "summary": "x", "new": [], "fixed": []}
        ]}
    """.trimIndent()

    @Test
    fun version_code_matches_gradle() {
        assertEquals(300L, Changelog.versionCode("0.3.0"))
        assertEquals(310L, Changelog.versionCode("0.3.10"))
        assertEquals(10203L, Changelog.versionCode("1.2.3"))
        assertNull(Changelog.versionCode("0.3"))
    }

    @Test
    fun newest_first_broken_entries_skipped() {
        val list = Changelog.parse(json)
        assertEquals(listOf("0.4.0", "0.3.0"), list.map { it.version })
        assertEquals(listOf("Свайп вправо — назад к списку"), list[0].new)
        assertTrue(list[0].fixed.isEmpty())
    }

    @Test
    fun garbage_is_empty() {
        assertTrue(Changelog.parse("not json").isEmpty())
    }

    private val entries = Changelog.parse(json)

    @Test
    fun fresh_install_shows_nothing() {
        assertNull(Changelog.toShow(seen = 0, installed = 400, freshInstall = true, entries))
    }

    @Test
    fun update_from_a_version_before_the_journal_shows_the_installed_one() {
        assertEquals("0.4.0", Changelog.toShow(seen = 0, installed = 400, freshInstall = false, entries)?.version)
    }

    @Test
    fun shown_once() {
        assertNull(Changelog.toShow(seen = 400, installed = 400, freshInstall = false, entries))
    }

    @Test
    fun only_entries_up_to_the_installed_version() {
        // Сборка 0.3.x не показывает записи о 0.4.0, которой у человека ещё нет.
        assertEquals("0.3.0", Changelog.toShow(seen = 200, installed = 300, freshInstall = false, entries)?.version)
        assertEquals(listOf("0.3.0"), Changelog.upTo(entries, 309).map { it.version })
    }

    @Test
    fun update_without_its_own_entry_shows_nothing() {
        assertNull(Changelog.toShow(seen = 400, installed = 401, freshInstall = false, entries))
    }
}
