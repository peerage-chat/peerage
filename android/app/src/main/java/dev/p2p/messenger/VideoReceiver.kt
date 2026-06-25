package dev.p2p.messenger

import android.media.MediaCodec
import android.media.MediaFormat
import android.view.Surface
import kotlin.concurrent.thread

/**
 * Видео-приёмник: тянет собранные H.264 access-unit'ы из Rust
 * ([Native.callPullVideoPacket]), скармливает аппаратному декодеру (MediaCodec)
 * и рендерит прямо в [surface] (zero-copy на экран). SPS/PPS приходят in-band
 * (добавлены к keyframe отправителем) — декодер инициализируется из потока.
 */
class VideoReceiver(
    private val surface: Surface,
    private val width: Int,
    private val height: Int,
    private val fps: Int = 30,
    /** Вызывается при изменении ориентации потока (угол сенсора отправителя, 0/90/
     *  180/270) — UI доворачивает тайл. Идёт 1-м байтом каждого access-unit'а. */
    private val onRotation: (Int) -> Unit = {},
    private val pullVideoPacket: () -> ByteArray
) {
    private var decoder: MediaCodec? = null
    private var loopThread: Thread? = null
    @Volatile private var running = false

    fun start() {
        android.util.Log.i("VideoReceiver", "start() called: width=$width, height=$height")
        running = true
        loopThread = thread(name = "vdec") { loop() }
    }

    private fun loop() {
        android.util.Log.i("VideoReceiver", "loop() started")
        var dec: MediaCodec? = null
        var pts = 0L                  // microseconds, для queueInputBuffer
        val frameUs = 1_000_000L / fps
        val frameNs = 1_000_000_000L / fps
        val info = MediaCodec.BufferInfo()
        var pulledCount = 0
        var renderedCount = 0
        var pendingPacket: ByteArray? = null
        var renderBaseNs = 0L         // wall-clock момент первого output-кадра
        var firstPtsUs = 0L           // PTS первого output-кадра
        var restartStreak = 0         // подряд идущих рестартов декодера без успешного рендера
        var lastRotation = -1         // последний сообщённый угол (чтобы не дёргать UI зря)

        while (running) {
            try {
                if (dec == null) {
                    android.util.Log.i("VideoReceiver", "Initializing MediaCodec H.264 decoder")
                    val fmt = MediaFormat.createVideoFormat("video/avc", width, height).apply {
                        // Low-latency декодирование (API 30+): минимум внутренней
                        // буферизации кодека — критично для реалтайм-звонка.
                        if (android.os.Build.VERSION.SDK_INT >= 30) {
                            setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
                        }
                    }
                    dec = MediaCodec.createDecoderByType("video/avc").apply {
                        configure(fmt, surface, null, 0)
                        start()
                    }
                    decoder = dec
                    renderBaseNs = 0L  // сброс clock при перезапуске декодера
                    android.util.Log.i("VideoReceiver", "Decoder started successfully")
                }

                val currentDec = dec!!

                // ── Подать входной пакет ──────────────────────────────────────
                // Снимаем 1-байтовый заголовок ориентации, остаток (сам access-unit)
                // отдаём декодеру. pendingPacket уже без заголовка.
                val pkt = pendingPacket ?: run {
                    val raw = pullVideoPacket()
                    if (raw.isEmpty()) raw else {
                        val rot = ((raw[0].toInt() and 0xFF) % 4) * 90
                        if (rot != lastRotation) { lastRotation = rot; onRotation(rot) }
                        raw.copyOfRange(1, raw.size)
                    }
                }
                if (pkt.isNotEmpty()) {
                    val inIdx = currentDec.dequeueInputBuffer(5_000)
                    if (inIdx >= 0) {
                        pendingPacket = null
                        pulledCount++
                        if (pulledCount % 30 == 0) {
                            android.util.Log.i("VideoReceiver", "Pulled 30 frames from JNI, last size = ${pkt.size} bytes")
                        }
                        val ib = currentDec.getInputBuffer(inIdx)
                        if (ib != null && ib.capacity() >= pkt.size) {
                            ib.clear(); ib.put(pkt)
                            currentDec.queueInputBuffer(inIdx, 0, pkt.size, pts, 0)
                            pts += frameUs
                        } else {
                            if (ib == null) {
                                android.util.Log.w("VideoReceiver", "getInputBuffer($inIdx) returned null")
                            } else {
                                android.util.Log.w("VideoReceiver", "Buffer capacity ${ib.capacity()} < packet size ${pkt.size}")
                            }
                            currentDec.queueInputBuffer(inIdx, 0, 0, pts, 0)
                        }
                    } else {
                        pendingPacket = pkt
                    }
                }

                // ── Получить один output-кадр (блокируем до ~8мс) ─────────────
                // Ранее: while (dequeue(0) >= 0) render(true) — рендерил пачками
                // без пейсинга. Теперь: один кадр за итерацию + timestamp-рендер.
                val outIdx = try { currentDec.dequeueOutputBuffer(info, 8_000) } catch (e: Exception) { -1 }
                if (outIdx >= 0) {
                    val nowNs = System.nanoTime()
                    if (renderBaseNs == 0L) {
                        renderBaseNs = nowNs + frameNs
                        firstPtsUs = info.presentationTimeUs
                    }
                    val renderNs = renderBaseNs + (info.presentationTimeUs - firstPtsUs) * 1_000L
                    val renderAt = if (renderNs < nowNs - 3 * frameNs) {
                        // Отстаём на 3+ кадра (после стала/заморозки) — ресинк clock,
                        // иначе burst-рендер ускоряет видео вместо плавного восстановления.
                        renderBaseNs = nowNs + frameNs
                        firstPtsUs = info.presentationTimeUs
                        renderBaseNs
                    } else {
                        renderNs.coerceAtLeast(nowNs)
                    }
                    runCatching { currentDec.releaseOutputBuffer(outIdx, renderAt) }
                    renderedCount++
                    restartStreak = 0 // успешный рендер — цепочка рестартов сброшена
                    if (renderedCount % 30 == 0) {
                        android.util.Log.i("VideoReceiver", "Rendered 30 frames to surface")
                    }
                }
                // INFO_OUTPUT_FORMAT_CHANGED (-2) и INFO_OUTPUT_BUFFERS_CHANGED (-3) игнорируем

            } catch (e: Exception) {
                if (running) {
                    runCatching { dec?.stop() }
                    runCatching { dec?.release() }
                    dec = null
                    decoder = null
                    pendingPacket = null
                    renderBaseNs = 0L
                    // НЕ сдаёмся насмерть: раньше после 6 ошибок ставили running=false
                    // и выходили — а тайл-SurfaceTexture жив, onSurfaceTextureAvailable
                    // повторно не стреляет → приёмник не пересоздаётся → картинка СЕРАЯ
                    // навсегда, пока не пересоздадут тайл (toggle камеры). Теперь после
                    // порога держим ДЛИННЫЙ backoff (≤ раза в 2с — каждый createDecoder
                    // это новый OMX-узел, частая тёрка вешает кодек-сервис; 2с безопасны).
                    // Декодер пересоздастся на след. итерации и восстановится, когда поток
                    // снова здоров и придёт keyframe. restartStreak сбрасывается при первом
                    // успешном рендере.
                    restartStreak++
                    val backoff = if (restartStreak >= 6) 2000L else 150L * restartStreak
                    android.util.Log.e("VideoReceiver", "Decoder error, restart $restartStreak, backoff ${backoff}ms", e)
                    Thread.sleep(backoff)
                }
            }
        }

        runCatching { dec?.stop() }
        runCatching { dec?.release() }
        decoder = null
    }

    fun stop() {
        running = false
        val t = loopThread; loopThread = null
        runCatching { t?.join(500) }
        val dec = decoder; decoder = null
        runCatching { dec?.stop() }
        runCatching { dec?.release() }
    }
}
