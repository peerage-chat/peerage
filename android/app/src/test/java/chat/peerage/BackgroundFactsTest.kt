package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Полоса «работа в фоне» на главной (п. 50, app-ui.md → «Работа в фоне»). */
class BackgroundFactsTest {
    private val day = 86_400L
    private val t0 = 1_000L * day

    private fun BgFacts.at(now: Long = t0, limited: Boolean = false, vendor: Boolean = true, autostart: Boolean = true) =
        strip(now, androidLimited = limited, hasVendor = vendor, hasAutostart = autostart)

    @Test
    fun android_limit_comes_first_and_stays_until_lifted() {
        val f = BgFacts(vendorOpened = true, autostartOpened = true)
        assertEquals(BgStrip.ANDROID, f.at(limited = true))
        assertEquals(BgStrip.ANDROID, f.opened(BgStrip.ANDROID).at(limited = true))
        assertNull(f.at(limited = false))
    }

    @Test
    fun firmware_screens_go_one_by_one_once_opened() {
        val f = BgFacts()
        assertEquals(BgStrip.VENDOR, f.at())
        val v = f.opened(BgStrip.VENDOR)
        assertEquals(BgStrip.AUTOSTART, v.at())
        assertNull(v.opened(BgStrip.AUTOSTART).at())
    }

    @Test
    fun stock_android_without_limit_shows_nothing() {
        assertNull(BgFacts().at(vendor = false, autostart = false))
    }

    @Test
    fun later_hides_for_three_days() {
        val f = BgFacts().later(t0)
        assertNull(f.at(now = t0 + 3 * day - 1, limited = true))
        assertEquals(BgStrip.ANDROID, f.at(now = t0 + 3 * day, limited = true))
    }
}
