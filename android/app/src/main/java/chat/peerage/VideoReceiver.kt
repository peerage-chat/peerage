package chat.peerage

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.view.Surface
import kotlin.concurrent.thread

/**
 * Видео-приёмник: тянет собранные H.264 access-unit'ы из Rust
 * ([Native.roomPullVideoPacket]), скармливает аппаратному декодеру (MediaCodec)
 * и рендерит прямо в [surface] (zero-copy на экран). SPS/PPS приходят in-band
 * (добавлены к keyframe отправителем) — декодер инициализируется из потока.
 *
 * Сроки кадров расписывает LiteRTC (ADR-014, шаг 8): по метке времени захвата, в
 * темпе камеры отправителя, с запасом на разброс сети. Кадр забирается в свой срок и
 * сразу идёт в декодер, готовый — сразу на экран: темп задаёт подача, а задержка
 * декодера у устройства постоянна и лишь сдвигает всё целиком (так и в WebRTC).
 * Держать готовые кадры до «срока плюс задержка» нельзя: выходных буферов у
 * декодера несколько, он встаёт, подача отстаёт, задержка растёт по кругу.
 *
 * Если декодер не успевает за потоком, кадры копятся в очереди LiteRTC; там она и
 * ограничена (перескок к ключевому кадру).
 */
