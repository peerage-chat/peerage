package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** «Отменить» в снекбаре: действие выполняется один раз — по истечении, досрочно или никогда. */
class UndoSlotTest {
    private val log = mutableListOf<String>()
    private fun offer(slot: UndoSlot, name: String) =
        slot.offer(name, commit = { log += "commit $name" }, revert = { log += "revert $name" })

    @Test
    fun expiryCommitsOnce() {
        val slot = UndoSlot()
        offer(slot, "a")
        val a = slot.current!!
        slot.expire(a)
        slot.expire(a)
        assertEquals(listOf("commit a"), log)
        assertNull(slot.current)
    }

    @Test
    fun undoRevertsAndNothingIsCommitted() {
        val slot = UndoSlot()
        offer(slot, "a")
        val a = slot.current!!
        slot.undo()
        slot.expire(a) // таймер опоздал — уже отменено
        assertEquals(listOf("revert a"), log)
    }

    @Test
    fun newOfferCommitsThePreviousAndOldTimerDoesNotTouchTheNew() {
        val slot = UndoSlot()
        offer(slot, "a")
        val a = slot.current!!
        offer(slot, "b")
        slot.expire(a)
        assertEquals(listOf("commit a"), log)
        assertEquals("b", slot.current!!.text)
    }

    @Test
    fun flushCommitsWhatIsPending() {
        val slot = UndoSlot()
        offer(slot, "a")
        slot.flush()
        slot.flush()
        assertEquals(listOf("commit a"), log)
        assertNull(slot.current)
    }
}
