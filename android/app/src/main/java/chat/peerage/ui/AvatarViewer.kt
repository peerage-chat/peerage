package chat.peerage.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Аватар профиля, который по тапу открывается во весь экран ([AvatarViewer]). Без картинки —
 * обычный аватар: букву разглядывать незачем.
 */
@Composable
internal fun OpenableAvatar(bytes: ByteArray?, square: Boolean = false, avatar: @Composable () -> Unit) {
    var open by remember { mutableStateOf(false) }
    // Отклик нажатия — по форме аватара: круг у человека, скруглённый квадрат у группы.
    val shape = if (square) androidx.compose.foundation.shape.RoundedCornerShape(Tokens.Radius.avatar) else CircleShape
    Box(
        if (bytes != null) {
            Modifier.clip(shape).clickable(role = Role.Button, onClickLabel = stringResource(R.string.avatar_open)) { open = true }
        } else {
            Modifier
        },
    ) { avatar() }
    if (open && bytes != null) AvatarViewer(bytes) { open = false }
}

/** Предел щипка: аватар маленький, дальше видны только пиксели. */
private const val MAX_ZOOM = 4f

/** Двойной тап приближает до стольких. */
private const val TAP_ZOOM = 2.5f

/**
 * Аватар во весь экран (п. 16 с телефона): тап по аватару в профиле — своём, контакта,
 * группы. Как просмотр фото, только без ленты: чёрный фон, щипок и двойной тап — зум,
 * тап по фону, «назад» или крестик — закрыть. Буква-заглушка не открывается — смотреть
 * в ней нечего, поэтому зовут, только когда картинка есть.
 */
@Composable
internal fun AvatarViewer(bytes: ByteArray, onClose: () -> Unit) {
    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val bmp = rememberBitmap(bytes, null, 1200.dp)
        var scale by remember { mutableFloatStateOf(1f) }
        var off by remember { mutableFloatStateOf(0f) }
        var offY by remember { mutableFloatStateOf(0f) }
        val zoom by animateFloatAsState(scale, label = "avatarZoom")
        Box(
            Modifier.fillMaxSize().background(Color.Black)
                .pointerInput(Unit) {
                    detectTapGestures(
                        onTap = { onClose() },
                        onDoubleTap = {
                            scale = if (scale > 1f) 1f else TAP_ZOOM
                            off = 0f
                            offY = 0f
                        },
                    )
                }
                .pointerInput(Unit) {
                    detectTransformGestures { _, pan: Offset, gesture, _ ->
                        scale = (scale * gesture).coerceIn(1f, MAX_ZOOM)
                        if (scale > 1f) {
                            off += pan.x
                            offY += pan.y
                        } else {
                            off = 0f
                            offY = 0f
                        }
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            if (bmp != null) {
                Image(
                    bmp,
                    null,
                    Modifier.fillMaxSize().graphicsLayer {
                        scaleX = zoom
                        scaleY = zoom
                        translationX = off
                        translationY = offY
                    },
                    contentScale = ContentScale.Fit,
                )
            }
            Box(
                Modifier.align(Alignment.TopStart).statusBarsPadding().padding(Tokens.Space.s)
                    .size(Tokens.Size.touch).clip(CircleShape)
                    .clickable(role = Role.Button, onClick = onClose),
                contentAlignment = Alignment.Center,
            ) {
                Icon(TablerIcons.X, stringResource(R.string.avatar_close), tint = Color.White, modifier = Modifier.size(Tokens.Size.iconM))
            }
        }
    }
}
