package chat.peerage

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadataRetriever
import android.net.Uri
import java.io.ByteArrayOutputStream

/** Метаданные вложения для карточки. Движок медиа не декодирует — это делает клиент. */
data class MediaMeta(
    val durationMs: Int = 0,
    val width: Int = 0,
    val height: Int = 0,
    /** JPEG-миниатюра (null — нет). Уйдёт отдельным мелким блобом. */
    val thumb: ByteArray? = null,
) {
    // ByteArray ломает автогенерируемые equals/hashCode — сравниваем по содержимому.
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is MediaMeta) return false
        return durationMs == other.durationMs && width == other.width && height == other.height &&
            (thumb?.contentEquals(other.thumb) ?: (other.thumb == null))
    }

    override fun hashCode(): Int {
        var r = durationMs
        r = 31 * r + width
        r = 31 * r + height
        r = 31 * r + (thumb?.contentHashCode() ?: 0)
        return r
    }

    companion object {
        /** Сторона миниатюры. Мелкая намеренно: она едет отдельным блобом и качается
         *  сразу, поэтому не должна весить как само вложение. */
        private const val THUMB_MAX = 320
        private const val THUMB_QUALITY = 70

        /** Извлечь метаданные вложения. Ошибки не фатальны — карточка просто будет
         *  беднее (движку уйдут нули), поэтому всё в runCatching. */
        fun extract(ctx: Context, uri: Uri, mime: String, bytes: ByteArray): MediaMeta = when {
            mime.startsWith("image/") -> imageMeta(bytes)
            mime.startsWith("video/") -> videoMeta(ctx, uri)
            mime.startsWith("audio/") -> audioMeta(ctx, uri)
            else -> MediaMeta()
        }

        private fun imageMeta(bytes: ByteArray): MediaMeta = runCatching {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            // Размеры и превью — уже повёрнутые по EXIF: у получателя рамка снимка строится
            // по ним, а превью едет без EXIF.
            val o = JpegOrientation.read(bytes)
            // Превью делаем только для крупных: мелкая картинка и так качается сразу.
            val thumb = if (bytes.size > 512 * 1024) {
                val opts = BitmapFactory.Options().apply {
                    inSampleSize = sampleFor(bounds.outWidth, bounds.outHeight)
                }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)?.let { jpeg(chat.peerage.ui.Bitmaps.oriented(it, o)) }
            } else {
                null
            }
            val swap = JpegOrientation.swapsSides(o)
            MediaMeta(
                width = if (swap) bounds.outHeight else bounds.outWidth,
                height = if (swap) bounds.outWidth else bounds.outHeight,
                thumb = thumb,
            )
        }.getOrDefault(MediaMeta())

        private fun videoMeta(ctx: Context, uri: Uri): MediaMeta = withRetriever(ctx, uri) { r ->
            MediaMeta(
                durationMs = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toIntOrNull() ?: 0,
                width = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: 0,
                height = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: 0,
                thumb = r.frameAtTime?.let { jpeg(it) },
            )
        }

        private fun audioMeta(ctx: Context, uri: Uri): MediaMeta = withRetriever(ctx, uri) { r ->
            MediaMeta(
                durationMs = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toIntOrNull() ?: 0,
            )
        }

        private fun withRetriever(ctx: Context, uri: Uri, body: (MediaMetadataRetriever) -> MediaMeta): MediaMeta =
            runCatching {
                val r = MediaMetadataRetriever()
                try {
                    r.setDataSource(ctx, uri)
                    body(r)
                } finally {
                    runCatching { r.release() }
                }
            }.getOrDefault(MediaMeta())

        private fun sampleFor(w: Int, h: Int): Int {
            var s = 1
            while (w / s > THUMB_MAX * 2 || h / s > THUMB_MAX * 2) s *= 2
            return s
        }

        /** Ужать кадр до THUMB_MAX по большей стороне и упаковать в JPEG. */
        private fun jpeg(src: Bitmap): ByteArray? = runCatching {
            val scale = THUMB_MAX.toFloat() / maxOf(src.width, src.height).coerceAtLeast(1)
            val bmp = if (scale < 1f) {
                Bitmap.createScaledBitmap(src, (src.width * scale).toInt().coerceAtLeast(1), (src.height * scale).toInt().coerceAtLeast(1), true)
            } else {
                src
            }
            ByteArrayOutputStream().use { out ->
                bmp.compress(Bitmap.CompressFormat.JPEG, THUMB_QUALITY, out)
                out.toByteArray()
            }
        }.getOrNull()
    }
}
