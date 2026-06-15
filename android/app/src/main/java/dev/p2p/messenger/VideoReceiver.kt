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
) {
    private var decoder: MediaCodec? = null
    private var loopThread: Thread? = null
    @Volatile private var running = false

    fun start() {
        val fmt = MediaFormat.createVideoFormat("video/avc", width, height)
        val dec = MediaCodec.createDecoderByType("video/avc")
        dec.configure(fmt, surface, null, 0) // вывод напрямую в surface
        dec.start()
        decoder = dec
        running = true
        loopThread = thread(name = "vdec") { loop(dec) }
    }

    private fun loop(dec: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        var pts = 0L
        val frameNs = 1_000_000_000L / VIDEO_FPS
        val bufferNs = 80_000_000L  // целевой буфер сглаживания (~80мс)
        val maxAheadNs = 260_000_000L // потолок задержки (иначе сбрасываем)
        var renderClock = 0L         // системное время следующего показа кадра
        while (running) {
            // Подача входных пакетов (по мере прихода из сети).
            val pkt = Native.callPullVideoPacket()
            if (pkt.isNotEmpty()) {
                val inIdx = try { dec.dequeueInputBuffer(10000) } catch (e: Exception) { break }
                if (inIdx >= 0) {
                    val ib = dec.getInputBuffer(inIdx)
                    if (ib != null && ib.capacity() >= pkt.size) {
                        ib.clear(); ib.put(pkt)
                        runCatching { dec.queueInputBuffer(inIdx, 0, pkt.size, pts, 0) }
                        pts += frameNs / 1000
                    } else {
                        runCatching { dec.queueInputBuffer(inIdx, 0, 0, pts, 0) }
                    }
                }
            } else {
                Thread.sleep(2)
            }
            // Вывод кадров на surface РОВНЫМ ТЕМПОМ: вместо «показать сейчас»
            // планируем показ по renderClock (frameNs между кадрами). SurfaceFlinger
            // покажет их по таймстампу — джиттер сети сглаживается.
            var outIdx = try { dec.dequeueOutputBuffer(info, 0) } catch (e: Exception) { -1 }
            while (outIdx >= 0) {
                val now = System.nanoTime()
                if (renderClock < now || renderClock > now + maxAheadNs) {
                    renderClock = now + bufferNs // старт/догон/сброс лишней задержки
                }
                runCatching { dec.releaseOutputBuffer(outIdx, renderClock) }
                renderClock += frameNs
                outIdx = try { dec.dequeueOutputBuffer(info, 0) } catch (e: Exception) { -1 }
            }
        }
    }

    fun stop() {
        running = false
        runCatching { loopThread?.join(500) }
        runCatching { decoder?.stop() }
        runCatching { decoder?.release() }
        decoder = null
    }
}
