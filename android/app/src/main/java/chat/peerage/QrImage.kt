package chat.peerage

import android.content.Context
import android.net.Uri
import androidx.compose.ui.graphics.asAndroidBitmap
import chat.peerage.ui.Bitmaps
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.NotFoundException
import com.google.zxing.RGBLuminanceSource
import com.google.zxing.common.GlobalHistogramBinarizer
import com.google.zxing.common.HybridBinarizer

/**
 * QR с готового снимка или скриншота: код контакта прислали картинкой, а не текстом. Снимок
 * уменьшается до [MAX_SIDE] — код на скриншоте телефона читается с запасом, а 50 Мп с камеры
 * не разворачиваются целиком. Разбор пикселей — без Android, проверяется юнит-тестом.
 */
object QrImage {
    private const val MAX_SIDE = 2048

    private val hints = mapOf(
        DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
        DecodeHintType.TRY_HARDER to true,
    )

    /** Текст кода на картинке [uri]; `null` — не картинка или кода на ней нет. Не с главного потока. */
    fun fromUri(ctx: Context, uri: Uri): String? {
        val bytes = runCatching { ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() ?: return null
        val bmp = Bitmaps.decode(bytes, MAX_SIDE, MAX_SIDE)?.asAndroidBitmap() ?: return null
        val px = IntArray(bmp.width * bmp.height)
        bmp.getPixels(px, 0, bmp.width, 0, 0, bmp.width, bmp.height)
        return fromPixels(px, bmp.width, bmp.height)
    }

    /** Текст кода в пикселях ARGB [w]×[h]; `null` — кода нет. */
    fun fromPixels(px: IntArray, w: Int, h: Int): String? {
        val src = RGBLuminanceSource(w, h, px)
        // Гибридный бинаризатор — для фото с тенями; глобальный добирает ровные скриншоты,
        // на которых гибридный иногда теряет мелкий код.
        for (bin in listOf(HybridBinarizer(src), GlobalHistogramBinarizer(src))) {
            try {
                return MultiFormatReader().decode(BinaryBitmap(bin), hints).text
            } catch (_: NotFoundException) {
                // нет кода этим способом — пробуем следующий
            }
        }
        return null
    }
}
