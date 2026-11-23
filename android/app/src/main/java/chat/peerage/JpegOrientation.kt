package chat.peerage

/**
 * Поворот кадра JPEG (EXIF, тег 0x0112): камера часто пишет пиксели боком, а как держать
 * снимок — тегом. `BitmapFactory` тег не читает, поэтому без этого снимок в ленте лежит на
 * боку и его пропорции перепутаны. Зачистка метаданных (`blob::scrub`) оставляет от EXIF
 * только этот тег. Разбор чистый — без Android, проверяется юнит-тестом.
 */
object JpegOrientation {
    /** 1..8 по EXIF; 1 — как есть (не JPEG, тега нет, файл оборван). */
    fun read(b: ByteArray): Int {
        if (b.size < 4 || b.u8(0) != 0xFF || b.u8(1) != 0xD8) return 1
        var i = 2
        while (i + 4 <= b.size && b.u8(i) == 0xFF && b.u8(i + 1) != 0xDA) {
            val end = i + 2 + ((b.u8(i + 2) shl 8) or b.u8(i + 3))
            if (b.u8(i + 1) == 0xE1) exif(b, i + 4, minOf(end, b.size))?.let { return it }
            i = end
        }
        return 1
    }

    /** Четверть оборота (5..8): ширина и высота меняются местами. */
    fun swapsSides(o: Int): Boolean = o in 5..8

    private fun exif(b: ByteArray, from: Int, to: Int): Int? {
        val exifHead = "Exif\u0000\u0000".toByteArray()
        if (to - from < 14 || (0 until 6).any { b[from + it] != exifHead[it] }) return null
        val t = from + 6
        val le = when {
            b.u8(t) == 'I'.code && b.u8(t + 1) == 'I'.code -> true
            b.u8(t) == 'M'.code && b.u8(t + 1) == 'M'.code -> false
            else -> return null
        }
        fun u16(at: Int): Int? = if (at < t || at + 2 > to) {
            null
        } else if (le) {
            b.u8(at) or (b.u8(at + 1) shl 8)
        } else {
            (b.u8(at) shl 8) or b.u8(at + 1)
        }
        fun u32(at: Int): Long? {
            val lo = u16(if (le) at else at + 2) ?: return null
            val hi = u16(if (le) at + 2 else at) ?: return null
            return (hi.toLong() shl 16) or lo.toLong()
        }
        val ifd = u32(t + 4)?.takeIf { it < to - t } ?: return null
        val at = t + ifd.toInt()
        val n = u16(at) ?: return null
        for (k in 0 until n) {
            val e = at + 2 + k * 12
            val tag = u16(e) ?: return null
            if (tag == 0x0112 && u16(e + 2) == 3) return u16(e + 8)?.takeIf { it in 1..8 }
        }
        return null
    }

    private fun ByteArray.u8(i: Int): Int = this[i].toInt() and 0xFF
}
