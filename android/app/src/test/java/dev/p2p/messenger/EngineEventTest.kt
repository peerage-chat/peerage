package dev.p2p.messenger

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Разбор событий движка. Тесты гоняют СЫРОЙ JSON — тот же, что печатает
 * `engine/src/dto.rs` (там его форма зафиксирована своими тестами). Пара тестов
 * с обеих сторон и есть защита от рассинхрона: переименуют поле в Rust — упадёт
 * тест там, забудут отразить здесь — упадёт этот.
 */
class EngineEventTest {
    private val peer = "aa11"

    @Test fun presence_parses() {
        val ev = parseEvent("""{"type":"presence","peer":"$peer","online":true}""")
        assertTrue(ev is EngineEvent.Presence)
        assertEquals(peer, (ev as EngineEvent.Presence).peer)
        assertTrue(ev.online)
    }

    @Test fun message_carries_the_stored_row() {
        // Строка истории целиком: подпись, «переслано», карточка вложения и курсор.
        val json = """
            {"type":"message","chat":"$peer","group":false,
             "msg":{"id":7,"sender":"$peer","text":"подпись","mine":false,"time":100,
                    "status":"","kind":"video","blob_id":"bb22","mime":"video/mp4",
                    "name":"clip.mp4","size":900,"duration_ms":12000,"width":1920,
                    "height":1080,"thumb_id":"cc33","downloaded":false,"reply_to":0,
                    "reply_sender":"","system":false,"fwd_from":"dd44","fwd_anon":false,
                    "edited_at":0,"row":5}}
        """.trimIndent()
        val ev = parseEvent(json) as EngineEvent.MessageEv
        assertEquals(peer, ev.chat)
        assertEquals(false, ev.group)
        val m = ev.msg.toModel()
        assertEquals("подпись", m.text)
        assertEquals("dd44", m.fwdFrom)
        assertEquals("bb22", m.blobId)
        assertEquals(900L, m.size)
        assertEquals(5L, m.row)
    }

    @Test fun group_message_parses() {
        val ev = parseEvent(
            """{"type":"message","chat":"ff","group":true,"msg":{"id":3,"sender":"aa","text":"привет","system":true}}""",
        ) as EngineEvent.MessageEv
        assertTrue(ev.group)
        assertEquals("привет", ev.msg.text)
        assertTrue(ev.msg.system)
    }

    @Test fun tag_only_event_parses() {
        assertTrue(parseEvent("""{"type":"groups_changed"}""") is EngineEvent.GroupsChanged)
    }

    @Test fun unknown_event_type_is_not_a_crash() {
        // Движок может начать слать событие раньше, чем клиент научится его
        // показывать. Это не повод ронять поток разбора.
        assertTrue(parseEvent("""{"type":"future_event","x":1}""") is EngineEvent.Unknown)
    }

    @Test fun malformed_json_is_not_a_crash() {
        assertTrue(parseEvent("{ not json") is EngineEvent.Unknown)
        assertTrue(parseEvent("") is EngineEvent.Unknown)
    }

    @Test fun outbox_stats_parses() {
        val s = decodeOrNull<OutboxStatsDto>("""{"pending":5,"bytes":90,"peers":2,"warn":true}""")!!
        assertEquals(5, s.pending)
        assertTrue(s.warn)
    }

    @Test fun storage_stats_parses_nested_shapes() {
        val json = """
            {"blobs":3,"blob_bytes":900,"outbox_pending":1,"outbox_bytes":10,
             "by_kind":{"image":700,"file":200},
             "chats":[{"peer":"aa","files":2,"bytes":700}]}
        """.trimIndent()
        val st = decodeOrNull<StorageStatsDto>(json)!!
        assertEquals(900L, st.blobBytes)
        assertEquals(700L, st.byKind["image"])
        assertEquals("aa", st.chats.single().peer)
    }

    @Test fun broken_list_yields_empty_not_exception() {
        assertTrue(decodeList<ContactDto>("{{{").isEmpty())
    }
}
