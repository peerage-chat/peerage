package dev.p2p.messenger

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

    /** Поддерживаемые 16:9-разрешения (≤ макс. выходного размера камеры). */
    fun resolutions(ctx: Context, front: Boolean): List<Res> {
        val ch = characteristics(ctx, front) ?: return buckets.take(2)
        val map = ch.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return buckets.take(2)
        val sizes = map.getOutputSizes(SurfaceTexture::class.java) ?: return buckets.take(2)
        val maxArea = sizes.maxOfOrNull { it.width.toLong() * it.height } ?: return buckets.take(2)
        return buckets.filter { it.width.toLong() * it.height <= maxArea }.ifEmpty { buckets.take(1) }
    }

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
