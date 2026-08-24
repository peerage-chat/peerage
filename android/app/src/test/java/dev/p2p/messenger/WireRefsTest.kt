package dev.p2p.messenger

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Граница с движком: форма ссылок на сообщения.
 *
 * Внутри клиента ссылка одна — пара «id + автор». На проводе форм две, и они
 * разные у каналов: личный спрашивает «моё ли», групповой — «чей». Эти тесты
 * держат форму: `androidjni` разбирает ровно поля `id`/`mine` и `id`/`sender`,
 * и переименование здесь молча сломало бы удаление и пересылку.
 */
class WireRefsTest {
    private val me = "aa11"
    private val peer = "bb22"

    @Test fun dm_ref_folds_author_into_mine() {
        val json = dmRefsJson(listOf(MsgRef(7, me)), me)
        assertEquals("""[{"id":7,"mine":true}]""", json)
    }

    @Test fun dm_ref_marks_foreign_message_as_not_mine() {
        val json = dmRefsJson(listOf(MsgRef(8, peer)), me)
        assertEquals("""[{"id":8,"mine":false}]""", json)
    }

    /** Пустой ключ — ещё не загруженная личность: чужим объявлять всё подряд
     *  нельзя, но и «моим» тоже. Проверяем, что своё при этом не потеряется. */
    @Test fun dm_ref_without_identity_calls_nothing_mine() {
        val json = dmRefsJson(listOf(MsgRef(9, me)), "")
        assertEquals("""[{"id":9,"mine":false}]""", json)
    }

    @Test fun group_ref_keeps_author() {
        val json = groupRefsJson(listOf(MsgRef(11, peer)))
        assertEquals("""[{"id":11,"sender":"bb22"}]""", json)
    }

    @Test fun refs_keep_order_and_count() {
        val json = groupRefsJson(listOf(MsgRef(1, me), MsgRef(2, peer), MsgRef(3, me)))
        assertEquals(
            """[{"id":1,"sender":"aa11"},{"id":2,"sender":"bb22"},{"id":3,"sender":"aa11"}]""",
            json,
        )
    }

    @Test fun empty_selection_gives_empty_array() {
        assertEquals("[]", dmRefsJson(emptyList(), me))
        assertEquals("[]", groupRefsJson(emptyList()))
    }
}
