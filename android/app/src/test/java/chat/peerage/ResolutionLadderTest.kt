package chat.peerage

import chat.peerage.ResolutionLadder.Size
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Ступени разрешения под оценку полосы: вниз — быстро и сразу куда надо, вверх —
 * медленно и по одной; дребезг цели около порога не дёргает кодер.
 */
class ResolutionLadderTest {
    private val camera = listOf(
        Size(1920, 1080),
        Size(1280, 720),
        Size(1024, 576),
        Size(960, 540),
        Size(864, 480),
        Size(640, 480), // 4:3 — не наш аспект
        Size(640, 360),
        Size(480, 270),
        Size(320, 180),
        Size(176, 144),
    )

    private fun ladder(top: Size = Size(1280, 720), max: Int = 2_000_000) =
        ResolutionLadder(ResolutionLadder.stepsFrom(top, camera), 30, max)

    /** Feed the same target once a second for [secs] seconds starting at [fromMs]. */
    private fun ResolutionLadder.feed(target: Int, fromMs: Long, secs: Int): Long {
        var t = fromMs
        repeat(secs) {
            update(target, t)
            t += 1000
        }
        return t
    }

    @Test fun steps_keep_aspect_and_shrink_enough() {
        val steps = ResolutionLadder.stepsFrom(Size(1280, 720), camera)
        assertEquals(
            listOf(Size(1280, 720), Size(960, 540), Size(640, 360), Size(480, 270), Size(320, 180)),
            steps,
        )
    }

    @Test fun thresholds_follow_the_table_and_respect_the_cap() {
        val l = ladder()
        assertEquals(1_500_000, l.upBitrate(0))
        assertEquals(800_000, l.upBitrate(1))
        assertEquals(500_000, l.upBitrate(2))
        assertEquals(900_000, l.downBitrate(0))
        // «Низкий» битрейт в настройках: к выбранному разрешению всё равно можно вернуться.
        val low = ladder(max = 1_000_000)
        assertEquals(900_000, low.upBitrate(0))
    }

    @Test fun drop_goes_straight_to_a_fitting_step_after_a_short_hold() {
        val l = ladder()
        var t = l.feed(2_000_000, 0, 10)
        assertEquals(0, l.current)
        l.update(450_000, t)
        assertEquals("одной секунды мало", 0, l.current)
        t = l.feed(450_000, t + 1000, 2)
        // 450к: 360p нужно 500к — мимо; 270p (300к) — да.
        assertEquals(3, l.current)
    }

    @Test fun recovery_climbs_one_step_at_a_time_and_slowly() {
        val l = ladder()
        var t = l.feed(250_000, 0, 5)
        assertEquals(4, l.current)
        // Полоса вернулась: каждые ≥ 8 с — на ступень выше.
        val history = mutableListOf<Int>()
        repeat(40) {
            l.update(3_000_000, t)
            history += l.current
            t += 1000
        }
        assertEquals(0, l.current)
        val changes = history.zipWithNext().count { (a, b) -> a != b }
        assertEquals("по одной ступени", 4, changes)
        assertTrue("не быстрее чем за 8 с", history.take(7).all { it == 4 })
    }

    @Test fun target_jitter_around_a_threshold_does_not_flap() {
        val l = ladder()
        var t = 0L
        // 850к–950к: ниже порога «вниз» (900к) — то на секунду, то нет.
        repeat(60) { i ->
            l.update(if (i % 2 == 0) 850_000 else 950_000, t)
            t += 1000
        }
        assertEquals(0, l.current)
    }

    @Test fun unknown_target_changes_nothing() {
        val l = ladder()
        l.feed(0, 0, 30)
        assertEquals(0, l.current)
    }
}
