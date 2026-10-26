package chat.peerage

import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager

/**
 * Возможности камеры устройства для экрана шаринга. Список разрешений/fps НЕ
 * хардкод — фильтруется по реальному [CameraCharacteristics] выбранной камеры
 * (StreamConfigurationMap + AE target-fps). Так бейдж «доступно на устройстве»
 * честный: на слабом сенсоре 1440p/4K просто не появятся.
 */
object CameraCaps {
    data class Res(val label: String, val width: Int, val height: Int)

    /** Стандартные 16:9-корзины, которые предлагаем, если сенсор их тянет. */
    private val buckets = listOf(
        Res("720p", 1280, 720),
        Res("1080p", 1920, 1080),
        Res("1440p", 2560, 1440),
        Res("4K", 3840, 2160),
    )

    private fun cameraId(cm: CameraManager, front: Boolean): String? {
        val want = if (front) {
            CameraCharacteristics.LENS_FACING_FRONT
        } else {
            CameraCharacteristics.LENS_FACING_BACK
        }
        return cm.cameraIdList.firstOrNull {
            cm.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == want
        }
    }

    private fun characteristics(ctx: Context, front: Boolean): CameraCharacteristics? = runCatching {
        val cm = ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val id = cameraId(cm, front) ?: return null
        cm.getCameraCharacteristics(id)
    }.getOrNull()

    /** Реальные разрешения УСТРОЙСТВА (не жёсткая карта): пересечение выходов камеры и
     *  возможностей H264-энкодера, аспект ≈16:9 (допуск ловит нестандартные вроде
     *  2592×1460), с учётом выравнивания энкодера. Список честный — что показано, то и
     *  кодируется. 4:3/1:1 не предлагаем (для звонка 16:9 эффективнее, тайл кропает);
     *  если 16:9 нет вовсе — фолбэк. */
    fun resolutions(ctx: Context, front: Boolean): List<Res> {
        val ch = characteristics(ctx, front) ?: return fallback()
        val map = ch.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return fallback()
        // getOutputSizes(SurfaceTexture) = формат IMPLEMENTATION_DEFINED — РОВНО то, что
        // камеро-HAL вычитал под VIDEO_ENCODER на этом чипе. Это и есть источник истины
        // (MediaCodec.isSizeSupported консервативен и врёт, напр. отказывал в 4К, который
        // железо реально кодирует). Никакого доп. гейта — что в списке, то и снимается.
        val sizes = map.getOutputSizes(SurfaceTexture::class.java) ?: return fallback()
        return sizes
            .filter { s ->
                val ratio = s.width.toDouble() / s.height
                ratio in 1.70..1.85 && s.height in 480..2160 // ~16:9 (ловит 2592×1460); 4:3/1:1 не для звонка
            }
            .sortedByDescending { it.width.toLong() * it.height }
            .distinctBy { heightClass(it.height) } // один пункт на класс высоты (самый широкий)
            .map { Res(label(it.height), it.width, it.height) }
            .ifEmpty { fallback() }
    }

    private fun heightClass(h: Int): Int = when {
        h <= 600 -> 480
        h <= 900 -> 720
        h <= 1200 -> 1080
        h <= 1600 -> 1440
        else -> 2160
    }

    private fun label(h: Int): String = when (heightClass(h)) {
        480 -> "480p"
        720 -> "720p"
        1080 -> "1080p"
        1440 -> "1440p"
        else -> "4K"
    }

    private fun fallback(): List<Res> = buckets.take(2)

    /** Тянет ли камера ≥60 fps (по AE target-fps-диапазонам). */
    fun supports60(ctx: Context, front: Boolean): Boolean {
        val ch = characteristics(ctx, front) ?: return false
        val ranges = ch.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) ?: return false
        return ranges.any { it.upper >= 60 }
    }

    /** Id камеры нужного объектива (для живого превью). */
    fun cameraId(ctx: Context, front: Boolean): String? = runCatching {
        cameraId(ctx.getSystemService(Context.CAMERA_SERVICE) as CameraManager, front)
    }.getOrNull()

    /** Угол сенсора камеры (0/90/180/270) — для доворота превью. */
    fun sensorOrientation(ctx: Context, front: Boolean): Int =
        characteristics(ctx, front)?.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
}
