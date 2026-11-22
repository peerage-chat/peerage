package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Режим чата: что пускает и когда кончается свой выбор (notifications.md). */
class NotifyModeTest {
    @Test
    fun mentionsLetOnlyMentions() {
        assertTrue(NotifyMode.All.lets(mentionsMe = false))
        assertTrue(NotifyMode.Mentions.lets(mentionsMe = true))
        assertFalse(NotifyMode.Mentions.lets(mentionsMe = false))
        assertFalse(NotifyMode.Nothing.lets(mentionsMe = true))
    }

    @Test
    fun ownChoiceExpiresToDefault() {
        val m = NotifyMode.Nothing
        assertEquals(m, NotifyMode.effective(m, untilSec = 100, default = NotifyMode.All, nowSec = 99))
        assertEquals(NotifyMode.All, NotifyMode.effective(m, untilSec = 100, default = NotifyMode.All, nowSec = 100))
        assertEquals(m, NotifyMode.effective(m, untilSec = 0, default = NotifyMode.All, nowSec = Long.MAX_VALUE))
    }

    @Test
    fun noOwnChoiceFollowsGroupDefault() {
        assertEquals(NotifyMode.Mentions, NotifyMode.effective(null, 0, NotifyMode.Mentions, 5))
    }

    @Test
    fun wireNamesMatchEngine() {
        assertEquals(listOf("all", "mentions", "none"), NotifyMode.entries.map { it.wire })
        assertEquals(NotifyMode.Mentions, NotifyMode.parse("mentions"))
        assertEquals(null, NotifyMode.parse(""))
    }

    @Test
    fun hourMuteKeepsMentionsInGroups() {
        assertEquals(NotifyMode.Mentions, NotifyMode.hourMute(group = true))
        assertEquals(NotifyMode.Nothing, NotifyMode.hourMute(group = false))
    }
}
