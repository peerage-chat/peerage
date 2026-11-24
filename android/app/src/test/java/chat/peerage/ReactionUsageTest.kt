package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test

class ReactionUsageTest {
    @Test
    fun withoutHistoryTheStripIsTheDefaultSix() {
        assertEquals(ReactionUsage.DEFAULTS, ReactionUsage.quick(emptyMap()))
    }

    @Test
    fun frequentReactionsComeFirstAndPushDefaultsOut() {
        val quick = ReactionUsage.quick(mapOf("🔥" to 5, "🙏" to 2, "🤝" to 1))
        assertEquals(listOf("🔥", "🙏", "🤝", "👍", "❤️", "😂"), quick)
    }

    @Test
    fun equalCountsKeepTheDefaultOrderThenAStableOne() {
        val quick = ReactionUsage.quick(mapOf("😢" to 3, "👍" to 3, "🤝" to 3, "🎉" to 3))
        assertEquals(listOf("👍", "😢", "🎉", "🤝", "❤️", "😂"), quick)
    }

    @Test
    fun countsSurviveTheRoundTripThroughPrefs() {
        val counts = mapOf("👍" to 4, "❤️" to 1, "👨‍👩‍👧" to 2)
        assertEquals(counts, ReactionUsage.parse(ReactionUsage.format(counts)))
        assertEquals(emptyMap<String, Int>(), ReactionUsage.parse(null))
        assertEquals(mapOf("👍" to 1), ReactionUsage.parse("👍=1 мусор =3 😂=x"))
    }
}
