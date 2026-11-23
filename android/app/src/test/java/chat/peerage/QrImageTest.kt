package chat.peerage

import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QrImageTest {
    /** Скриншот: код [text] на белом поле [w]×[h], левый верхний угол кода в ([x], [y]). */
    private fun screenshot(text: String, w: Int, h: Int, x: Int, y: Int, side: Int): IntArray {
        val m = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, side, side)
        val px = IntArray(w * h) { 0xFFFFFFFF.toInt() }
        for (j in 0 until m.height) for (i in 0 until m.width) if (m[i, j]) px[(y + j) * w + x + i] = 0xFF000000.toInt()
        return px
    }

    @Test
    fun readsCodeFromScreenshot() {
        val code = "peerage:1:" + "ab".repeat(40)
        assertEquals(code, QrImage.fromPixels(screenshot(code, 720, 1600, 120, 500, 480), 720, 1600))
    }

    @Test
    fun noCodeIsNull() {
        val blank = IntArray(400 * 400) { 0xFF808080.toInt() }
        assertNull(QrImage.fromPixels(blank, 400, 400))
    }
}
