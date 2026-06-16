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
    private val pullVideoPacket: () -> ByteArray
) {
    private var decoder: MediaCodec? = null
    private var loopThread: Thread? = null
    @Volatile private var running = false

    fun start() {
        running = true
        loopThread = thread(name = "vdec") { loop() }
    }

    private fun loop() {
        var dec: MediaCodec? = null
        var pts = 0L
        val frameNs = 1_000_000_000L / VIDEO_FPS
        val bufferNs = 80_000_000L  // целевой буфер сглаживания (~80мс)
        val maxAheadNs = 260_000_000L // потолок задержки (иначе сбрасываем)
        var renderClock = 0L         // системное время следующего показа кадра
        val info = MediaCodec.BufferInfo()

        while (running) {
            try {
                if (dec == null) {
                    val fmt = MediaFormat.createVideoFormat("video/avc", width, height)
                    dec = MediaCodec.createDecoderByType("video/avc").apply {
                        configure(fmt, surface, null, 0)
                        start()
                    }
                    decoder = dec // сохраняем ссылку для stop()
                }

                val pkt = pullVideoPacket()
                if (pkt.isNotEmpty()) {
                    val inIdx = dec.dequeueInputBuffer(10000)
                    if (inIdx >= 0) {
                        val ib = dec.getInputBuffer(inIdx)
                        if (ib != null && ib.capacity() >= pkt.size) {
                            ib.clear(); ib.put(pkt)
                            dec.queueInputBuffer(inIdx, 0, pkt.size, pts, 0)
                            pts += frameNs / 1000
                        } else {
                            dec.queueInputBuffer(inIdx, 0, 0, pts, 0)
                        }
                    }
                } else {
                    Thread.sleep(2)
                }

                var outIdx = dec.dequeueOutputBuffer(info, 0)
                while (outIdx >= 0) {
                    val now = System.nanoTime()
                    if (renderClock < now || renderClock > now + maxAheadNs) {
                        renderClock = now + bufferNs
                    }

                    // Если кадр слишком старый (отстает от renderClock больше чем на буфер сглаживания),
                    // рендерим его немедленно (true), чтобы не копить задержку.
                    val isLate = (now - renderClock) > bufferNs
                    if (isLate) {
                        dec.releaseOutputBuffer(outIdx, true)
                        renderClock = now
                    } else {
                        dec.releaseOutputBuffer(outIdx, renderClock)
                    }

                    renderClock += frameNs
                    outIdx = dec.dequeueOutputBuffer(info, 0)
                }
            } catch (e: Exception) {
                android.util.Log.e("VideoReceiver", "Decoder error, restarting...", e)
                runCatching { dec?.stop() }
                runCatching { dec?.release() }
                dec = null
                decoder = null
                Thread.sleep(100) // пауза перед перезапуском
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
