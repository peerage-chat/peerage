package chat.peerage

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RoomWatchTest {
    private val start = 1_000_000L
    private val warm = start + RoomWatch.WARMUP_MS + 1

    @Test
    fun emptyRoomComingAliveNotifies() {
        val w = RoomWatch(start)
        assertFalse(w.observe("g", false, warm))
        assertTrue(w.observe("g", true, warm + 3_000))
        assertFalse("уже живая — не заново", w.observe("g", true, warm + 6_000))
    }

    @Test
    fun roomAlreadyRunningAtStartDoesNot() {
        val w = RoomWatch(start)
        assertFalse(w.observe("g", false, start + 1_000))
        assertFalse("маячки ещё не дошли", w.observe("g", true, start + 5_000))
    }

    @Test
    fun firstSeenAliveAfterWarmupNotifies() = assertTrue(RoomWatch(start).observe("g", true, warm))

    @Test
    fun flickerIsNotComingAlive() {
        val w = RoomWatch(start)
        w.observe("g", false, warm)
        assertTrue(w.observe("g", true, warm + 1_000))
        w.observe("g", false, warm + 30_000)
        assertFalse("маячок задержался", w.observe("g", true, warm + 40_000))
        w.observe("g", false, warm + 50_000)
        assertTrue("пусто дольше паузы — ожила снова", w.observe("g", true, warm + 50_000 + RoomWatch.QUIET_MS + 1))
    }

    @Test
    fun ownRoomLeftWithOthersInsideDoesNot() {
        val w = RoomWatch(start)
        w.observe("g", true, warm) // свою комнату тикер отмечает живой
        assertFalse(w.observe("g", true, warm + 3_000))
    }

    @Test
    fun roomsAreIndependent() {
        val w = RoomWatch(start)
        w.observe("a", true, warm)
        assertTrue(w.observe("b", true, warm + 1_000))
    }
}
