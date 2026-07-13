package dev.p2p.messenger

import android.content.Context
import android.media.MediaRecorder
import android.os.Build
import java.io.File

/**
 * Запись голосового сообщения (M2): MediaRecorder → MPEG-4/AAC во временный файл,
 * на [stop] отдаёт байты. Байты уходят в [AppModel.sendMedia] (mime audio/mp4) — тем
 * же блоб-путём, что картинки/файлы. Held-to-record: [start] на нажатии, [stop] на
 * отпускании кнопки микрофона.
 */
class VoiceRecorder(private val ctx: Context) {
    private var rec: MediaRecorder? = null
    private var file: File? = null

    /** Начать запись. false — не удалось (нет разрешения/занят кодек). */
    fun start(): Boolean {
        val f = File(ctx.cacheDir, "voice_${System.currentTimeMillis()}.m4a")

        @Suppress("DEPRECATION")
        val r = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) MediaRecorder(ctx) else MediaRecorder()
        return runCatching {
            r.setAudioSource(MediaRecorder.AudioSource.MIC)
            r.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
            r.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
            r.setAudioEncodingBitRate(64_000)
            r.setAudioSamplingRate(44_100)
            r.setOutputFile(f.absolutePath)
            r.prepare()
            r.start()
            rec = r
            file = f
            true
        }.getOrElse {
            runCatching { r.release() }
            f.delete()
            false
        }
    }

    /** Остановить и вернуть байты записи (null — ошибка/слишком коротко). */
    fun stop(): ByteArray? {
        val r = rec ?: return null
        val f = file
        rec = null
        file = null
        val bytes = runCatching {
            r.stop()
            f?.readBytes()
        }.getOrNull()
        runCatching { r.release() }
        f?.delete()
        return bytes?.takeIf { it.isNotEmpty() }
    }
}
