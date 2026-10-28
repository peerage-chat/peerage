package chat.peerage

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
    startFront: Boolean = true,
    /** (access-unit, ключевой, угол сенсора / 90, fps). */
    private val sendVideo: (ByteArray, Boolean, Int, Int) -> Unit,
) {
    private var encoder: MediaCodec? = null
    private var inputSurface: Surface? = null
    private var device: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var camThread: HandlerThread? = null
    private var camHandler: Handler? = null
    private var drainThread: Thread? = null

    @Volatile private var running = false
    private var camId: String? = null

    /** Ориентация сенсора камеры (0/90/180/270). Энкодер пишет сырой кадр с сенсора
     *  (без коррекции, в отличие от превью-SurfaceTexture) → шлём угол приёмнику,
     *  чтобы он довернул картинку до «правильной». */
    @Volatile private var sensorOrientation = 0

    /** Surface тайла «Вы» — локальное превью прямо с камеры (без энкодера). */
    @Volatile private var previewSurface: Surface? = null

    /** Какую камеру открывать: фронтальную (true) или тыловую. Стартовая сторона —
     *  из [startFront] (экран шаринга может выбрать тыловую); меняется флипом. */
    @Volatile private var facingFront = startFront

    /** [preview] — surface тайла «Вы» для локального превью (или null). Камера
     *  рендерит в энкодер + (если задан) превью ОДНОЙ сессией — без пересборки. */
    @SuppressLint("MissingPermission") // CAMERA запрашивается до видео-звонка
    fun start(preview: Surface? = null) {
        android.util.Log.i("VideoSender", "start(): ${width}x$height@$fps, preview=${preview != null}")
        previewSurface = preview
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

    /** Переключить фронт/тыл по ходу звонка. Энкодер остаётся (тот же Surface) —
     *  пересоздаём только сессию камеры под новый объектив. Угол сенсора другой →
     *  заголовок ориентации в потоке обновится сам (читается в drain). */
    fun switchCamera() {
        val h = camHandler ?: return
        h.post {
            val surf = inputSurface ?: return@post
            facingFront = !facingFront
            runCatching { session?.close() }
            runCatching { device?.close() }
            device = null
            openCamera(surf)
        }
    }

    @SuppressLint("MissingPermission")
    private fun openCamera(target: Surface) {
        val mgr = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val cid = pickCamera(mgr)
        if (cid == null) {
            android.util.Log.e("VideoSender", "No camera found (facing front or fallback)!")
            return
        }
        camId = cid
        sensorOrientation = runCatching {
            mgr.getCameraCharacteristics(cid).get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
        }.getOrDefault(0)
        android.util.Log.i("VideoSender", "Opening camera: $cid (sensorOrientation=$sensorOrientation)")
        runCatching { camThread?.quitSafely() } // не плодить потоки при ретрае открытия
        val t = HandlerThread("venc-cam").apply { start() }
        camThread = t
        camHandler = Handler(t.looper)
        try {
            mgr.openCamera(
                cid,
                object : CameraDevice.StateCallback() {
                    override fun onOpened(cam: CameraDevice) {
                        android.util.Log.i("VideoSender", "Camera $cid opened successfully")
                        device = cam
                        configureSession(cam)
                    }
                    override fun onDisconnected(cam: CameraDevice) {
                        android.util.Log.w("VideoSender", "Camera $cid disconnected")
                        cam.close()
                        device = null
                        // Камеру забрали (другое приложение/система) — пробуем вернуть,
                        // как в onError. Иначе видео встаёт навсегда до перезвона.
                        camHandler?.postDelayed({ if (running) openCamera(target) }, 500)
                    }
                    override fun onError(cam: CameraDevice, error: Int) {
                        android.util.Log.e("VideoSender", "Camera $cid error: $error")
                        cam.close()
                        // Повторная попытка через 500мс — камера могла быть занята предыдущей сессией
                        camHandler?.postDelayed({ if (running) openCamera(target) }, 500)
                    }
                },
                camHandler,
            )
        } catch (e: Exception) {
            android.util.Log.e("VideoSender", "openCamera failed: ${e.message}")
            camHandler?.postDelayed({ if (running) openCamera(target) }, 500)
        }
    }

    /** (Пере)создать capture-сессию с текущими целями: энкодер + (опц.) превью-surface
     *  тайла «Вы». Превью идёт ПРЯМО с камеры — без отдельного энкодера/декодера.
     *  Camera2 не даёт добавить цель в готовую сессию → пересоздаём (редко: при
     *  появлении/уходе превью). */
    @Suppress("DEPRECATION")
    private fun configureSession(cam: CameraDevice) {
        val enc = inputSurface ?: return
        val cid = camId ?: return
        val mgr = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val targets = listOfNotNull(enc, previewSurface)
        runCatching { session?.close() }
        cam.createCaptureSession(
            targets,
            object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(s: CameraCaptureSession) {
                    android.util.Log.i("VideoSender", "Session configured (targets=${targets.size})")
                    session = s
                    val req = cam.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                        targets.forEach { addTarget(it) }
                        val range = pickFpsRange(mgr, cid)
                        range?.let { set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, it) }
                    }
                    runCatching { s.setRepeatingRequest(req.build(), null, camHandler) }
                    requestKeyframe()
                }
                override fun onConfigureFailed(s: CameraCaptureSession) {
                    android.util.Log.e("VideoSender", "Camera capture session configuration failed")
                }
            },
            camHandler,
        )
    }

    private fun drain(enc: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        var config: ByteArray? = null // SPS+PPS
        var encodedCount = 0
        var errStreak = 0
        android.util.Log.i("VideoSender", "venc-drain thread started")
        while (running) {
            val idx = try {
                enc.dequeueOutputBuffer(info, 10000)
            } catch (e: Exception) {
                // Разовый сбой не должен убивать дренаж на весь звонок. Переживаем
                // транзиент; выходим лишь если энкодер стабильно мёртв (10 подряд).
                android.util.Log.e("VideoSender", "Encoder dequeue exception (streak=${errStreak + 1})", e)
                if (++errStreak >= 10) {
                    android.util.Log.e("VideoSender", "encoder unrecoverable, drain stops")
                    break
                }
                Thread.sleep(20)
                continue
            }
            errStreak = 0
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
                    val au = if (key && cfg != null) cfg + data else data
                    // Ориентация (угол сенсора/90) — приёмник доворачивает; fps — приёмник
                    // пейсит по НЕМУ, а не по своей настройке (иначе разный fps → видео не
                    // на той скорости). Оба едут в протоколе LiteRTC рядом с кадром.
                    encodedCount++
                    if (encodedCount % 30 == 0) {
                        android.util.Log.i("VideoSender", "Encoded and sent 30 frames, last size = ${au.size} bytes (key=$key)")
                    }
                    sendVideo(au, key, (sensorOrientation / 90) % 4, fps)
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
        session = null
        device = null
        encoder = null
        inputSurface = null
        camThread = null
        previewSurface = null
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
        ranges.firstOrNull { it.lower == targetFps && it.upper == targetFps } // точный фикс [fps,fps]
            ?: ranges.filter { it.lower == it.upper && it.upper <= targetFps }.maxByOrNull { it.upper } // любой фикс ≤ fps
            ?: ranges.filter { it.upper <= targetFps }.maxByOrNull { it.lower.toLong() * 1000 + it.upper } // наименее «плавающий»
            ?: ranges.minByOrNull { it.upper } // запас: наименьший верх
    }.getOrNull()

    /** Камера нужного объектива ([facingFront]); если такой нет — первая попавшаяся. */
    private fun pickCamera(mgr: CameraManager): String? {
        val want = if (facingFront) {
            CameraCharacteristics.LENS_FACING_FRONT
        } else {
            CameraCharacteristics.LENS_FACING_BACK
        }
        var fallback: String? = null
        for (id in mgr.cameraIdList) {
            val facing = mgr.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING)
            if (fallback == null) fallback = id
            if (facing == want) return id
        }
        return fallback
    }
}
