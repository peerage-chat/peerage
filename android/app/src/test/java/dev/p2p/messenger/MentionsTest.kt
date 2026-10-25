package dev.p2p.messenger

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Упоминания на границе с движком: участки там в байтах UTF-8, здесь — в индексах
 * UTF-16. Ошибка на единицу — и подсветка съезжает на соседнюю букву, а движок
 * молча отбрасывает упоминание как кривое.
 */
class MentionsTest {
    private val bob = "b".repeat(64)
    private val ann = "a".repeat(64)

    @Test fun offsets_round_trip_through_cyrillic_and_emoji() {
        val text = "👋 @Боб"
        val at = text.indexOf('@') // 3: эмодзи — два char, пробел — один
        assertEquals(5, Mentions.utf8Len(text, at)) // 4 байта эмодзи + пробел
        assertEquals(at, Mentions.utf16Index(text, 5))
        assertEquals(text.length, Mentions.utf16Index(text, Mentions.utf8Len(text, text.length)))
        // Посреди «Б» (2 байта) — не граница символа.
        assertNull(Mentions.utf16Index(text, 7))
    }

    @Test fun collect_finds_picked_names_in_final_text() {
        val text = "@Боб и @Аня К., гляньте"
        val got = Mentions.collect(text, mapOf(bob to "Боб", ann to "Аня К."))
        assertEquals(listOf(bob, ann), got.map { it.key })
        assertEquals(0, got[0].at)
        assertEquals("@Боб".toByteArray().size, got[0].len)
        assertEquals("@Боб и ".toByteArray().size, got[1].at)
        // И обратно в индексы строки — ровно те же участки.
        val ranges = Mentions.ranges(text, got)
        assertEquals("@Боб", text.substring(ranges[0].first))
        assertEquals("@Аня К.", text.substring(ranges[1].first))
    }

    @Test fun erased_or_extended_name_is_not_a_mention() {
        assertEquals(emptyList<Mention>(), Mentions.collect("привет всем", mapOf(bob to "Боб")))
        assertEquals(emptyList<Mention>(), Mentions.collect("@Бобёр", mapOf(bob to "Боб")))
    }

    @Test fun longer_label_wins_over_its_prefix() {
        val got = Mentions.collect("@Аня К.", mapOf(bob to "Аня", ann to "Аня К."))
        assertEquals(listOf(ann), got.map { it.key })
    }

    @Test fun query_only_at_word_start() {
        assertEquals(0 to "Бо", Mentions.query("@Бо", 3))
        assertEquals(4 to "", Mentions.query("hi, @", 5))
        assertNull(Mentions.query("mail@host", 9)) // адрес почты, не упоминание
        assertNull(Mentions.query("@Боб привет", 11)) // после пробела запрос кончился
    }

    @Test fun picked_restores_labels_for_edit() {
        val text = "ну что, @Боб?"
        val m = Mentions.collect(text, mapOf(bob to "Боб"))
        assertEquals(mapOf(bob to "Боб"), Mentions.picked(text, m))
    }

    @Test fun json_matches_engine_shape() {
        assertEquals("""[{"at":0,"len":7,"key":"$bob"}]""", Mentions.toJson(listOf(Mention(0, 7, bob))))
    }
}
