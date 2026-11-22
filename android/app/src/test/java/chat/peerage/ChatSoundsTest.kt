package chat.peerage

import chat.peerage.ChatSounds.Kind
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Правило звуков в чате (notifications.md → «Звуки в приложении»). */
class ChatSoundsTest {
    private val now = 10_000_000L
    private val long = now - 60_000L // давно не звучал

    private fun ok(
        kind: Kind = Kind.In,
        enabled: Boolean = true,
        quiet: Boolean = false,
        muted: Boolean = false,
        sent: Long = now - 1_000,
        last: Long = long,
    ) = ChatSounds.allowed(kind, enabled, quiet, muted, sent, now, last)

    @Test
    fun freshIncomingSounds() = assertTrue(ok())

    @Test
    fun switchAndSilentModeSilence() {
        assertFalse(ok(enabled = false))
        assertFalse(ok(quiet = true))
        assertFalse(ok(kind = Kind.Out, quiet = true))
    }

    @Test
    fun backlogOlderThanTwoMinutesIsSilent() {
        assertFalse(ok(sent = now - ChatSounds.BACKLOG_MS - 1))
        assertTrue(ok(sent = now - ChatSounds.BACKLOG_MS + 1_000))
        assertTrue("время неизвестно (старый клиент) — как свежее", ok(sent = 0))
    }

    @Test
    fun burstSoundsOnce() {
        assertFalse(ok(last = now - 500))
        assertTrue(ok(last = now - ChatSounds.THROTTLE_MS))
    }

    @Test
    fun mutedChatSilencesOnlyIncoming() {
        assertFalse(ok(muted = true))
        assertTrue(ok(kind = Kind.Out, muted = true))
    }
}