class VideoReceiver(
    private val surface: Surface,
    private val width: Int,
    private val height: Int,
    /** Вызывается при изменении ориентации потока (угол сенсора отправителя, 0/90/
     *  180/270) — UI доворачивает тайл. Идёт 1-м байтом каждого access-unit'а. */
    private val onRotation: (Int) -> Unit = {},
    /** Новому декодеру нужен ключевой кадр: попросить его у отправителя. Зовётся из
     *  потока декодера (не из Compose), при каждом создании декодера. */
    private val requestKeyframe: () -> Unit = {},
    private val pullVideoPacket: () -> ByteArray,
) {
    private var decoder: MediaCodec? = null
    private var loopThread: Thread? = null

    @Volatile private var running = false

    /** Задержка своего декодера: срок кадра → кадр на экране (сглаженная). На столько
     *  видео отстаёт от сроков LiteRTC — для синхронизации со звуком. */
    @Volatile var decoderDelayNs = 0L
        private set

    fun start() {
        android.util.Log.i("VideoReceiver", "start() called: width=$width, height=$height")
        running = true
        loopThread = thread(name = "vdec") { loop() }
    }

    private fun loop() {
        android.util.Log.i("VideoReceiver", "loop() started")
        var dec: MediaCodec? = null
        val info = MediaCodec.BufferInfo()
        // Кадр уже взят из LiteRTC, но входные буферы декодера заняты — подаём позже.
        var pending: ByteArray? = null
        var pendingDueUs = 0L
        var lastRotation = -1
        var renderedCount = 0
        var restartStreak = 0
        // Для лога: насколько позже срока кадр подан и показан (худшее за 30 кадров).
        var worstFeedLateUs = 0L
        var worstShownLateUs = 0L
        var fedCount = 0L
        var outCount = 0L

        while (running) {
            try {
                if (dec == null) {
                    android.util.Log.i("VideoReceiver", "Initializing MediaCodec H.264 decoder")
                    val codec = MediaCodec.createDecoderByType("video/avc")
                    val fmt = MediaFormat.createVideoFormat("video/avc", width, height).apply {
                        if (android.os.Build.VERSION.SDK_INT >= 30) {
                            setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
                        }
                        // Отправитель меняет разрешение по полосе (ступени, ADR-014 §7b):
                        // с запасом под 1080p декодер переходит на новый размер по SPS
                        // ключевого кадра, без пересоздания.
                        adaptiveMax(codec)?.let { (mw, mh) ->
                            setInteger(MediaFormat.KEY_MAX_WIDTH, mw)
                            setInteger(MediaFormat.KEY_MAX_HEIGHT, mh)
                        }
                    }
                    dec = codec.apply {
                        configure(fmt, surface, null, 0)
                        start()
                    }
                    decoder = dec
                    pending = null
                    // Опоры у нового декодера нет; ключевой по расписанию — раз в 10 с.
                    requestKeyframe()
                    android.util.Log.i(
                        "VideoReceiver",
                        "Decoder started: ${codec.name}, lowLatency=" +
                            runCatching {
                                codec.codecInfo.getCapabilitiesForType("video/avc")
                                    .isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency)
                            }.getOrNull(),
                    )
                }
                val currentDec = dec!!

                // 1. Подать декодеру всё, чей срок пришёл. Раскладка JNI (не протокол):
                //    [0] поворот (угол/90), [1..5) срок кадра от «сейчас», мкс (i32 BE,
                //    ≤ 0), дальше access-unit. Срок едет в pts.
                while (true) {
                    if (pending == null) {
                        val raw = pullVideoPacket()
                        if (raw.size < PULL_HEADER) break
                        val rot = ((raw[0].toInt() and 0xFF) % 4) * 90
                        if (rot != lastRotation) {
                            lastRotation = rot
                            onRotation(rot)
                        }
                        val dueInUs = java.nio.ByteBuffer.wrap(raw, 1, 4).int
                        pendingDueUs = System.nanoTime() / 1_000L + dueInUs
                        pending = raw.copyOfRange(PULL_HEADER, raw.size)
                    }
                    val au = pending ?: break
                    val inIdx = currentDec.dequeueInputBuffer(0)
                    if (inIdx < 0) break // вход занят — этот кадр подождёт
                    val ib = currentDec.getInputBuffer(inIdx)
                    if (ib != null && ib.capacity() >= au.size) {
                        ib.clear()
                        ib.put(au)
                        currentDec.queueInputBuffer(inIdx, 0, au.size, pendingDueUs, 0)
                        fedCount++
                    } else {
                        currentDec.queueInputBuffer(inIdx, 0, 0, pendingDueUs, 0)
                    }
                    worstFeedLateUs = maxOf(worstFeedLateUs, System.nanoTime() / 1_000L - pendingDueUs)
                    pending = null
                }

                // 2. Готовый кадр — сразу на экран. Ждём выхода не дольше 4 мс: пора
                //    снова подавать.
                val outIdx = try {
                    currentDec.dequeueOutputBuffer(info, POLL_US)
                } catch (e: Exception) {
                    -1
                }
                if (outIdx >= 0) {
                    val lateNs = System.nanoTime() - info.presentationTimeUs * 1_000L
                    outCount++
                    runCatching { currentDec.releaseOutputBuffer(outIdx, true) }
                    decoderDelayNs += ((lateNs - decoderDelayNs) * DELAY_SMOOTHING).toLong()
                    worstShownLateUs = maxOf(worstShownLateUs, lateNs / 1_000L)
                    renderedCount++
                    restartStreak = 0
                    if (renderedCount % 30 == 0) {
                        android.util.Log.i(
                            "VideoReceiver",
                            "Rendered 30 (in decoder ${fedCount - outCount}, decoder ${decoderDelayNs / 1_000_000}ms, worst " +
                                "after due: fed +${worstFeedLateUs / 1000}ms, shown +${worstShownLateUs / 1000}ms)",
                        )
                        worstFeedLateUs = 0
                        worstShownLateUs = 0
                    }
                }
                // INFO_OUTPUT_FORMAT_CHANGED (-2) / INFO_OUTPUT_BUFFERS_CHANGED (-3) игнорируем
            } catch (e: Exception) {
                if (running) {
                    runCatching { dec?.stop() }
                    runCatching { dec?.release() }
                    dec = null
                    decoder = null
                    pending = null
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

        runCatching { dec?.stop() }
        runCatching { dec?.release() }
        decoder = null
    }

    fun stop() {
        running = false
        val t = loopThread
        loopThread = null
        runCatching { t?.join(500) }
        val dec = decoder
        decoder = null
        runCatching { dec?.stop() }
        runCatching { dec?.release() }
    }

    /** Предел размера для смены разрешения без пересоздания декодера: не меньше
     *  своего, до 1080p, в пределах того, что декодер умеет. `null` — декодер без
     *  adaptive playback: новый размер он примет только как сбой и перезапуск. */
    private fun adaptiveMax(codec: MediaCodec): Pair<Int, Int>? = runCatching {
        val caps = codec.codecInfo.getCapabilitiesForType("video/avc")
        if (!caps.isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_AdaptivePlayback)) return null
        val v = caps.videoCapabilities
        maxOf(width, 1920).coerceAtMost(v.supportedWidths.upper) to
            maxOf(height, 1080).coerceAtMost(v.supportedHeights.upper)
    }.getOrNull()

    private companion object {
        /** Поворот + срок кадра перед access-unit'ом. */
        const val PULL_HEADER = 5

        /** Дольше не ждём выхода декодера: пора снова подавать. */
        const val POLL_US = 4_000L

        /** Сглаживание задержки декодера (доля нового замера). */
        const val DELAY_SMOOTHING = 0.05
    }
}
