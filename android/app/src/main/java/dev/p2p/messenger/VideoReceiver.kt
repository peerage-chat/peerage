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
        android.util.Log.i("VideoReceiver", "start() called: width=$width, height=$height")
        running = true
        loopThread = thread(name = "vdec") { loop() }
    }

    private fun loop() {
        android.util.Log.i("VideoReceiver", "loop() started")
        var dec: MediaCodec? = null
        var pts = 0L
        val frameNs = 1_000_000_000L / VIDEO_FPS
        val info = MediaCodec.BufferInfo()
        var pulledCount = 0
        var renderedCount = 0
        var pendingPacket: ByteArray? = null

        while (running) {
            try {
                if (dec == null) {
                    android.util.Log.i("VideoReceiver", "Initializing MediaCodec H.264 decoder")
                    val fmt = MediaFormat.createVideoFormat("video/avc", width, height)
                    dec = MediaCodec.createDecoderByType("video/avc").apply {
                        configure(fmt, surface, null, 0)
                        start()
                    }
                    decoder = dec // сохраняем ссылку для stop()
                    android.util.Log.i("VideoReceiver", "Decoder started successfully")
                }

                val currentDec = dec

                val pkt = pendingPacket ?: pullVideoPacket()
                if (pkt.isNotEmpty()) {
                    val inIdx = currentDec.dequeueInputBuffer(10000)
                    if (inIdx >= 0) {
                        pendingPacket = null // successfully processed
                        pulledCount++
                        if (pulledCount % 30 == 0) {
                            android.util.Log.i("VideoReceiver", "Pulled 30 frames from JNI, last size = ${pkt.size} bytes")
                        }
                        val ib = currentDec.getInputBuffer(inIdx)
                        if (ib != null && ib.capacity() >= pkt.size) {
                            ib.clear(); ib.put(pkt)
                            currentDec.queueInputBuffer(inIdx, 0, pkt.size, pts, 0)
                            pts += frameNs / 1000
                        } else {
                            if (ib == null) {
                                android.util.Log.w("VideoReceiver", "getInputBuffer($inIdx) returned null")
                            } else {
                                android.util.Log.w("VideoReceiver", "Buffer capacity ${ib.capacity()} < packet size ${pkt.size}")
                            }
                            currentDec.queueInputBuffer(inIdx, 0, 0, pts, 0)
                        }
                    } else {
                        // Dequeue input buffer timed out. Keep the packet for the next loop.
                        pendingPacket = pkt
                        Thread.sleep(2)
                    }
                } else {
                    pendingPacket = null
                    Thread.sleep(2)
                }

                var outIdx = try { currentDec.dequeueOutputBuffer(info, 0) } catch (e: Exception) { -1 }
                while (outIdx >= 0) {
                    runCatching { currentDec.releaseOutputBuffer(outIdx, true) }
                    renderedCount++
                    if (renderedCount % 30 == 0) {
                        android.util.Log.i("VideoReceiver", "Rendered 30 frames to surface")
                    }
                    outIdx = try { currentDec.dequeueOutputBuffer(info, 0) } catch (e: Exception) { -1 }
                }
            } catch (e: Exception) {
                if (running) {
                    android.util.Log.e("VideoReceiver", "Decoder error, restarting...", e)
                    runCatching { dec?.stop() }
                    runCatching { dec?.release() }
                    dec = null
                    decoder = null
                    pendingPacket = null // Clear pending packet on decoder reset
                    Thread.sleep(100) // пауза перед перезапуском
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
