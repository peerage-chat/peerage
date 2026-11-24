package chat.peerage.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Аватар профиля, который по тапу открывается в просмотрщике ([AvatarViewer]). Без картинки —
 * обычный аватар: букву разглядывать незачем. Сам аватар — место возврата: протяжка вниз
 * уводит снимок обратно в него, как фото из чата — в пузырь.
 */
@Composable
internal fun OpenableAvatar(
    model: AppModel,
    title: String,
    bytes: ByteArray?,
    square: Boolean = false,
    avatar: @Composable () -> Unit,
) {
    var open by remember { mutableStateOf(false) }
    val key = remember { Any() }
    // Отклик нажатия — по форме аватара: круг у человека, скруглённый квадрат у группы.
    val shape = if (square) androidx.compose.foundation.shape.RoundedCornerShape(Tokens.Radius.avatar) else CircleShape
    Box(
        if (bytes != null) {
            Modifier.mediaSpot(key).clip(shape)
                .clickable(role = Role.Button, onClickLabel = stringResource(R.string.avatar_open)) { open = true }
        } else {
            Modifier
        },
    ) { avatar() }
    if (open && bytes != null) AvatarViewer(model, title, bytes, key) { open = false }
}

/**
 * Фото профиля во весь экран (п. 16, п. 30): тот же просмотрщик, что у фото из чата, — зум,
 * хром, закрытие протяжкой. Страница одна; в «⋮» — «Сохранить» и «Поделиться»: ответить или
 * переслать аватар нечем, это не сообщение.
 */
@Composable
internal fun AvatarViewer(model: AppModel, title: String, bytes: ByteArray, key: Any, onClose: () -> Unit) {
    val page = remember(bytes) {
        val mime = imageMime(bytes)
        ViewerPage(key, blobId = "", video = false, mime = mime, name = "avatar." + mime.substringAfter('/'))
    }
    val pager = rememberPagerState { 1 }
    ViewerFrame(
        model,
        listOf(page),
        pager,
        bytesOf = { bytes },
        title = title,
        sub = stringResource(R.string.viewer_avatar),
        onClose = onClose,
        menu = { vm -> SaveShareItems(vm) },
    )
}

/** Формат картинки по её байтам: у аватара нет имени файла, а галерее нужен тип. */
private fun imageMime(bytes: ByteArray): String {
    val o = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
    android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size, o)
    return o.outMimeType ?: "image/jpeg"
}
