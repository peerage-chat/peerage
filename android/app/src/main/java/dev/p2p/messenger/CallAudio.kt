package dev.p2p.messenger

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import kotlin.concurrent.thread

/**
 * Платформенный аудио ввод/вывод звонка. Микрофон (`AudioRecord`) → [Native.callPushAudio];
 * [Native.callPullAudio] → динамик (`AudioTrack`). Кодеки/сеть — в Rust-движке.
 *
 * Эхоподавление (AEC) — через `AcousticEchoCanceler` (ТЗ: AudioEffect API).
 */
class CallAudio(private val context: Context) {
    private var record: AudioRecord? = null
    private var track: AudioTrack? = null
    private var aec: AcousticEchoCanceler? = null
    @Volatile private var running = false
    private val threads = mutableListOf<Thread>()

    @SuppressLint("MissingPermission") // RECORD_AUDIO запрашивается до старта звонка
    fun start(sampleRate: Int, channels: Int) {
        val inMask = if (channels >= 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
        val outMask = if (channels >= 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val fmt = AudioFormat.ENCODING_PCM_16BIT

        val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        am.mode = AudioManager.MODE_IN_COMMUNICATION

        val inMin = AudioRecord.getMinBufferSize(sampleRate, inMask, fmt).coerceAtLeast(2048)
        val rec = AudioRecord(
            MediaRecorder.AudioSource.VOICE_COMMUNICATION,
            sampleRate, inMask, fmt, inMin * 2,
        )
        if (AcousticEchoCanceler.isAvailable()) {
            aec = AcousticEchoCanceler.create(rec.audioSessionId)?.apply { enabled = true }
        }

        val outMin = AudioTrack.getMinBufferSize(sampleRate, outMask, fmt).coerceAtLeast(2048)
        val trk = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build()
            )
            .setAudioFormat(
                AudioFormat.Builder().setSampleRate(sampleRate).setEncoding(fmt).setChannelMask(outMask).build()
            )
            .setBufferSizeInBytes(outMin * 2)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()

        record = rec
        track = trk
        running = true
        rec.startRecording()
        trk.play()

        // Захват микрофона → Rust.
        val chunk = sampleRate / 50 * (if (channels >= 2) 2 else 1) // ~20мс
        threads += thread(name = "call-capture") {
            val buf = ShortArray(chunk)
            while (running) {
                val n = rec.read(buf, 0, buf.size)
                if (n > 0) Native.callPushAudio(buf, n)
            }
        }
        // Воспроизведение: Rust → динамик.
        threads += thread(name = "call-playback") {
            val buf = ShortArray(chunk)
            while (running) {
                Native.callPullAudio(buf) // заполняет тишиной, если данных нет
                trk.write(buf, 0, buf.size)
            }
        }
    }

    fun setMuted(muted: Boolean) = Native.callSetMuted(muted)

    fun stop() {
        running = false
        threads.forEach { runCatching { it.join(500) } }
        threads.clear()
        runCatching { record?.stop() }; runCatching { record?.release() }
        runCatching { track?.stop() }; runCatching { track?.release() }
        runCatching { aec?.release() }
        record = null; track = null; aec = null
        runCatching {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            am.mode = AudioManager.MODE_NORMAL
        }
    }
}
