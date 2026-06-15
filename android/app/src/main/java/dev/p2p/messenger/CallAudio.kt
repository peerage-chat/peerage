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
import android.media.audiofx.AutomaticGainControl
import android.media.audiofx.NoiseSuppressor
import android.os.Process
import kotlin.concurrent.thread

/**
 * Платформенный аудио ввод/вывод звонка. Микрофон (`AudioRecord`) → [Native.callPushAudio];
 * [Native.callPullAudio] → динамик (`AudioTrack`). Кодеки/сеть — в Rust-движке.
 *
 * Чистота речи: эхоподавление (AEC) + шумоподавление (NS) + автоусиление (AGC),
 * все через AudioEffect API на сессии записи. Потоки захвата/воспроизведения —
 * с приоритетом URGENT_AUDIO, чтобы видео-нагрузка не вызывала underrun/шум.
 */
class CallAudio(
    private val context: Context,
    // Мост к ядру — по умолчанию 1:1-звонок; для комнаты передаём room-варианты.
    private val push: (ShortArray, Int) -> Unit = Native::callPushAudio,
    private val pull: (ShortArray) -> Int = Native::callPullAudio,
    private val mute: (Boolean) -> Unit = Native::callSetMuted,
) {
    private var record: AudioRecord? = null
    private var track: AudioTrack? = null
    private var aec: AcousticEchoCanceler? = null
    private var ns: NoiseSuppressor? = null
    private var agc: AutomaticGainControl? = null
    @Volatile private var running = false
    private val threads = mutableListOf<Thread>()

    @SuppressLint("MissingPermission") // RECORD_AUDIO запрашивается до старта звонка
    fun start(sampleRate: Int, channels: Int) {
        val inMask = if (channels >= 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
        val outMask = if (channels >= 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val fmt = AudioFormat.ENCODING_PCM_16BIT

        val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        am.mode = AudioManager.MODE_IN_COMMUNICATION

        // Буферы с запасом (×4) — меньше риск underrun под нагрузкой видео.
        val inMin = AudioRecord.getMinBufferSize(sampleRate, inMask, fmt).coerceAtLeast(2048)
        val rec = AudioRecord(
            MediaRecorder.AudioSource.VOICE_COMMUNICATION,
            sampleRate, inMask, fmt, inMin * 4,
        )
        val sid = rec.audioSessionId
        if (AcousticEchoCanceler.isAvailable()) {
            aec = AcousticEchoCanceler.create(sid)?.apply { enabled = true }
        }
        if (NoiseSuppressor.isAvailable()) {
            ns = NoiseSuppressor.create(sid)?.apply { enabled = true }
        }
        if (AutomaticGainControl.isAvailable()) {
            agc = AutomaticGainControl.create(sid)?.apply { enabled = true }
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
            .setBufferSizeInBytes(outMin * 4)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()

        record = rec
        track = trk
        running = true
        rec.startRecording()
        trk.play()

        // Захват микрофона → Rust. Приоритет URGENT_AUDIO — чтобы видео не голодало.
        val chunk = sampleRate / 50 * (if (channels >= 2) 2 else 1) // ~20мс
        threads += thread(name = "call-capture") {
            Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
            val buf = ShortArray(chunk)
            while (running) {
                val n = rec.read(buf, 0, buf.size)
                if (n > 0) push(buf, n)
            }
        }
        // Воспроизведение: Rust → динамик.
        threads += thread(name = "call-playback") {
            Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
            val buf = ShortArray(chunk)
            while (running) {
                pull(buf) // заполняет тишиной, если данных нет
                trk.write(buf, 0, buf.size)
            }
        }
    }

    fun setMuted(muted: Boolean) = mute(muted)

    fun stop() {
        running = false
        threads.forEach { runCatching { it.join(500) } }
        threads.clear()
        runCatching { record?.stop() }; runCatching { record?.release() }
        runCatching { track?.stop() }; runCatching { track?.release() }
        runCatching { aec?.release() }; runCatching { ns?.release() }; runCatching { agc?.release() }
        record = null; track = null; aec = null; ns = null; agc = null
        runCatching {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            am.mode = AudioManager.MODE_NORMAL
        }
    }
}
