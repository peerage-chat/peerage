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
 * Захват кадров с камеры (Camera2) для видео-звонка. Планы YUV_420_888 отдаются
 * НАПРЯМУЮ в нативный код ([Native.callPushVideoYuv]) — конвертация YUV→RGB и
 * кодирование идут в Rust. В Kotlin остаётся только дешёвое копирование планов
 * в переиспользуемые буферы (без пер-пиксельной математики и аллокаций на кадр).
 */
class CameraCapture(
    private val context: Context,
    private val width: Int,
    private val height: Int,
) {
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private var thread: HandlerThread? = null
    private var handler: Handler? = null
    // Переиспользуемые буферы планов (размеры постоянны в рамках сессии).
    private var yBuf: ByteArray? = null
    private var uBuf: ByteArray? = null
    private var vBuf: ByteArray? = null

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
                val yP = img.planes[0]; val uP = img.planes[1]; val vP = img.planes[2]
                val yb = yP.buffer; val ub = uP.buffer; val vb = vP.buffer
                val y = yBuf?.takeIf { it.size == yb.remaining() } ?: ByteArray(yb.remaining()).also { yBuf = it }
                val u = uBuf?.takeIf { it.size == ub.remaining() } ?: ByteArray(ub.remaining()).also { uBuf = it }
                val v = vBuf?.takeIf { it.size == vb.remaining() } ?: ByteArray(vb.remaining()).also { vBuf = it }
                yb.get(y); ub.get(u); vb.get(v)
                Native.callPushVideoYuv(y, u, v, img.width, img.height, yP.rowStride, uP.rowStride, uP.pixelStride)
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
}
