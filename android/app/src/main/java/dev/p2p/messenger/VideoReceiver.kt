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
        while (running) {
            val pkt = Native.callPullVideoPacket()
            if (pkt.isNotEmpty()) {
                val inIdx = try { dec.dequeueInputBuffer(10000) } catch (e: Exception) { break }
                if (inIdx >= 0) {
                    val ib = dec.getInputBuffer(inIdx)
                    if (ib != null && ib.capacity() >= pkt.size) {
                        ib.clear()
                        ib.put(pkt)
                        runCatching { dec.queueInputBuffer(inIdx, 0, pkt.size, pts, 0) }
                        pts += 50_000 // ~20fps, мкс
                    } else {
                        runCatching { dec.queueInputBuffer(inIdx, 0, 0, pts, 0) }
                    }
                }
            } else {
                Thread.sleep(4)
            }
            // Слить готовые кадры на surface.
            var outIdx = try { dec.dequeueOutputBuffer(info, 0) } catch (e: Exception) { -1 }
            while (outIdx >= 0) {
                runCatching { dec.releaseOutputBuffer(outIdx, true) } // true = рендер
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
