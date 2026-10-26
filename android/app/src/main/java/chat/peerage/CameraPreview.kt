package chat.peerage

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.os.Handler
import android.os.HandlerThread
import android.view.Surface
import android.view.TextureView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView

/**
 * Живое превью камеры (просмотр, без энкодера) для экрана шаринга. Открывает камеру
 * выбранного объектива в [TextureView]; доворот по углу сенсора + зеркало у фронталки
 * (cover-crop как у приёмника видео). Использовать ТОЛЬКО когда камера свободна
 * (видео в звонке выключено) — иначе конфликт с [VideoSender] (камера single-access).
 * Смена фронт/тыл — через `key(front)` на вызывающей стороне (полный пересоздание).
 */
@Composable
fun CameraPreview(front: Boolean, modifier: Modifier) {
    val ctx = LocalContext.current
    val controller = remember(front) { CameraPreviewController(ctx) }
    AndroidView(
        factory = { c ->
            TextureView(c).apply {
                surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                    override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                        controller.start(this@apply, st, front)
                    }

                    override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {}

                    override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean {
                        controller.stop()
                        return true
                    }

                    override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
                }
            }
        },
        modifier = modifier,
    )
    DisposableEffect(front) { onDispose { controller.stop() } }
}

/** Жизненный цикл одной превью-сессии камеры (open → repeating preview → close). */
private class CameraPreviewController(private val ctx: Context) {
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var thread: HandlerThread? = null
    private var handler: Handler? = null

    @Volatile private var closed = false

    @SuppressLint("MissingPermission") // CAMERA уже выдан для видеозвонков
    fun start(view: TextureView, st: SurfaceTexture, front: Boolean) {
        val id = CameraCaps.cameraId(ctx, front) ?: return
        val bw = 1280
        val bh = 720
        st.setDefaultBufferSize(bw, bh)
        // Те же повороты, что у своего превью в звонке (SelfPreviewSurface): камера уже
        // доворачивает в портрет → матрицу считаем по ПОРТРЕТНЫМ размерам (bh×bw),
        // rotation=0, зеркало для фронталки.
        applyTextureTransform(view, bh, bw, 0, mirror = front)
        val surface = Surface(st)
        thread = HandlerThread("cam-preview").also { it.start() }
        handler = Handler(thread!!.looper)
        val mgr = ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        runCatching {
            mgr.openCamera(
                id,
                object : CameraDevice.StateCallback() {
                    override fun onOpened(cam: CameraDevice) {
                        if (closed) {
                            cam.close()
                            return
                        }
                        device = cam
                        val req = cam.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply { addTarget(surface) }
                        cam.createCaptureSession(
                            listOf(surface),
                            object : CameraCaptureSession.StateCallback() {
                                override fun onConfigured(s: CameraCaptureSession) {
                                    if (closed) {
                                        s.close()
                                        return
                                    }
                                    session = s
                                    runCatching { s.setRepeatingRequest(req.build(), null, handler) }
                                }

                                override fun onConfigureFailed(s: CameraCaptureSession) {}
                            },
                            handler,
                        )
                    }

                    override fun onDisconnected(cam: CameraDevice) {
                        cam.close()
                    }

                    override fun onError(cam: CameraDevice, error: Int) {
                        cam.close()
                    }
                },
                handler,
            )
        }
    }

    fun stop() {
        closed = true
        runCatching { session?.close() }
        session = null
        runCatching { device?.close() }
        device = null
        thread?.quitSafely()
        thread = null
        handler = null
    }
}
