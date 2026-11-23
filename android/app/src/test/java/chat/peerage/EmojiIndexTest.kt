package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class EmojiIndexTest {
    private val sample = EmojiIndex.parse(
        """
        # шапка
        0	😀	широко улыбается|grinning face	лицо|улыбка|smile
        0	😺	улыбающийся кот|grinning cat	кот|cat
        2	🐱	морда кошки|cat face	кот|кошка
        2	🐈	кошка|cat	кот
        7	❤️	красное сердце|red heart	любовь|сердце|heart|love
        3	🍓	клубника|strawberry	ягода
        """.trimIndent(),
    )

    @Test
    fun parsesGroupsAndSkipsComments() {
        assertEquals(listOf(0, 0, 2, 2, 7, 3), sample.map { it.group })
    }

    @Test
    fun findsByRussianAndEnglishPrefix() {
        assertEquals(listOf("🐈", "🐱", "😺"), EmojiIndex.search(sample, "cat"))
        assertEquals(listOf("🐈", "🐱"), EmojiIndex.search(sample, "кош"))
        assertEquals(listOf("❤️"), EmojiIndex.search(sample, "HEA"))
        assertEquals(listOf("😺", "😀"), EmojiIndex.search(sample, "улыб"))
    }

    @Test
    fun yoEqualsYe() {
        assertEquals(listOf("🍓"), EmojiIndex.search(sample, "ягода"))
        assertEquals(EmojiIndex.search(sample, "серд"), EmojiIndex.search(sample, "сёрд"))
    }

    @Test
    fun allTokensMustMatchAndEmptyQueryFindsNothing() {
        assertEquals(listOf("🐱"), EmojiIndex.search(sample, "морда кош"))
        assertEquals(emptyList<String>(), EmojiIndex.search(sample, "  "))
        assertEquals(emptyList<String>(), EmojiIndex.search(sample, "кот ягода"))
    }

    @Test
    fun bundledSetIsComplete() {
        // Набор в assets: все группы, тысячи символов, у каждого есть слова — иначе поиск слеп.
        val all = EmojiIndex.parse(File("src/main/assets/emoji.tsv").readText())
        assertTrue(all.size.toString(), all.size > 1800)
        assertEquals((0..8).toSet(), all.map { it.group }.toSet())
        assertTrue(all.all { it.names.isNotEmpty() })
        val up = EmojiIndex.search(all, "палец вверх").take(3)
        assertTrue(up.toString(), "👍" in up)
        assertTrue(EmojiIndex.search(all, "флаг", limit = 1000).size > 200)
    }
}
