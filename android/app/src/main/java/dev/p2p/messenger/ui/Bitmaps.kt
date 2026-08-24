package dev.p2p.messenger.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.runtime.Composable
import androidx.compose.runtime.produceState
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Декодирование картинок под РАЗМЕР ПОКАЗА, с общим кэшем.
 *
 * До этого каждое место рисования звало `BitmapFactory.decodeByteArray` без опций:
 * снимок 1080×2400 разворачивался в 10.4 МБ пикселей независимо от того, что его
 * показывают в пузыре 240dp (замер: открытие чата с пятью вложениями добавляло
 * +51 МБ нативной памяти, то есть ровно пять полноразмерных битмапов). Фото с
 * телефона на 12 Мп стоило бы 48 МБ штука.
 *
 * Здесь байты декодируются с `inSampleSize` и, если нужно, дожимаются до целевого
 * размера, а результат живёт в LRU с потолком от heap приложения. Ключ кэша — хэш
 * блоба плюс целевой размер: один и тот же снимок в ленте и в просмотрщике это два
 * разных предмета, и мельче не должен вытеснять крупнее.
 */
internal object Bitmaps {
    /** Восьмая часть кучи — обычная доля под изображения; остальное нужно самому
     *  приложению (история, состояние Compose, сетевые буферы). */
    private val cache = object : LruCache<String, ImageBitmap>(
        ((Runtime.getRuntime().maxMemory() / 8) / 1024).toInt().coerceAtLeast(8 * 1024),
    ) {
        /** LruCache считает в килобайтах — иначе потолок не выразить в байтах heap. */
        override fun sizeOf(key: String, value: ImageBitmap): Int =
            (value.width.toLong() * value.height * 4 / 1024).toInt().coerceAtLeast(1)
    }

    fun get(key: String): ImageBitmap? = cache.get(key)

    fun put(key: String, bmp: ImageBitmap) {
        cache.put(key, bmp)
    }

    /** Сбросить кэш (выход из аккаунта, нехватка памяти). */
    fun clear() = cache.evictAll()

    /**
     * Во сколько раз уменьшать при декоде. Степень двойки — единственное, что
     * понимает `inSampleSize`; остальное добирается масштабированием.
     */
    fun sampleFor(width: Int, height: Int, maxW: Int, maxH: Int): Int {
        var sample = 1
        var w = width
        var h = height
        while (w / 2 >= maxW && h / 2 >= maxH) {
            w /= 2
            h /= 2
            sample *= 2
        }
        return sample
    }

    /**
     * Декод под ограничение `maxW × maxH`. Пропорции сохраняются; картинка меньше
     * ограничения не растягивается — увеличит её уже сам вывод.
     */
    fun decode(bytes: ByteArray, maxW: Int, maxH: Int): ImageBitmap? = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        val opts = BitmapFactory.Options().apply {
            inSampleSize = sampleFor(bounds.outWidth, bounds.outHeight, maxW, maxH)
        }
        val raw = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts) ?: return null
        // `inSampleSize` даёт только степень двойки, поэтому после него картинка
        // может остаться заметно крупнее нужного — дожимаем и отпускаем исходник.
        val k = minOf(maxW.toFloat() / raw.width, maxH.toFloat() / raw.height, 1f)
        if (k >= 1f) {
            raw.asImageBitmap()
        } else {
            val scaled = Bitmap.createScaledBitmap(
                raw,
                (raw.width * k).toInt().coerceAtLeast(1),
                (raw.height * k).toInt().coerceAtLeast(1),
                true,
            )
            if (scaled !== raw) raw.recycle()
            scaled.asImageBitmap()
        }
    }.getOrNull()
}

/**
 * Картинка под размер показа: сперва кэш, иначе декод в фоне.
 *
 * Декод синхронно в композиции держал кадр отрисовки; здесь он уходит на
 * `Dispatchers.Default`, а до готовности вызывающий получает null и рисует своё
 * пустое место.
 *
 * [cacheKey] — хэш блоба: байты одного вложения приходят одним и тем же массивом,
 * но `remember` по массиву не переживает выход из чата, а кэш переживает.
 */
@Composable
internal fun rememberBitmap(
    bytes: ByteArray?,
    cacheKey: String?,
    maxWidth: Dp,
    maxHeight: Dp = maxWidth,
): ImageBitmap? {
    val density = LocalDensity.current
    val maxW = with(density) { maxWidth.roundToPx() }
    val maxH = with(density) { maxHeight.roundToPx() }
    val key = (cacheKey?.takeIf { it.isNotEmpty() } ?: bytes?.contentHashCode()?.toString())
        ?.let { "$it@${maxW}x$maxH" }
    if (bytes == null || key == null) return null
    return produceState<ImageBitmap?>(Bitmaps.get(key), key, bytes) {
        if (value == null) {
            value = withContext(Dispatchers.Default) { Bitmaps.decode(bytes, maxW, maxH) }
                ?.also { Bitmaps.put(key, it) }
        }
    }.value
}

/** Та же картинка, но под весь экран — для полноэкранного просмотра. */
@Composable
internal fun rememberFullscreenBitmap(bytes: ByteArray?, cacheKey: String?): ImageBitmap? {
    val cfg = LocalConfiguration.current
    return rememberBitmap(bytes, cacheKey, cfg.screenWidthDp.dp, cfg.screenHeightDp.dp)
}

/**
 * Первый кадр видео под размер показа.
 *
 * Раньше это делалось прямо в композиции: файл целиком писался на диск, кадр
 * доставался `MediaMetadataRetriever` и оставался в полном разрешении — всё
 * синхронно, в потоке отрисовки. Теперь это фоновая работа с тем же кэшем.
 */
@Composable
internal fun rememberVideoFrame(bytes: ByteArray?, cacheKey: String?, maxWidth: Dp): ImageBitmap? {
    val maxPx = with(LocalDensity.current) { maxWidth.roundToPx() }
    val key = (cacheKey?.takeIf { it.isNotEmpty() } ?: bytes?.contentHashCode()?.toString())
        ?.let { "frame:$it@$maxPx" }
    if (bytes == null || key == null) return null
    return produceState<ImageBitmap?>(Bitmaps.get(key), key, bytes) {
        if (value == null) {
            value = withContext(Dispatchers.IO) { videoFrame(bytes, maxPx) }
                ?.also { Bitmaps.put(key, it) }
        }
    }.value
}

private fun videoFrame(bytes: ByteArray, maxPx: Int): ImageBitmap? = runCatching {
    val f = java.io.File.createTempFile("frame", null)
    try {
        f.writeBytes(bytes)
        android.media.MediaMetadataRetriever().use { r ->
            r.setDataSource(f.absolutePath)
            // Начиная с Android 8.1 система сама отдаёт уменьшенный кадр — это
            // дешевле, чем декодировать полный и жать его потом.
            val raw = if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O_MR1) {
                r.getScaledFrameAtTime(0, android.media.MediaMetadataRetriever.OPTION_CLOSEST_SYNC, maxPx, maxPx)
            } else {
                r.frameAtTime
            } ?: return null
            val k = minOf(maxPx.toFloat() / raw.width, maxPx.toFloat() / raw.height, 1f)
            if (k >= 1f) {
                raw.asImageBitmap()
            } else {
                val scaled = Bitmap.createScaledBitmap(
                    raw,
                    (raw.width * k).toInt().coerceAtLeast(1),
                    (raw.height * k).toInt().coerceAtLeast(1),
                    true,
                )
                if (scaled !== raw) raw.recycle()
                scaled.asImageBitmap()
            }
        }
    } finally {
        f.delete()
    }
}.getOrNull()
