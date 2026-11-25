package chat.peerage.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Download
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import chat.peerage.Album
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.Message
import chat.peerage.MsgRef
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Альбом, который рисует ряд сообщения (п. 69): лента кладёт его вокруг ПЕРВОГО сообщения
 * альбома, остальные его сообщения своих рядов не рисуют. Пусто — обычное сообщение.
 */
internal val LocalAlbum = compositionLocalOf<List<Message>> { emptyList() }

/**
 * Сообщения ряда [m]: все снимки альбома, если ряд его рисует, иначе само [m]. Ряд — единица
 * выбора: его кружок, тап в режиме выбора и протяжка берут альбом целиком.
 */
@Composable
internal fun rowMessages(m: Message): List<Message> =
    LocalAlbum.current.takeIf { a -> a.size >= 2 && a.first().let { it.id == m.id && it.sender == m.sender } } ?: listOf(m)

/**
 * Сетка альбома шириной [width]: ряды по [Album.rows], одиночный ряд — шире, чем высок.
 * У каждой ячейки своё: тап — просмотрщик с этого снимка ([onOpen]), в выборе — выбор этого
 * сообщения; долгое нажатие — выбор этого сообщения; ещё не скачанное — кнопкой загрузки.
 */
@Composable
internal fun AlbumGrid(model: AppModel, items: List<Message>, width: Dp, group: Boolean, onOpen: (Message) -> Unit) {
    val gap = Tokens.Space.xxs
    val rows = Album.rows(items.size)
    val starts = rows.runningFold(0) { acc, n -> acc + n }
    Column(Modifier.width(width), verticalArrangement = Arrangement.spacedBy(gap)) {
        rows.forEachIndexed { r, n ->
            val cellW = (width - gap * (n - 1)) / n
            val cellH = if (n == 1) width * 0.66f else cellW
            Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                items.subList(starts[r], starts[r] + n).forEach { m ->
                    AlbumCell(model, m, DpSize(cellW, cellH), group, onOpen)
                }
            }
        }
    }
}

@Composable
private fun AlbumCell(model: AppModel, m: Message, size: DpSize, group: Boolean, onOpen: (Message) -> Unit) {
    val ref = MsgRef(m.id, m.sender)
    val selecting = model.selection.isNotEmpty()
    val open = { if (selecting) model.toggleSelect(m) else onOpen(m) }
    val pick = { if (m.selectable) model.toggleSelect(m) }
    val bytes = if (m.kind == "image") model.blob(m.blobId) else null
    val ready = m.downloaded || model.savedBlobs[m.blobId] == true
    Box(Modifier.size(size).mediaSpot(ref)) {
        when {
            m.kind == "image" && (m.downloaded || bytes != null) ->
                ImageAttachment(bytes, size, onOpen = open, bare = true, onLongClick = pick, cacheKey = m.blobId)
            m.kind == "video" && ready ->
                VideoAttachment(model.blob(m.thumbId), size, true, m.durationMs, onOpen = open, bare = true, onLongClick = pick, cacheKey = m.thumbId)
            else -> PendingCell(model, m, size, group, onLong = pick)
        }
        if (selecting) {
            Box(Modifier.align(Alignment.TopEnd).padding(Tokens.Space.xs)) { SelectCheck(ref in model.selection) }
        }
    }
}

/** Ещё не скачанное в альбоме: миниатюра (если прислали) и посередине — загрузка или её ход. */
@Composable
private fun PendingCell(model: AppModel, m: Message, size: DpSize, group: Boolean, onLong: () -> Unit) {
    val thumb = rememberBitmap(model.blob(m.thumbId), m.thumbId, maxOf(size.width, size.height))
    val progress = model.downloads[m.blobId]
    val download = {
        // Просим у автора: в личном чате это собеседник, в группе — тот, кто прислал.
        val from = if (group) m.sender else model.currentChat
        val chat = if (group) model.currentGroup?.let { ChatTarget(it, true) } else model.currentChat?.let { ChatTarget(it, false) }
        from?.let { model.downloadMedia(it, m.blobId, m.name, m.size, chat) }
        Unit
    }
    Box(
        Modifier.size(size).background(Tokens.Colors.thumbScrim)
            .combinedClickable(
                onClickLabel = stringResource(R.string.chat_download),
                onClick = {
                    if (model.selection.isNotEmpty()) {
                        model.toggleSelect(m)
                    } else if (progress == null) {
                        download()
                    }
                },
                onLongClick = onLong,
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (thumb != null) Image(thumb, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        Box(
            Modifier.size(Tokens.Size.replyBadge).clip(CircleShape).background(Tokens.Colors.playScrim),
            contentAlignment = Alignment.Center,
        ) {
            when {
                progress == AppModel.DOWNLOAD_INIT ->
                    CircularProgressIndicator(color = Color.White, modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
                progress != null ->
                    CircularProgressIndicator(progress = progress, color = Color.White, modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
                else -> Icon(Icons.Filled.Download, null, tint = Color.White, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }
    }
}
