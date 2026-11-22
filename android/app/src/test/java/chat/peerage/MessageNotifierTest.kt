package chat.peerage

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Когда входящее поднимает шторку (notifications.md, п. 3 и 4). */
class MessageNotifierTest {
    private val now = 50_000_000L

    private fun ok(
        mine: Boolean = false,
        system: Boolean = false,
        call: Boolean = false,
        onScreen: Boolean = false,
        muted: Boolean = false,
        sent: Long = now - 2_000,
    ) = MessageNotifier.shouldNotify(mine, system, call, onScreen, muted, sent, now)

    @Test
    fun freshIncomingInAnotherChatNotifies() = assertTrue(ok())

    @Test
    fun openChatOnScreenOnlySounds() = assertFalse(ok(onScreen = true))

    @Test
    fun ownSystemAndCallsDoNot() {
        assertFalse(ok(mine = true))
        assertFalse(ok(system = true))
        assertFalse(ok(call = true))
    }

    @Test
    fun mutedChatDoesNot() = assertFalse(ok(muted = true))

    @Test
    fun backlogAfterOfflineDoesNot() {
        assertFalse(ok(sent = now - ChatSounds.BACKLOG_MS - 1))
        assertTrue("время неизвестно — как свежее", ok(sent = 0))
    }
}
