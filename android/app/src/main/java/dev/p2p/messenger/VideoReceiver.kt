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
        // Темп берём ИЗ ПОТОКА (заголовок кадра), а не из локальной настройки: иначе
        // при разном fps у устройств приёмник пейсит не на той скорости (был баг:
        // локально 60, поток 30 → видео 2× → слив буфера → фриз, и так по кругу).
        var streamFps = fps
        var frameUs = 1_000_000L / streamFps
        var frameNs = 1_000_000_000L / streamFps
        val info = MediaCodec.BufferInfo()

        // ── Jitter-buffer (M1, идея WebRTC) ───────────────────────────────────
        // Очередь СЖАТЫХ access-unit'ов в приложении (а не в кодеке: его выходные
        // буферы — ~8, туда глубокий буфер не положить). Кормим декодер «точно в
        // срок» (за feedLead до показа) → пайплайн кодека мелкий, а вся глубина —
        // в этой очереди. Глубина = задержка, адаптивная: растёт при underrun.
        val queue = ArrayDeque<ByteArray>()
        val maxQueue = 90                 // жёсткий потолок (≈3с), защита от флуда
        var feedLeadNs = 3 * frameNs      // насколько раньше показа кормить декодер (зависит от темпа)
        val minTarget = 3; val maxTarget = 30
        var targetFrames = 8              // старт ~266мс; растёт при underrun
        var playing = false               // прошли ли пребуфер
        // Якорь часов показа: кадр с pts=clockAnchorPtsUs показывается в clockAnchorNs;
        // остальные — линейно от него. Ребейзится при возобновлении после underrun,
        // чтобы НЕ догонять пачкой (для реалтайма гэп пропускаем, не ускоряемся).
        var clockAnchorNs = 0L
        var clockAnchorPtsUs = 0L
        var feedIdx = 0                   // МОНОТОННЫЙ счётчик поданных (pts=feedIdx*frameUs); не сбрасывать при underrun
        var lastRotation = -1
        var pulledCount = 0; var renderedCount = 0; var underrunCount = 0
        var restartStreak = 0
        var sinceUnderrun = 0             // кадров без underrun (для усадки задержки)

        while (running) {
            try {
                if (dec == null) {
                    android.util.Log.i("VideoReceiver", "Initializing MediaCodec H.264 decoder")
                    val fmt = MediaFormat.createVideoFormat("video/avc", width, height).apply {
                        if (android.os.Build.VERSION.SDK_INT >= 30) {
                            setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
                        }
                    }
                    dec = MediaCodec.createDecoderByType("video/avc").apply {
                        configure(fmt, surface, null, 0); start()
                    }
                    decoder = dec
                    playing = false; feedIdx = 0; queue.clear() // новый декодер — свежий pts-таймлайн
                    android.util.Log.i("VideoReceiver", "Decoder started successfully")
                }
                val currentDec = dec!!
                val nowNs = System.nanoTime()

                // 1. Слить из JNI в jitter-buffer. Заголовок 2 байта: [0]=ориентация
                //    (угол/90), [1]=fps отправителя (для пейсинга). Остаток — access-unit.
                while (queue.size < maxQueue) {
                    val raw = pullVideoPacket()
                    if (raw.size < 2) { if (raw.isEmpty()) break else continue }
                    val rot = ((raw[0].toInt() and 0xFF) % 4) * 90
                    if (rot != lastRotation) { lastRotation = rot; onRotation(rot) }
                    val sfps = raw[1].toInt() and 0xFF
                    if (sfps in 1..120 && sfps != streamFps) {
                        streamFps = sfps
                        frameUs = 1_000_000L / sfps
                        frameNs = 1_000_000_000L / sfps
                        feedLeadNs = 3 * frameNs
                        playing = false // ре-пребуфер на новом темпе (ребейз якоря)
                        android.util.Log.i("VideoReceiver", "stream fps=$sfps")
                    }
                    queue.addLast(raw.copyOfRange(2, raw.size))
                    pulledCount++
                }

                // 2. Пребуфер: не начинаем показ, пока не накопили target кадров.
                // Задержку даёт сама ПОДУШКА из target накопленных кадров (мы их
                // показываем по cadence, пока новые догоняют), а не доп. сдвиг базы.
                // База = сейчас + feedLead (кадр 0 кормим сейчас, показ когда декоднётся).
                if (!playing && queue.size >= targetFrames) {
                    // Возобновление: РЕБЕЙЗ якоря — следующий кадр покажем «сейчас»,
                    // без догоняющей пачки. feedIdx НЕ трогаем (pts монотонен).
                    playing = true
                    clockAnchorPtsUs = feedIdx.toLong() * frameUs
                    clockAnchorNs = nowNs + feedLeadNs
                }

                // 3. Подача «точно в срок»: кормим кадр feedIdx за feedLead до показа.
                if (playing) {
                    val ptsUs = feedIdx.toLong() * frameUs
                    val playoutNs = clockAnchorNs + (ptsUs - clockAnchorPtsUs) * 1_000L
                    if (nowNs >= playoutNs - feedLeadNs) {
                        val au = queue.removeFirstOrNull()
                        if (au == null) {
                            // Underrun: пора кормить, а буфер пуст → растим задержку.
                            playing = false
                            targetFrames = (targetFrames + 2).coerceAtMost(maxTarget)
                            underrunCount++; sinceUnderrun = 0
                            android.util.Log.w("VideoReceiver", "underrun → target=$targetFrames (n=$underrunCount)")
                        } else {
                            val inIdx = currentDec.dequeueInputBuffer(0)
                            if (inIdx < 0) {
                                queue.addFirst(au) // вход кодека занят — вернём, попробуем позже
                            } else {
                                val ib = currentDec.getInputBuffer(inIdx)
                                if (ib != null && ib.capacity() >= au.size) {
                                    ib.clear(); ib.put(au)
                                    currentDec.queueInputBuffer(inIdx, 0, au.size, ptsUs, 0)
                                } else {
                                    currentDec.queueInputBuffer(inIdx, 0, 0, ptsUs, 0)
                                }
                                feedIdx++
                            }
                        }
                    }
                }

                // 4. Показ output'ов с ЯВНЫМ пейсингом сном (эмулятор/часть устройств
                // НЕ чтят present-timestamp в releaseOutputBuffer → метку игнорируют и
                // кадры летят пачкой). Спим до момента показа, затем рендерим сразу.
                val outIdx = try { currentDec.dequeueOutputBuffer(info, 4_000) } catch (e: Exception) { -1 }
                if (outIdx >= 0) {
                    val presentNs = clockAnchorNs + (info.presentationTimeUs - clockAnchorPtsUs) * 1_000L
                    val waitNs = presentNs - System.nanoTime()
                    if (waitNs in 1..200_000_000L) { // ждём (кадр в будущем), кап 200мс
                        Thread.sleep(waitNs / 1_000_000L, (waitNs % 1_000_000L).toInt())
                    }
                    runCatching { currentDec.releaseOutputBuffer(outIdx, true) } // рендер немедленно
                    renderedCount++; restartStreak = 0
                    // Усадка задержки: долго без underrun и буфер с запасом → -1 кадр.
                    if (++sinceUnderrun >= 300 && targetFrames > minTarget && queue.size > targetFrames) {
                        targetFrames--; sinceUnderrun = 0
                    }
                    if (renderedCount % 30 == 0) {
                        android.util.Log.i("VideoReceiver", "Rendered 30 (buf=${queue.size} target=$targetFrames underruns=$underrunCount)")
                    }
                }
                // INFO_OUTPUT_FORMAT_CHANGED (-2) / INFO_OUTPUT_BUFFERS_CHANGED (-3) игнорируем

            } catch (e: Exception) {
                if (running) {
                    runCatching { dec?.stop() }; runCatching { dec?.release() }
                    dec = null; decoder = null
                    queue.clear(); playing = false; feedIdx = 0
                    // НЕ сдаёмся насмерть (иначе серое навсегда до toggle тайла): после
                    // порога длинный backoff (≤ раз/2с — createDecoder это новый OMX-узел,
                    // частая тёрка вешает кодек-сервис). restartStreak сбрасывается при
                    // первом успешном рендере; декодер восстановится на keyframe.
                    restartStreak++
                    val backoff = if (restartStreak >= 6) 2000L else 150L * restartStreak
                    android.util.Log.e("VideoReceiver", "Decoder error, restart $restartStreak, backoff ${backoff}ms", e)
                    Thread.sleep(backoff)
                }
            }
        }

        runCatching { dec?.stop() }; runCatching { dec?.release() }
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
