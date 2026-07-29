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

    @Test fun media_parses_all_card_fields() {
        val json = """
            {"type":"media","peer":"$peer","id":7,"time":100,"kind":"video",
             "blob_id":"bb22","mime":"video/mp4","name":"clip.mp4","size":900,
             "duration_ms":12000,"width":1920,"height":1080,"thumb_id":"cc33",
             "downloaded":false,"auto":false}
        """.trimIndent()
        val ev = parseEvent(json) as EngineEvent.Media
        assertEquals("bb22", ev.blobId)
        assertEquals("cc33", ev.thumbId)
        assertEquals(900L, ev.size)
        assertEquals(12000, ev.durationMs)
        assertEquals(1920, ev.width)
        assertEquals(false, ev.auto)
    }

    @Test fun tag_only_event_parses() {
        assertTrue(parseEvent("""{"type":"groups_changed"}""") is EngineEvent.GroupsChanged)
    }

    @Test fun group_msg_own_echo_has_only_gid_and_mine() {
        // Своё групповое приходит короткой формой: остальные поля не сериализуются.
        val ev = parseEvent("""{"type":"group_msg","gid":"ff","mine":true}""") as EngineEvent.GroupMsgEv
        assertEquals("ff", ev.gid)
        assertTrue(ev.mine)
        assertEquals("", ev.text)
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
