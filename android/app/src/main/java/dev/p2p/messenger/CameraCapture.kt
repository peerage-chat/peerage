package dev.p2p.messenger

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.Image
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface

/**
 * Захват кадров с камеры (Camera2) в RGB8 для видео-звонка. Каждый кадр
 * конвертируется из YUV_420_888 в плотный RGB (w×h×3) и отдаётся в [onFrame]
 * (обычно → [Native.callPushVideo]). Разрешение фиксировано под видео-движок.
 */
class CameraCapture(
    private val context: Context,
    private val width: Int,
    private val height: Int,
    private val onFrame: (ByteArray) -> Unit,
) {
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private var thread: HandlerThread? = null
    private var handler: Handler? = null
    private val rgb = ByteArray(width * height * 3)

    @SuppressLint("MissingPermission") // CAMERA запрашивается до старта видео-звонка
    fun start() {
        val mgr = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val camId = pickFrontCamera(mgr) ?: return
        val t = HandlerThread("camera").apply { start() }
        thread = t
        handler = Handler(t.looper)

        val r = ImageReader.newInstance(width, height, ImageFormat.YUV_420_888, 2)
        r.setOnImageAvailableListener({ ir ->
            val img = ir.acquireLatestImage() ?: return@setOnImageAvailableListener
            try {
                yuv420ToRgb(img, rgb)
                onFrame(rgb.copyOf())
            } finally {
                img.close()
            }
        }, handler)
        reader = r

        mgr.openCamera(camId, object : CameraDevice.StateCallback() {
            override fun onOpened(cam: CameraDevice) {
                device = cam
                val surface = r.surface
                @Suppress("DEPRECATION")
                cam.createCaptureSession(listOf(surface), object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(s: CameraCaptureSession) {
                        session = s
                        val req = cam.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                            addTarget(surface)
                        }
                        s.setRepeatingRequest(req.build(), null, handler)
                    }
                    override fun onConfigureFailed(s: CameraCaptureSession) {}
                }, handler)
            }
            override fun onDisconnected(cam: CameraDevice) { cam.close() }
            override fun onError(cam: CameraDevice, error: Int) { cam.close() }
        }, handler)
    }

    fun stop() {
        runCatching { session?.close() }
        runCatching { device?.close() }
        runCatching { reader?.close() }
        runCatching { thread?.quitSafely() }
        session = null; device = null; reader = null; thread = null; handler = null
    }

    private fun pickFrontCamera(mgr: CameraManager): String? {
        var fallback: String? = null
        for (id in mgr.cameraIdList) {
            val facing = mgr.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING)
            if (fallback == null) fallback = id
            if (facing == CameraCharacteristics.LENS_FACING_FRONT) return id
        }
        return fallback
    }

    /** YUV_420_888 → плотный RGB8 (учитывает row/pixel stride планов). */
    private fun yuv420ToRgb(image: Image, out: ByteArray) {
        val w = minOf(image.width, width)
        val h = minOf(image.height, height)
        val yP = image.planes[0]; val uP = image.planes[1]; val vP = image.planes[2]
        val yB = yP.buffer; val uB = uP.buffer; val vB = vP.buffer
        val yRow = yP.rowStride
        val uRow = uP.rowStride; val uPix = uP.pixelStride
        val vRow = vP.rowStride; val vPix = vP.pixelStride
        var o = 0
        for (j in 0 until h) {
            val yr = j * yRow
            val uvRow = j shr 1
            for (i in 0 until w) {
                val yv = (yB.get(yr + i).toInt() and 0xff)
                val uvCol = i shr 1
                val u = (uB.get(uvRow * uRow + uvCol * uPix).toInt() and 0xff) - 128
                val v = (vB.get(uvRow * vRow + uvCol * vPix).toInt() and 0xff) - 128
                val r = yv + ((1.370705f * v).toInt())
                val g = yv - ((0.337633f * u).toInt()) - ((0.698001f * v).toInt())
                val b = yv + ((1.732446f * u).toInt())
                out[o++] = r.coerceIn(0, 255).toByte()
                out[o++] = g.coerceIn(0, 255).toByte()
                out[o++] = b.coerceIn(0, 255).toByte()
            }
        }
    }
}
