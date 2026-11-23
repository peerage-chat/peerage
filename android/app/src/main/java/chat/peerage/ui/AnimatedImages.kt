package chat.peerage.ui

import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.graphics.drawable.Drawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.RememberObserver
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.nio.ByteBuffer

/**
 * Анимированная картинка (GIF, анимированный WebP) как [Painter] — ложится в обычный `Image`
 * со всеми его обрезками, `ContentScale` и `graphicsLayer` (зум и протяжка просмотрщика).
 * Android 9+ (`AnimatedImageDrawable`); ниже, для неанимированных байтов и пока идёт разбор —
 * null, и вызывающий рисует обычный битмап (первый кадр).
 *
 * [maxSide] — сторона показа: кадры декодируются уменьшенными до неё, а не в оригинале.
 */
@Composable
internal fun rememberAnimatedPainter(bytes: ByteArray?, maxSide: Dp): Painter? {
    val px = with(LocalDensity.current) { maxSide.roundToPx() }
    if (bytes == null || Build.VERSION.SDK_INT < 28 || !looksAnimated(bytes)) return null
    val drawable by produceState<AnimatedImageDrawable?>(null, bytes, px) {
        value = withContext(Dispatchers.Default) { decodeAnimated(bytes, px) }
    }
    val d = drawable ?: return null
    return remember(d) { AnimatedDrawablePainter(d) }
}

/** GIF или WebP с флагом анимации в VP8X: остальное декодировать как анимацию незачем. */
private fun looksAnimated(b: ByteArray): Boolean {
    val gif = b.size >= 6 && b[0] == 'G'.code.toByte() && b[1] == 'I'.code.toByte() && b[2] == 'F'.code.toByte()
    val webp = b.size >= 21 && String(b, 0, 4, Charsets.US_ASCII) == "RIFF" &&
        String(b, 8, 8, Charsets.US_ASCII) == "WEBPVP8X" && (b[20].toInt() and 0x02) != 0
    return gif || webp
}

/** null — не анимация (один кадр) или байты не разобрались: тогда рисуется битмап. */
private fun decodeAnimated(bytes: ByteArray, maxSide: Int): AnimatedImageDrawable? {
    if (Build.VERSION.SDK_INT < 28) return null
    return runCatching {
        val src = ImageDecoder.createSource(ByteBuffer.wrap(bytes))
        ImageDecoder.decodeDrawable(src) { decoder, info, _ ->
            // Шаг уменьшения — по меньшей стороне: кадр потом режется под квадрат пузыря.
            val side = minOf(info.size.width, info.size.height)
            decoder.setTargetSampleSize((side / maxSide.coerceAtLeast(1)).coerceAtLeast(1))
        } as? AnimatedImageDrawable
    }.getOrNull()
}

/**
 * Painter поверх [AnimatedImageDrawable]: кадры меняет сам drawable (свой поток декода), а
 * перерисовку Compose просит через счётчик, который дёргает `invalidateDrawable`. Играет,
 * пока painter в композиции: ушёл с экрана (лента выгрузила пузырь) — остановлен.
 */
private class AnimatedDrawablePainter(private val d: AnimatedImageDrawable) : Painter(), RememberObserver {
    private var frame by mutableIntStateOf(0)
    private val main = Handler(Looper.getMainLooper())
    private val callback = object : Drawable.Callback {
        override fun invalidateDrawable(who: Drawable) {
            frame++
        }

        override fun scheduleDrawable(who: Drawable, what: Runnable, `when`: Long) {
            main.postAtTime(what, `when`)
        }

        override fun unscheduleDrawable(who: Drawable, what: Runnable) {
            main.removeCallbacks(what)
        }
    }

    override val intrinsicSize: Size
        get() = Size(d.intrinsicWidth.toFloat(), d.intrinsicHeight.toFloat())

    override fun DrawScope.onDraw() {
        frame // чтение — подписка на смену кадра
        // Android 9–11 рисуют AnimatedImageDrawable в его собственном размере, границы не
        // растягивают: масштаб — холстом, границы — родные.
        val w = d.intrinsicWidth.coerceAtLeast(1)
        val h = d.intrinsicHeight.coerceAtLeast(1)
        d.setBounds(0, 0, w, h)
        scale(size.width / w, size.height / h, pivot = Offset.Zero) {
            drawIntoCanvas { c -> d.draw(c.nativeCanvas) }
        }
    }

    override fun onRemembered() {
        d.callback = callback
        d.start()
    }

    override fun onForgotten() = release()

    override fun onAbandoned() = release()

    private fun release() {
        d.stop()
        d.callback = null
    }
}
