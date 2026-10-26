package dev.p2p.messenger.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.p2p.messenger.AppModel
import dev.p2p.messenger.HoloAccent
import dev.p2p.messenger.HoloAccentDark
import dev.p2p.messenger.HoloBg
import dev.p2p.messenger.R
import dev.p2p.messenger.Tokens
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.io.ByteArrayOutputStream

/** Сторона аватара в пикселях: с запасом на самый крупный показ (96dp на xxxhdpi). */
private const val AVATAR_PX = 512

/**
 * Выбор своего аватара: системный Photo Picker → квадрат [AVATAR_PX] → движок.
 * Один на все входы (онбординг, профиль, правка профиля), чтобы аватар везде
 * готовился одинаково. Возвращает «открыть выбор».
 */
@Composable
internal fun rememberAvatarPicker(model: AppModel): () -> Unit {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch(Dispatchers.IO) {
            val jpeg = avatarJpeg(ctx, uri) ?: return@launch
            model.setSelfAvatar(jpeg, "image/jpeg")
        }
    }
    return { picker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
}

/**
 * Свой аватар, который можно сменить тапом: значок камеры в углу говорит об этом
 * без подписи (так же, как на шаге профиля в онбординге).
 */
@Composable
internal fun EditableAvatar(name: String, avatar: ByteArray?, onPick: () -> Unit) {
    Box(
        contentAlignment = Alignment.BottomEnd,
        modifier = Modifier.clickable(onClick = onPick),
    ) {
        SquareAvatar(name, size = Tokens.Size.avatarBig, avatarBytes = avatar, circle = true, ring = HoloBg)
        Box(
            Modifier.size(34.dp).clip(CircleShape).background(HoloAccent).border(3.dp, HoloBg, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                TablerIcons.Camera,
                stringResource(R.string.settings_tap_avatar),
                tint = HoloAccentDark,
                modifier = Modifier.size(17.dp),
            )
        }
    }
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
