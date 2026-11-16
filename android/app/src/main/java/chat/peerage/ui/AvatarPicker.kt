package chat.peerage.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import chat.peerage.AppModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream

/** Сторона аватара в пикселях: с запасом на самый крупный показ (96dp на xxxhdpi). */
private const val AVATAR_PX = 512

/**
 * Выбор своего аватара: системный Photo Picker → квадрат [AVATAR_PX] → движок.
 * Один на все входы (онбординг, профиль, правка профиля), чтобы аватар везде
 * готовился одинаково. Возвращает «открыть выбор».
 */
@Composable
internal fun rememberAvatarPicker(model: AppModel): () -> Unit = rememberImagePicker { model.setSelfAvatar(it, "image/jpeg") }

/**
 * Картинка-аватар из системного Photo Picker: квадрат [AVATAR_PX], JPEG без метаданных.
 * Тем же путём готовится и фото группы. [onJpeg] — на главном потоке.
 */
@Composable
internal fun rememberImagePicker(onJpeg: (ByteArray) -> Unit): () -> Unit {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch(Dispatchers.IO) {
            val jpeg = avatarJpeg(ctx, uri) ?: return@launch
            withContext(Dispatchers.Main) { onJpeg(jpeg) }
        }
    }
    return { picker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
}

/**
 * Картинка → квадрат по центру, [AVATAR_PX], JPEG. Исходник не отправляем: аватар
 * получает и хранит каждый контакт, а фото с камеры — это мегабайты ради кружка
 * в 44dp. Перекодирование заодно не несёт ни одного байта метаданных снимка.
 */
private fun avatarJpeg(ctx: Context, uri: Uri): ByteArray? = runCatching {
    val src = decodeUpright(ctx, uri) ?: return null
    val side = minOf(src.width, src.height)
    val square = Bitmap.createBitmap(src, (src.width - side) / 2, (src.height - side) / 2, side, side)
    val out = if (side > AVATAR_PX) Bitmap.createScaledBitmap(square, AVATAR_PX, AVATAR_PX, true) else square
    ByteArrayOutputStream().use { buf ->
        out.compress(Bitmap.CompressFormat.JPEG, 88, buf)
        buf.toByteArray()
    }
}.getOrNull()

/** Декодировать уже уменьшенным (фото с камеры целиком в память не влезает с запасом)
 *  и повёрнутым как надо: ImageDecoder сам применяет EXIF-поворот. До Android 9 его
 *  нет — там снимок может лечь боком, но это редкие устройства. */
private fun decodeUpright(ctx: Context, uri: Uri): Bitmap? {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        val source = ImageDecoder.createSource(ctx.contentResolver, uri)
        return ImageDecoder.decodeBitmap(source) { dec, info, _ ->
            val side = minOf(info.size.width, info.size.height)
            if (side > AVATAR_PX * 2) {
                val k = side / (AVATAR_PX * 2)
                dec.setTargetSize(info.size.width / k, info.size.height / k)
            }
            // Программный буфер: из аппаратного createBitmap/compress не читают.
            dec.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
    }
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    ctx.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
    var sample = 1
    while (minOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= AVATAR_PX) sample *= 2
    val opts = BitmapFactory.Options().apply { inSampleSize = sample }
    return ctx.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, opts) }
}
