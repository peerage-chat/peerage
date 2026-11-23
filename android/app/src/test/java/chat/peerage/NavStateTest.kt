package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.io.File

class NavStateTest {
    private fun nav(vararg path: Screen) = NavState(Screen.Contacts).apply { path.forEach { go(it) } }

    /** Баг 2026-10-22: «Новый чат» → «Добавить контакт» → назад вело на главную. */
    @Test
    fun backReturnsWhereYouCameFrom() {
        val n = nav(Screen.NewChat, Screen.Add)
        n.back()
        assertEquals(Screen.NewChat, n.current)

        val m = nav(Screen.Me, Screen.SettingsSecurity, Screen.ContactList, Screen.Add)
        m.back()
        assertEquals(Screen.ContactList, m.current)
        m.back()
        assertEquals(Screen.SettingsSecurity, m.current)
    }

    /** Предиктивный «назад» тянет к экрану, куда «назад» и приведёт. */
    @Test
    fun previousIsWhereBackLeads() {
        val n = nav(Screen.Me, Screen.SettingsSecurity)
        assertEquals(Screen.Me, n.previous)
        assertEquals(2, n.indexOf(Screen.SettingsSecurity))
        n.back()
        assertEquals(Screen.Me, n.current)
        assertEquals(-1, n.indexOf(Screen.SettingsSecurity))
        n.back()
        assertEquals(null, n.previous)
    }

    /** Направление перехода, когда прежнего экрана в истории уже нет: «назад» и «вглубь». */
    @Test
    fun lastStepDirection() {
        val n = nav(Screen.Me)
        assertEquals(true, n.lastForward)
        n.back()
        assertEquals(false, n.lastForward)
        n.go(Screen.Me)
        n.go(Screen.Storage)
        n.go(Screen.Me) // к уже открытому — это возврат
        assertEquals(false, n.lastForward)
        n.replace(Screen.Calls)
        assertEquals(true, n.lastForward)
    }

    /** Любой путь вперёд разматывается назад в точности в обратном порядке. */
    @Test
    fun anyForwardPathUnwindsInReverse() {
        val path = listOf(
            Screen.Me,
            Screen.MyCode,
            Screen.Storage,
            Screen.Backup,
        )
        val n = nav(*path.toTypedArray())
        for (expected in path.reversed().drop(1) + Screen.Contacts) {
            n.back()
            assertEquals(expected, n.current)
        }
        n.back() // на главной «назад» ничего не делает — выходит система
        assertEquals(Screen.Contacts, n.current)
        assertEquals(1, n.depth)
    }

    /** Переход на экран из истории — возврат к нему, без петель: чат → профиль →
     *  «Написать» → назад ведёт туда, откуда открыли чат, а не в профиль. */
    @Test
    fun goingToScreenInHistoryReturnsToIt() {
        val n = nav(Screen.Search, Screen.Chat, Screen.Info)
        n.go(Screen.Chat)
        assertEquals(Screen.Chat, n.current)
        n.back()
        assertEquals(Screen.Search, n.current)
    }

    @Test
    fun goingHomeStartsHistoryOver() {
        val n = nav(Screen.NewChat, Screen.CreateGroup)
        n.go(Screen.Contacts)
        assertEquals(1, n.depth)
    }

    /** Выбор собеседника в «Новом чате» заменяет выбор: назад из чата — на главную. */
    @Test
    fun replaceDropsTheFinishedScreen() {
        val n = nav(Screen.NewChat)
        n.replace(Screen.Chat)
        n.back()
        assertEquals(Screen.Contacts, n.current)
    }

    /** Нас исключили из группы, пока были в ней или в её участниках. */
    @Test
    fun leaveUnwindsPastGoneScreens() {
        val n = nav(Screen.Calls, Screen.GroupChat, Screen.AddMember)
        n.leave(setOf(Screen.GroupChat, Screen.AddMember))
        assertEquals(Screen.Calls, n.current)
    }

    @Test
    fun deepLinkKeepsHomeUnderneath() {
        val n = NavState(Screen.RotateAddress)
        n.back()
        assertEquals(Screen.Contacts, n.current)
    }

    /**
     * Две панели: выбранное в списке открывается с чистой истории — «назад» из нового чата
     * ведёт на главную (пустая правая панель), а не в прежний чат и его профиль.
     */
    @Test
    fun resetToStartsHistoryOverHome() {
        val n = nav(Screen.Chat, Screen.Info)
        n.resetTo(Screen.GroupChat)
        assertEquals(Screen.GroupChat, n.current)
        assertEquals(2, n.depth)
        assertEquals(Screen.Contacts, n.previous)
        n.resetTo(Screen.Contacts)
        assertEquals(1, n.depth)
    }

    /**
     * Сторож от возврата бага: у шапок не должно быть своих `onBack = { screen = … }`
     * и таблицы родителей — «назад» решает только стек. Именно так баг и появлялся:
     * новый экран получал зашитого родителя мимо общего правила.
     */
    @Test
    fun shellHasNoHandWrittenBackTargets() {
        val src = File("src/main/java/chat/peerage/MainActivity.kt").readText()
        assertFalse("свой onBack в шапке", Regex("""onBack\s*=\s*\{""").containsMatchIn(src))
        assertFalse("таблица родителей", src.contains("parentOf"))
    }
}
