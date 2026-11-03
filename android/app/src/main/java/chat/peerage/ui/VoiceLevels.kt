package chat.peerage.ui

import android.media.MediaCodec
import android.media.MediaDataSource
import android.media.MediaExtractor
import android.media.MediaFormat
import android.util.LruCache
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.produceState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.nio.ByteOrder
import kotlin.math.abs

/**
 * Волна голосового — громкость по столбикам, снятая с самого звука.
 *
 * Сообщение волну не несёт, поэтому она считается здесь: AAC разворачивается в PCM, и
 * каждый столбик — пик своего отрезка. Голосовые короткие (секунды), декодер системный;
 * результат кэшируется по блобу — второй показ того же сообщения ничего не декодирует.
 */
private val levelsCache = LruCache<String, VoiceWave>(128)

/** Волна и длительность звука (мс) — длительность из самого файла. */
internal class VoiceWave(val levels: FloatArray, val durationMs: Int)

/** Нижний столбик: тишина видна чертой, а не пропуском. */
private const val LEVEL_FLOOR = 0.15f

/** Какой по громкости столбик считать полным (доля от тихого к громкому). */
private const val TOP_PERCENTILE = 0.9f

/** Предохранитель цикла декодера: битый файл не должен крутить его вечно. */
private const val DECODE_STEPS_MAX = 200_000

/** Волна для [bytes] на [bars] столбиков (0..1); null — пока считается или звук не разобрался. */
@Composable
internal fun rememberVoiceLevels(blobId: String, bytes: ByteArray?, bars: Int): State<VoiceWave?> =
    produceState(levelsCache.get("$blobId/$bars"), blobId, bytes != null, bars) {
        if (value != null || bytes == null) return@produceState
        value = withContext(Dispatchers.IO) {
            decodeLevels(bytes, bars)?.also { levelsCache.put("$blobId/$bars", it) }
        }
    }

/** Пики PCM по отрезкам, сведённые к [bars] столбикам и к самому громкому. */
internal fun decodeLevels(bytes: ByteArray, bars: Int): VoiceWave? = runCatching {
    val (peaks, durationMs) = decodePeaks(bytes) ?: return null
    if (peaks.isEmpty() || bars <= 0) return null
    val out = FloatArray(bars) { i ->
        val from = i * peaks.size / bars
        val to = maxOf(from + 1, (i + 1) * peaks.size / bars)
        (from until minOf(to, peaks.size)).maxOfOrNull { peaks[it] } ?: 0f
    }
    // Норма — не самый громкий столбик, а почти самый: щелчок в начале записи иначе
    // сплющивал всю остальную волну в черту.
    val top = out.sorted()[(bars * TOP_PERCENTILE).toInt().coerceAtMost(bars - 1)].takeIf { it > 0f } ?: 1f
    VoiceWave(FloatArray(bars) { (out[it] / top).coerceIn(LEVEL_FLOOR, 1f) }, durationMs)
}.getOrNull()

/** Пик каждого выходного буфера декодера (16-бит PCM) и длительность дорожки, мс. */
private fun decodePeaks(bytes: ByteArray): Pair<List<Float>, Int>? {
    val ex = MediaExtractor()
    ex.setDataSource(BytesSource(bytes))
    val track = (0 until ex.trackCount).firstOrNull {
        ex.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true
    }
    if (track == null) {
        ex.release()
        return null
    }
    ex.selectTrack(track)
    val format = ex.getTrackFormat(track)
    val durationMs = if (format.containsKey(MediaFormat.KEY_DURATION)) (format.getLong(MediaFormat.KEY_DURATION) / 1000).toInt() else 0
    val codec = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME)!!)
    val peaks = ArrayList<Float>()
    try {
        codec.configure(format, null, null, 0)
        codec.start()
        val info = MediaCodec.BufferInfo()
        var inputDone = false
        var steps = 0
        while (steps++ < DECODE_STEPS_MAX) {
            if (!inputDone) {
                val i = codec.dequeueInputBuffer(10_000)
                if (i >= 0) {
                    val n = ex.readSampleData(codec.getInputBuffer(i)!!, 0)
                    if (n < 0) {
                        codec.queueInputBuffer(i, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                        inputDone = true
                    } else {
                        codec.queueInputBuffer(i, 0, n, ex.sampleTime, 0)
                        ex.advance()
                    }
                }
            }
            val o = codec.dequeueOutputBuffer(info, 10_000)
            if (o >= 0) {
                val pcm = codec.getOutputBuffer(o)!!.order(ByteOrder.nativeOrder()).asShortBuffer()
                var peak = 0
                while (pcm.hasRemaining()) peak = maxOf(peak, abs(pcm.get().toInt()))
                if (info.size > 0) peaks.add(peak / 32768f)
                codec.releaseOutputBuffer(o, false)
                if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
            }
        }
    } finally {
        runCatching { codec.stop() }
        codec.release()
        ex.release()
    }
    return peaks to durationMs
}

/** Звук из памяти: голосовое уже лежит в кэше блобов, писать его в файл ради разбора незачем. */
private class BytesSource(private val bytes: ByteArray) : MediaDataSource() {
    override fun readAt(position: Long, buffer: ByteArray, offset: Int, size: Int): Int {
        if (position >= bytes.size) return -1
        val n = minOf(size.toLong(), bytes.size - position).toInt()
        System.arraycopy(bytes, position.toInt(), buffer, offset, n)
        return n
    }

    override fun getSize(): Long = bytes.size.toLong()

    override fun close() {}
}
