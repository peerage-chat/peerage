package dev.p2p.messenger

import android.annotation.SuppressLint
import android.content.Context
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.util.Range
import android.view.Surface
import kotlin.concurrent.thread

/**
 * Видео-отправитель: Camera2 рендерит напрямую в input-Surface аппаратного
 * H.264-энкодера (MediaCodec, zero-copy — без YUV→RGB и софт-кодека). Выходные
 * access-unit'ы уходят в Rust ([Native.callSendVideo]) для фрагментации/отправки.
 * SPS/PPS (CODEC_CONFIG) добавляются к каждому keyframe — чтобы приёмник мог
 * инициализировать декодер «с середины» потока.
 */
class VideoSender(
    private val context: Context,
    private val width: Int,
    private val height: Int,
    private val fps: Int,
    private val bitrate: Int,
    private val sendVideo: (ByteArray, Boolean) -> Unit
) {
    private var encoder: MediaCodec? = null
    private var inputSurface: Surface? = null
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var camThread: HandlerThread? = null
    private var camHandler: Handler? = null
    private var drainThread: Thread? = null
    @Volatile private var running = false

    @SuppressLint("MissingPermission") // CAMERA запрашивается до видео-звонка
    fun start() {
        android.util.Log.i("VideoSender", "start() called: width=$width, height=$height, fps=$fps, bitrate=$bitrate")
        val fmt = MediaFormat.createVideoFormat("video/avc", width, height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, bitrate)
            setInteger(MediaFormat.KEY_FRAME_RATE, fps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1) // keyframe ~раз в секунду
        }
        val enc = MediaCodec.createEncoderByType("video/avc")
        enc.configure(fmt, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        val surf = enc.createInputSurface()
        enc.start()
        encoder = enc
        inputSurface = surf
        running = true
        drainThread = thread(name = "venc-drain") { drain(enc) }
        openCamera(surf)
    }

    @SuppressLint("MissingPermission")
    private fun openCamera(target: Surface) {
        val mgr = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val camId = pickFront(mgr)
        if (camId == null) {
            android.util.Log.e("VideoSender", "No camera found (facing front or fallback)!")
            return
        }
        android.util.Log.i("VideoSender", "Opening camera: $camId")
        val t = HandlerThread("venc-cam").apply { start() }
        camThread = t
        camHandler = Handler(t.looper)
        mgr.openCamera(camId, object : CameraDevice.StateCallback() {
            override fun onOpened(cam: CameraDevice) {
                android.util.Log.i("VideoSender", "Camera $camId opened successfully")
                device = cam
                @Suppress("DEPRECATION")
                cam.createCaptureSession(listOf(target), object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(s: CameraCaptureSession) {
                        android.util.Log.i("VideoSender", "Camera capture session configured")
                        session = s
                        val req = cam.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                            addTarget(target)
                            // Ограничиваем частоту камеры под целевой fps (меньше
                            // перепроизводства кадров → меньше рывков/трафика).
                            val range = pickFpsRange(mgr, camId)
                            android.util.Log.i("VideoSender", "AE Target FPS Range selected: $range")
                            range?.let {
                                set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, it)
                            }
                        }
                        runCatching { s.setRepeatingRequest(req.build(), null, camHandler) }
                        // Камера только начала подавать кадры — гарантируем I-frame немедленно.
                        // requestKeyframe() на call_accept срабатывает до открытия камеры
                        // и теряется; здесь момент всегда правильный.
                        requestKeyframe()
                    }
                    override fun onConfigureFailed(s: CameraCaptureSession) {
                        android.util.Log.e("VideoSender", "Camera capture session configuration failed")
                    }
                }, camHandler)
            }
            override fun onDisconnected(cam: CameraDevice) {
                android.util.Log.w("VideoSender", "Camera $camId disconnected")
                cam.close()
            }
            override fun onError(cam: CameraDevice, error: Int) {
                android.util.Log.e("VideoSender", "Camera $camId error: $error")
                cam.close()
            }
        }, camHandler)
    }

    private fun drain(enc: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        var config: ByteArray? = null // SPS+PPS
        var encodedCount = 0
        android.util.Log.i("VideoSender", "venc-drain thread started")
        while (running) {
            val idx = try { enc.dequeueOutputBuffer(info, 10000) } catch (e: Exception) {
                android.util.Log.e("VideoSender", "Encoder dequeueOutputBuffer exception", e)
                break
            }
            if (idx < 0) continue
            val buf = enc.getOutputBuffer(idx)
            if (buf != null && info.size > 0) {
                val data = ByteArray(info.size)
                buf.position(info.offset)
                buf.limit(info.offset + info.size)
                buf.get(data)
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                    android.util.Log.i("VideoSender", "Received CODEC_CONFIG SPS/PPS size = ${data.size} bytes")
                    config = data
                } else {
                    val key = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
                    val cfg = config
                    val out = if (key && cfg != null) cfg + data else data
                    encodedCount++
                    if (encodedCount % 30 == 0) {
                        android.util.Log.i("VideoSender", "Encoded and sent 30 frames, last size = ${out.size} bytes (key=$key)")
                    }
                    sendVideo(out, key)
                }
            }
            runCatching { enc.releaseOutputBuffer(idx, false) }
        }
    }

    fun requestKeyframe() {
        val enc = encoder ?: return
        val p = android.os.Bundle()
        p.putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0)
        runCatching { enc.setParameters(p) }
    }

    fun stop() {
        running = false
        // Сначала ОСТАНОВИТЬ камеру (иначе HAL виснет «waiting for requests» и
        // течёт память), потом закрыть сессию/устройство, затем энкодер.
        runCatching { session?.stopRepeating() }
        runCatching { session?.abortCaptures() }
        runCatching { drainThread?.join(500) }
        runCatching { session?.close() }
        runCatching { device?.close() }
        runCatching { encoder?.stop() }
        runCatching { encoder?.release() }
        runCatching { inputSurface?.release() }
        runCatching { camThread?.quitSafely() }
        session = null; device = null; encoder = null; inputSurface = null; camThread = null
    }

    /** FPS-диапазон, максимально близкий к ФИКСИРОВАННОМУ целевому fps. Фикс.
     *  диапазон (lower==upper) важен: тогда камера не «плавает» по частоте (в т.ч.
     *  не падает до 15 в темноте) — иначе приёмник, темпирующий ровно на fps,
     *  ловит дрифт и периодические рывки. null — оставить авто. */
    private fun pickFpsRange(mgr: CameraManager, camId: String): Range<Int>? = runCatching {
        val ranges = mgr.getCameraCharacteristics(camId)
            .get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) ?: return null
        val needsScale = ranges.any { it.upper > 1000 }
        val targetFps = if (needsScale) fps * 1000 else fps
        ranges.firstOrNull { it.lower == targetFps && it.upper == targetFps }                       // точный фикс [fps,fps]
            ?: ranges.filter { it.lower == it.upper && it.upper <= targetFps }.maxByOrNull { it.upper } // любой фикс ≤ fps
            ?: ranges.filter { it.upper <= targetFps }.maxByOrNull { it.lower.toLong() * 1000 + it.upper } // наименее «плавающий»
            ?: ranges.minByOrNull { it.upper }                                          // запас: наименьший верх
    }.getOrNull()

    private fun pickFront(mgr: CameraManager): String? {
        var fallback: String? = null
        for (id in mgr.cameraIdList) {
            val facing = mgr.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING)
            if (fallback == null) fallback = id
            if (facing == CameraCharacteristics.LENS_FACING_FRONT) return id
        }
        return fallback
    }
}
