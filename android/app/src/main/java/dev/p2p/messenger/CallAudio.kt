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

class CallAudio(
    private val context: Context,
    private val push: (ShortArray, Int) -> Unit,
    private val pull: (ShortArray) -> Int,
    private val mute: (Boolean) -> Unit,
) {
    private var record: AudioRecord? = null
    private var track: AudioTrack? = null
    private var aec: AcousticEchoCanceler? = null
    private var ns: NoiseSuppressor? = null
    private var agc: AutomaticGainControl? = null

    @Volatile private var running = false
    private val threads = mutableListOf<Thread>()

    @SuppressLint("MissingPermission")
    fun start(sampleRate: Int, channels: Int) {
        val inMask = if (channels >= 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
        val outMask = if (channels >= 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val fmt = AudioFormat.ENCODING_PCM_16BIT

        val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        am.mode = AudioManager.MODE_IN_COMMUNICATION

        val inMin = AudioRecord.getMinBufferSize(sampleRate, inMask, fmt).coerceAtLeast(2048)
        val rec = AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, sampleRate, inMask, fmt, inMin * 2)
        val sid = rec.audioSessionId
        if (AcousticEchoCanceler.isAvailable()) aec = AcousticEchoCanceler.create(sid)?.apply { enabled = true }
        if (NoiseSuppressor.isAvailable()) ns = NoiseSuppressor.create(sid)?.apply { enabled = true }
        if (AutomaticGainControl.isAvailable()) agc = AutomaticGainControl.create(sid)?.apply { enabled = true }

        val outMin = AudioTrack.getMinBufferSize(sampleRate, outMask, fmt).coerceAtLeast(2048)
        val trk = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder().setSampleRate(sampleRate).setEncoding(fmt).setChannelMask(outMask).build(),
            )
            .setBufferSizeInBytes(outMin * 2) // меньше буфер = меньше задержка вывода
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()

        record = rec
        track = trk
        running = true
        rec.startRecording()
        trk.play()

        val chunk = sampleRate / 50 * (if (channels >= 2) 2 else 1) // 20ms frame

        threads += thread(name = "call-capture") {
            Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
            val buf = ShortArray(chunk)
            while (running) {
                val n = rec.read(buf, 0, buf.size) // blocking: ровный 20мс ритм захвата
                when {
                    n > 0 -> push(buf, n)
                    else -> {
                        if (!running) break
                        android.util.Log.e("CallAudio", "capture error $n")
                    }
                }
            }
        }

        // Темп воспроизведения задаёт САМО железо: блокирующий trk.write() возвращается
        // ровно тогда, когда AudioTrack освободил место под новый кадр (= частота ЦАП).
        // Без ручного nanoTime-сна: он добавлял третьи часы (дрейф к отправителю/железу)
        // и мешал дозаполнить буфер после голодания. Это подход WebRTC/Telegram —
        // потребление аудио управляется аудио-устройством, а не таймером.
        threads += thread(name = "call-playback") {
            Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
            val buf = ShortArray(chunk)
            var starvations = 0
            var lastLogMs = System.currentTimeMillis()
            while (running) {
                if (pull(buf) == 0) {
                    starvations++
                    buf.fill(0)
                }
                trk.write(buf, 0, buf.size) // blocking: железо задаёт ритм
                val nowMs = System.currentTimeMillis()
                if (nowMs - lastLogMs >= 5000) {
                    android.util.Log.i("CallAudio", "playback: $starvations starv/5s")
                    starvations = 0
                    lastLogMs = nowMs
                }
            }
        }
    }

    fun setMuted(muted: Boolean) = mute(muted)

    fun stop() {
        running = false
        runCatching { record?.stop() }
        threads.forEach { runCatching { it.join(300) } }
        threads.clear()
        runCatching { record?.release() }
        runCatching { track?.pause() }
        runCatching { track?.flush() }
        runCatching { track?.stop() }
        runCatching { track?.release() }
        runCatching { aec?.release() }
        runCatching { ns?.release() }
        runCatching { agc?.release() }
        record = null
        track = null
        aec = null
        ns = null
        agc = null
        runCatching {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            if (am.mode != AudioManager.MODE_NORMAL) am.mode = AudioManager.MODE_NORMAL
        }
    }
}
