package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test

class FastScrollTest {
    @Test
    fun trackEndsMapToFirstAndLast() {
        assertEquals(0, FastScroll.indexAt(0f, 1000))
        assertEquals(999, FastScroll.indexAt(1f, 1000))
        assertEquals(500, FastScroll.indexAt(0.5f, 1001))
    }

    @Test
    fun outOfTrackIsClamped() {
        assertEquals(0, FastScroll.indexAt(-0.3f, 10))
        assertEquals(9, FastScroll.indexAt(1.7f, 10))
    }

    @Test
    fun emptyOrSingleGalleryStaysAtTop() {
        assertEquals(0, FastScroll.indexAt(0.8f, 0))
        assertEquals(0f, FastScroll.fractionOf(0, 1))
    }

    @Test
    fun fractionIsInverseOfIndex() {
        assertEquals(0.5f, FastScroll.fractionOf(500, 1001), 1e-6f)
        assertEquals(1f, FastScroll.fractionOf(2000, 1001), 1e-6f)
    }
}
