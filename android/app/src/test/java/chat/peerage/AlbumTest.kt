package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AlbumTest {
    private fun counter(): () -> Long {
        var n = 0L
        return { ++n }
    }

    @Test
    fun photosSentTogetherShareOneAlbum() {
        assertEquals(listOf(1L, 1L, 1L), Album.ids(listOf(true, true, true), on = true, next = counter()))
    }

    @Test
    fun filesStayOutOfTheAlbum() {
        assertEquals(listOf(1L, 0L, 1L), Album.ids(listOf(true, false, true), on = true, next = counter()))
    }

    @Test
    fun singlePhotoIsNoAlbum() {
        assertEquals(listOf(0L, 0L), Album.ids(listOf(true, false), on = true, next = counter()))
    }

    @Test
    fun switchedOffSendsSingly() {
        assertEquals(listOf(0L, 0L), Album.ids(listOf(true, true), on = false, next = counter()))
    }

    @Test
    fun longSeriesSplitsIntoAlbumsOfMax() {
        val ids = Album.ids(List(Album.MAX * 2 + 1) { true }, on = true, next = counter())
        assertEquals(List(Album.MAX) { 1L } + List(Album.MAX) { 2L } + 0L, ids)
    }

    @Test
    fun rowsKeepTheShortRowOnTop() {
        assertEquals(listOf(2), Album.rows(2))
        assertEquals(listOf(1, 2), Album.rows(3))
        assertEquals(listOf(2, 2), Album.rows(4))
        assertEquals(listOf(2, 3), Album.rows(5))
        assertEquals(listOf(3, 3), Album.rows(6))
        assertEquals(listOf(2, 2, 3), Album.rows(7))
        assertEquals(listOf(2, 2, 3, 3), Album.rows(10))
        (1..Album.MAX).forEach { assertEquals(it, Album.rows(it).sum()) }
    }

    @Test
    fun spanCoversNeighboursWithTheSameKey() {
        val feed = listOf(null, "a", "a", "a", "b", "b", null, "c")
        assertNull(Album.span(feed, 0) { it })
        assertEquals(1..3, Album.span(feed, 2) { it })
        assertEquals(4..5, Album.span(feed, 5) { it })
        // Одно сообщение с номером — не альбом.
        assertNull(Album.span(feed, 7) { it })
    }

    @Test
    fun longRunIsCutIntoAlbumsOfMax() {
        val feed = List(Album.MAX + 3) { "a" }
        assertEquals(0 until Album.MAX, Album.span(feed, 4) { it })
        assertEquals(Album.MAX until Album.MAX + 3, Album.span(feed, Album.MAX + 1) { it })
    }
}
