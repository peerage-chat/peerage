package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test

class DragRangeTest {
    private val order = (1L..6L).map { MsgRef(it, "s") }
    private fun refs(vararg ids: Long) = ids.map { MsgRef(it, "s") }.toSet()

    @Test
    fun dragSelectsEverythingBetweenAnchorAndFinger() {
        // Вниз по ленте и вверх — одинаково: диапазон от первого до текущего.
        assertEquals(refs(2, 3, 4, 5), DragRange.apply(emptySet(), order, MsgRef(2, "s"), MsgRef(5, "s"), adding = true))
        assertEquals(refs(2, 3, 4), DragRange.apply(emptySet(), order, MsgRef(4, "s"), MsgRef(2, "s"), adding = true))
    }

    @Test
    fun goingBackRestoresWhatWasSelectedBefore() {
        val before = refs(6)
        assertEquals(refs(1, 2, 6), DragRange.apply(before, order, MsgRef(1, "s"), MsgRef(2, "s"), adding = true))
    }

    @Test
    fun startingOnSelectedMessageClearsTheRange() {
        assertEquals(refs(1, 6), DragRange.apply(refs(1, 3, 4, 6), order, MsgRef(3, "s"), MsgRef(5, "s"), adding = false))
    }

    @Test
    fun fingerOutsideKnownMessagesKeepsJustTheAnchor() {
        assertEquals(refs(3), DragRange.apply(emptySet(), order, MsgRef(3, "s"), MsgRef(99, "x"), adding = true))
    }
}
