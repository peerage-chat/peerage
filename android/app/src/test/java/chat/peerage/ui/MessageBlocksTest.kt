package chat.peerage.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Calendar
import java.util.TimeZone

/**
 * Склейка сообщений в блоки — то, из-за чего пузыри срастаются углами.
 *
 * Правило легко сломать незаметно: ошибка в нём не роняет приложение, а лишь
 * рвёт блок или, наоборот, склеивает чужое со своим — заметить это можно только
 * глазами на конкретной переписке. Поэтому оно под тестом отдельно от разметки:
 * скоро личный чат и группа будут рисоваться одним кодом, и правило должно
 * пережить переезд без изменений.
 */
class MessageBlocksTest {
    private val alice = "aa11"
    private val bob = "bb22"

    /** Полдень заданного дня — чтобы окно в пять минут не задевало границу суток. */
    private fun noon(day: Int): Long {
        val c = Calendar.getInstance(TimeZone.getDefault())
        c.set(2026, Calendar.AUGUST, day, 12, 0, 0)
        c.set(Calendar.MILLISECOND, 0)
        return c.timeInMillis / 1000
    }

    @Test fun same_author_within_window_continues_block() {
        val t = noon(10)
        assertTrue(continuesBlock(alice, t, alice, t + 60))
    }

    @Test fun different_author_breaks_block() {
        val t = noon(10)
        assertFalse(continuesBlock(alice, t, bob, t + 60))
    }

    @Test fun gap_beyond_window_breaks_block() {
        val t = noon(10)
        assertFalse(continuesBlock(alice, t, alice, t + BLOCK_WINDOW_SEC))
        assertTrue(continuesBlock(alice, t, alice, t + BLOCK_WINDOW_SEC - 1))
    }

    /** День сменился — блок рвётся, даже если сообщения рядом по времени: между
     *  ними встанет ярлык даты, и склеенные углы разъедут его пополам. */
    @Test fun day_change_breaks_block_even_within_window() {
        val c = Calendar.getInstance(TimeZone.getDefault())
        c.set(2026, Calendar.AUGUST, 10, 23, 59, 0)
        c.set(Calendar.MILLISECOND, 0)
        val before = c.timeInMillis / 1000
        assertFalse(continuesBlock(alice, before, alice, before + 120))
    }

    /** Системная строка («Вера вошла в группу») передаётся как null-автор. */
    @Test fun system_line_breaks_block() {
        val t = noon(10)
        assertFalse(continuesBlock(null, t, alice, t + 10))
    }

    @Test fun positions_cover_single_first_middle_last() {
        assertEquals(BlockPos.Solo, blockPos(withPrev = false, withNext = false))
        assertEquals(BlockPos.First, blockPos(withPrev = false, withNext = true))
        assertEquals(BlockPos.Mid, blockPos(withPrev = true, withNext = true))
        assertEquals(BlockPos.Last, blockPos(withPrev = true, withNext = false))
    }

    /** Внутри блока зазор меньше, чем между блоками, — иначе склейка не читается. */
    @Test fun gap_inside_block_is_smaller_than_between_blocks() {
        assertTrue(blockGap(BlockPos.Mid).value < blockGap(BlockPos.Last).value)
        assertTrue(blockGap(BlockPos.First).value < blockGap(BlockPos.Solo).value)
    }
}
