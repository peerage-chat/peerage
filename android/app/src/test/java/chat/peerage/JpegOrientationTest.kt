package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Test

class JpegOrientationTest {
    /** JPEG с одним APP1: TIFF в порядке [order], в IFD0 — Make и поворот [o]. */
    private fun jpeg(o: Int, order: String): ByteArray {
        val le = order == "II"
        fun u16(v: Int) = if (le) byteArrayOf(v.toByte(), (v shr 8).toByte()) else byteArrayOf((v shr 8).toByte(), v.toByte())
        fun u32(v: Int) = if (le) u16(v) + u16(0) else u16(0) + u16(v)
        val tiff = order.toByteArray() + u16(42) + u32(8) + u16(2) +
            u16(0x010F) + u16(2) + u32(4) + "Mi\u0000\u0000".toByteArray() +
            u16(0x0112) + u16(3) + u32(1) + u16(o) + u16(0) +
            u32(0)
        val app1 = "Exif\u0000\u0000".toByteArray() + tiff
        val len = app1.size + 2
        return byteArrayOf(0xFF.toByte(), 0xD8.toByte(), 0xFF.toByte(), 0xE1.toByte(), (len shr 8).toByte(), len.toByte()) +
            app1 + byteArrayOf(0xFF.toByte(), 0xDA.toByte(), 0, 2, 0x12, 0x34, 0xFF.toByte(), 0xD9.toByte())
    }

    @Test
    fun readsOrientationInBothByteOrders() {
        assertEquals(6, JpegOrientation.read(jpeg(6, "II")))
        assertEquals(8, JpegOrientation.read(jpeg(8, "MM")))
    }

    @Test
    fun defaultsToUprightWhenAbsentOrBroken() {
        assertEquals(1, JpegOrientation.read(byteArrayOf(0xFF.toByte(), 0xD8.toByte(), 0xFF.toByte(), 0xD9.toByte())))
        assertEquals(1, JpegOrientation.read("GIF89a".toByteArray()))
        assertEquals(1, JpegOrientation.read(jpeg(6, "II").copyOf(20))) // оборван посреди EXIF
        assertEquals(1, JpegOrientation.read(jpeg(42, "II"))) // вне 1..8
    }

    @Test
    fun sidesSwapOnlyForQuarterTurns() {
        assertEquals(listOf(5, 6, 7, 8), (1..8).filter { JpegOrientation.swapsSides(it) })
    }
}
