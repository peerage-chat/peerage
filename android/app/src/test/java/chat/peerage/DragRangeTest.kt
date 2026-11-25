package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DragRangeTest {
    private fun ref(id: Long) = MsgRef(id, "s")
    private val order = (1L..6L).map { listOf(ref(it)) }
    private fun refs(vararg ids: Long) = ids.map { ref(it) }.toSet()

    // Лента с альбомом: 2, 3, 4 — один ряд.
    private val withAlbum = listOf(listOf(ref(1)), listOf(ref(2), ref(3), ref(4)), listOf(ref(5)))

    @Test
    fun dragSelectsEverythingBetweenAnchorAndFinger() {
        // Вниз по ленте и вверх — одинаково: диапазон от первого до текущего.
        assertEquals(refs(2, 3, 4, 5), DragRange.apply(emptySet(), order, ref(2), ref(5), adding = true))
        assertEquals(refs(2, 3, 4), DragRange.apply(emptySet(), order, ref(4), ref(2), adding = true))
    }

    @Test
    fun goingBackRestoresWhatWasSelectedBefore() {
        val before = refs(6)
        assertEquals(refs(1, 2, 6), DragRange.apply(before, order, ref(1), ref(2), adding = true))
    }

    @Test
    fun startingOnSelectedMessageClearsTheRange() {
        assertEquals(refs(1, 6), DragRange.apply(refs(1, 3, 4, 6), order, ref(3), ref(5), adding = false))
    }

    @Test
    fun fingerOutsideKnownMessagesKeepsJustTheAnchorRow() {
        assertEquals(refs(3), DragRange.apply(emptySet(), order, ref(3), MsgRef(99, "x"), adding = true))
        assertEquals(refs(2, 3, 4), DragRange.apply(emptySet(), withAlbum, ref(2), MsgRef(99, "x"), adding = true))
    }

    @Test
    fun albumRowIsTakenWhole() {
        // Начали на альбоме — он выбран целиком; дотянули до альбома — тоже.
        assertEquals(refs(2, 3, 4), DragRange.apply(emptySet(), withAlbum, ref(2), ref(2), adding = true))
        assertEquals(refs(1, 2, 3, 4), DragRange.apply(emptySet(), withAlbum, ref(1), ref(2), adding = true))
        assertEquals(refs(2, 3, 4, 5), DragRange.apply(emptySet(), withAlbum, ref(5), ref(2), adding = true))
    }

    @Test
    fun rowIsSelectedOnlyWhenAllOfItIs() {
        assertFalse(DragRange.rowSelected(refs(2), withAlbum[1]))
        assertTrue(DragRange.rowSelected(refs(2, 3, 4), withAlbum[1]))
    }

    @Test
    fun toggleRowSelectsTheRestOrClearsWhenFull() {
        // Часть альбома выбрана — ряд добирается целиком; весь — снимается целиком.
        assertEquals(refs(2, 3, 4, 5), DragRange.toggleRow(refs(3, 5), withAlbum[1]))
        assertEquals(refs(5), DragRange.toggleRow(refs(2, 3, 4, 5), withAlbum[1]))
        assertEquals(refs(1), DragRange.toggleRow(emptySet(), listOf(ref(1))))
    }
}
