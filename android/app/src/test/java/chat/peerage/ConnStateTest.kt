package chat.peerage

import chat.peerage.ConnState.Part
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Строка непрочитанного в «связи» (notifications.md, решение 2). */
class ConnStateTest {
    @Test
    fun nothingUnreadIsEmpty() {
        assertTrue(ConnState().parts().isEmpty())
        assertFalse(ConnState(offline = true, searching = true).hasUnread)
    }

    @Test
    fun partsInFixedOrder() {
        val s = ConnState(messages = 5, chats = 2, mention = true, requests = 1, invites = 3)
        assertEquals(listOf(Part.Messages(5, 2), Part.Mention, Part.Requests(1), Part.Invites(3)), s.parts())
        assertTrue(s.hasUnread)
    }

    @Test
    fun requestAloneIsUnread() {
        assertEquals(listOf(Part.Requests(2)), ConnState(requests = 2).parts())
    }
}
