package chat.peerage

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTimestamp
import android.media.AudioTrack
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import android.media.audiofx.AutomaticGainControl
import android.media.audiofx.NoiseSuppressor
import android.os.Process
import kotlin.concurrent.thread

/** Тумблеры аппаратной обработки микрофона (экран настроек звука). */
data class AudioFx(val ns: Boolean = true, val aec: Boolean = true, val agc: Boolean = true)

class CallAudio(
    private val context: Context,
    /** (PCM, сэмплов, сколько назад записан последний, мкс; 0 — неизвестно). */
    private val push: (ShortArray, Int, Int) -> Unit,
    /** (куда, через сколько записанное сюда будет слышно, мкс) → сэмплов. */
    private val pull: (ShortArray, Int) -> Int,
    private val mute: (Boolean) -> Unit,
    fx: AudioFx = AudioFx(),
    inputDevice: AudioDeviceInfo? = null,
    private val onLevel: (Float) -> Unit = {},
) {
    /** Микрофон и его аппаратная обработка — живут и умирают вместе. */
    private class Mic(
        val record: AudioRecord,
        val aec: AcousticEchoCanceler?,
        val ns: NoiseSuppressor?,
        val agc: AutomaticGainControl?,
    ) {
        fun setFx(fx: AudioFx) {
            runCatching { aec?.enabled = fx.aec }
            runCatching { ns?.enabled = fx.ns }
            runCatching { agc?.enabled = fx.agc }
        }

        fun release() {
            runCatching { record.stop() }
            runCatching { record.release() }
            runCatching { aec?.release() }
            runCatching { ns?.release() }
            runCatching { agc?.release() }
        }
    }

    /** Меняют поток захвата (переоткрытие умершего) и [stop] — под [micLock]. */
    @Volatile private var mic: Mic? = null
    private val micLock = Any()
    private var track: AudioTrack? = null

    /** Тумблеры и микрофон сейчас — с ними же откроется переоткрытый. */
    @Volatile private var fx = fx

    @Volatile private var inputDevice = inputDevice

    @Volatile private var running = false
    private val threads = mutableListOf<Thread>()

    /** Для лога: возраст последнего захваченного куска, мкс. */
    @Volatile private var lastCaptureAgeUs = 0

    @SuppressLint("MissingPermission")
    fun start(sampleRate: Int, channels: Int) {
        val inMask = if (channels >= 2) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
        val outMask = if (channels >= 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val fmt = AudioFormat.ENCODING_PCM_16BIT

        val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        am.mode = AudioManager.MODE_IN_COMMUNICATION

        val firstMic = openMic(sampleRate, inMask, fmt)

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

        mic = firstMic
        track = trk
        running = true
        trk.play()

        val ch = if (channels >= 2) 2 else 1
        val chunk = sampleRate / 50 * ch // 20ms frame

        threads += thread(name = "call-capture") {
            Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO)
            val buf = ShortArray(chunk)
            var levelTick = 0
            val stamp = AudioTimestamp()
            var framesRead = 0L
            val recovery = MicRecovery()
            while (running) {
                val rec = mic?.record
                // blocking: ровный 20мс ритм захвата. Микрофона нет (переоткрыть не
                // вышло) — как умерший: следующая попытка позже.
                val n = rec?.read(buf, 0, buf.size) ?: AudioRecord.ERROR_DEAD_OBJECT
                when (val step = recovery.after(n)) {
                    MicRecovery.Step.Go -> {
                        rec ?: continue
                        framesRead += n / ch
                        val ageUs = captureAgeUs(rec, stamp, framesRead, sampleRate)
                        lastCaptureAgeUs = ageUs
                        push(buf, n, ageUs)
                        // Уровень мика (RMS) для индикатора — раз в ~3 кадра (60мс), не каждый.
                        if (++levelTick >= 3) {
                            levelTick = 0
                            var sum = 0.0
                            for (i in 0 until n) {
                                val s = buf[i].toDouble()
                                sum += s * s
                            }
                            val rms = Math.sqrt(sum / n)
                            onLevel((rms / 32768.0 * 4.0).toFloat().coerceIn(0f, 1f))
                        }
                    }
                    is MicRecovery.Step.Wait -> {
                        if (!running) break
                        android.util.Log.e("CallAudio", "capture error $n")
                        Thread.sleep(step.ms)
                    }
                    is MicRecovery.Step.Reopen -> {
                        if (!running) break
                        android.util.Log.w("CallAudio", "mic dead ($n), reopen in ${step.afterMs}ms")
                        synchronized(micLock) {
                            mic?.release()
                            mic = null
                        }
                        Thread.sleep(step.afterMs)
                        synchronized(micLock) {
                            if (running) {
                                mic = runCatching { openMic(sampleRate, inMask, fmt) }
                                    .onFailure { android.util.Log.w("CallAudio", "mic reopen failed: $it") }
                                    .getOrNull()
                            }
                        }
                        framesRead = 0
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
            val stamp = AudioTimestamp()
            var framesWritten = 0L
            // Пока у AudioTrack нет метки — считаем, что буфер полон (write блокирует).
            val fullBufferUs = (trk.bufferSizeInFrames * 1_000_000L / sampleRate).toInt()
            while (running) {
                val delayUs = outputDelayUs(trk, stamp, framesWritten, sampleRate) ?: fullBufferUs
                if (pull(buf, delayUs) == 0) {
                    starvations++
                    buf.fill(0)
                }
                trk.write(buf, 0, buf.size) // blocking: железо задаёт ритм
                framesWritten += buf.size / ch
                val nowMs = System.currentTimeMillis()
                if (nowMs - lastLogMs >= 5000) {
                    android.util.Log.i(
                        "CallAudio",
                        "playback: $starvations starv/5s, output ${delayUs / 1000}ms, capture ${lastCaptureAgeUs / 1000}ms",
                    )
                    starvations = 0
                    lastLogMs = nowMs
                }
            }
        }
    }

    /** Микрофон с выбранным устройством и тумблерами, уже пишущий. Не вышло —
     *  исключение, и ничего не удержано. */
    @SuppressLint("MissingPermission")
    private fun openMic(sampleRate: Int, inMask: Int, fmt: Int): Mic {
        val inMin = AudioRecord.getMinBufferSize(sampleRate, inMask, fmt).coerceAtLeast(2048)
        val rec = AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, sampleRate, inMask, fmt, inMin * 2)
        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            rec.release()
            error("AudioRecord not initialized")
        }
        inputDevice?.let { runCatching { rec.preferredDevice = it } } // выбранный микрофон
        val sid = rec.audioSessionId
        val m = Mic(
            rec,
            if (AcousticEchoCanceler.isAvailable()) AcousticEchoCanceler.create(sid) else null,
            if (NoiseSuppressor.isAvailable()) NoiseSuppressor.create(sid) else null,
            if (AutomaticGainControl.isAvailable()) AutomaticGainControl.create(sid) else null,
        )
        m.setFx(fx)
        try {
            rec.startRecording()
        } catch (e: Exception) {
            m.release()
            throw e
        }
        return m
    }

    /** Сколько назад записан последний прочитанный сэмпл, мкс (0 — неизвестно): по
     *  метке AudioRecord «кадр № такой-то записан тогда-то». */
    private fun captureAgeUs(rec: AudioRecord, stamp: AudioTimestamp, framesRead: Long, rate: Int): Int {
        if (rec.getTimestamp(stamp, AudioTimestamp.TIMEBASE_MONOTONIC) != AudioRecord.SUCCESS) return 0
        val lastNs = stamp.nanoTime + (framesRead - 1 - stamp.framePosition) * 1_000_000_000L / rate
        val ageUs = (System.nanoTime() - lastNs) / 1000
        return if (ageUs in 0..MAX_DELAY_US) ageUs.toInt() else 0
    }

    /** Через сколько будет слышен следующий записанный в [trk] сэмпл, мкс: всё, что
     *  записано и ещё не проиграно (по метке AudioTrack). `null` — метки ещё нет. */
    private fun outputDelayUs(trk: AudioTrack, stamp: AudioTimestamp, framesWritten: Long, rate: Int): Int? {
        if (!trk.getTimestamp(stamp)) return null
        val playedNow = stamp.framePosition + (System.nanoTime() - stamp.nanoTime) * rate / 1_000_000_000L
        val us = (framesWritten - playedNow) * 1_000_000L / rate
        return if (us in 0..MAX_DELAY_US) us.toInt() else null
    }

    fun setMuted(muted: Boolean) = mute(muted)

    /** Применить тумблеры обработки на лету (на существующих эффектах). */
    fun applyFx(newFx: AudioFx) {
        fx = newFx
        mic?.setFx(newFx)
    }

    /** Сменить входное устройство на лету (на работающем AudioRecord). */
    fun setInputDevice(dev: AudioDeviceInfo?) {
        inputDevice = dev
        runCatching { mic?.record?.preferredDevice = dev }
    }

    fun stop() {
        running = false
        runCatching { mic?.record?.stop() } // read() вернётся
        threads.forEach { runCatching { it.join(300) } }
        threads.clear()
        synchronized(micLock) {
            mic?.release()
            mic = null
        }
        runCatching { track?.pause() }
        runCatching { track?.flush() }
        runCatching { track?.stop() }
        runCatching { track?.release() }
        track = null
        runCatching {
            val am = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
            if (am.mode != AudioManager.MODE_NORMAL) am.mode = AudioManager.MODE_NORMAL
        }
    }

    private companion object {
        /** Задержка захвата или вывода больше этой — сбой замера, не верим. */
        const val MAX_DELAY_US = 1_000_000L
    }
}
