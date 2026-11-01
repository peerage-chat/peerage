@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package chat.peerage.ui

import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.provider.MediaStore
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Message
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.VolumeOff
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import chat.peerage.AppModel
import chat.peerage.HoloAccent
import chat.peerage.HoloError
import chat.peerage.HoloSecondary
import chat.peerage.R
import chat.peerage.Tokens
import kotlin.math.abs

/** Одно вложение в просмотрщике. Плоская запись, а не сообщение чата: просмотрщик
 *  одинаково открывается из личного чата, группы и «Сохранённого». */
internal data class ViewerItem(
    val id: Long,
    val sender: String,
    val author: String,
    val time: Long,
    val mime: String,
    val name: String,
    val blobId: String,
)

/**
 * Полноэкранный просмотр вложения — фото и видео.
 *
 * Экран сознательно «голый»: чёрный фон во весь экран, никаких карточек и
 * поверхностей. Здесь единственный предмет — сам снимок, и всё, что не он,
 * соревнуется с ним за внимание. Поэтому хром (шапка и таймлайн) прячется по
 * тапу и сам уходит через пять секунд после последнего касания.
 *
 * Жесты: щипок — зум, двойной тап у фото — 1× ↔ 2.5×, протяжка вниз — закрыть,
 * протяжка вбок — соседнее вложение чата. Пока снимок увеличен, протяжка двигает
 * его, а не листает и не закрывает: иначе рассмотреть угол было бы невозможно.
 *
 * Список [items] — то, что уже загружено в ленту чата; полная история медиа
 * потребует запроса в движок (задача #45), и тогда счётчик «N из M» будет
 * считать по всему чату, а не по подгруженному куску.
 */
@Composable
internal fun MediaViewer(
    model: AppModel,
    items: List<ViewerItem>,
    startIndex: Int,
    onClose: () -> Unit,
    onForward: (ViewerItem) -> Unit,
    onDelete: (ViewerItem) -> Unit,
    onShowInChat: (ViewerItem) -> Unit,
) {
    if (items.isEmpty()) return
    val ctx = LocalContext.current
    val storagePerm = rememberPermissionRunner()
    val pager = rememberPagerState(initialPage = startIndex.coerceIn(0, items.lastIndex)) { items.size }
    val item = items[pager.currentPage.coerceIn(0, items.lastIndex)]
    val video = item.mime.startsWith("video/")
    // Видео в память не читаем: оно проигрывается и отдаётся файлом (см. [Content]).
    val bytes = if (video) null else model.blob(item.blobId)

    var chrome by remember { mutableStateOf(true) }
    var menu by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    var muted by remember { mutableStateOf(false) }
    // Смещение «оттягивания» вниз. Фон гаснет вместе с ним — жест виден до того,
    // как отпустишь палец, и понятно, что произойдёт.
    var dragY by remember { mutableFloatStateOf(0f) }
    // Пороги — в dp, а не в пикселях: в пикселях жест на плотном экране требовал
    // протянуть вдвое дальше, чем на простом.
    val density = androidx.compose.ui.platform.LocalDensity.current
    val closeAt = with(density) { CloseDrag.toPx() }
    val fadeOver = with(density) { CloseFade.toPx() }
    val dim = (1f - (abs(dragY) / fadeOver)).coerceIn(0.35f, 1f)

    // Хром уходит сам: пока он на экране, он закрывает часть кадра. Отсчёт
    // перезапускается на каждое переключение и на смену страницы; открытое меню
    // держит его — иначе пункты исчезали бы из-под пальца.
    LaunchedEffect(chrome, pager.currentPage, menu) {
        if (chrome && !menu) {
            kotlinx.coroutines.delay(5000)
            chrome = false
        }
    }

    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false)) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = dim)),
            contentAlignment = Alignment.Center,
        ) {
            HorizontalPager(pager, Modifier.fillMaxSize()) { page ->
                val it = items[page]
                MediaPage(
                    model,
                    it,
                    if (it.mime.startsWith("video/")) null else model.blob(it.blobId),
                    muted,
                    chrome,
                    onChrome = { chrome = !chrome },
                    onDrag = { dy ->
                        dragY += dy
                        if (dragY > closeAt) onClose()
                    },
                    onDragEnd = { dragY = 0f },
                )
            }

            // Шапка поверх снимка, а не над ним: отдельная полоса откусывала бы
            // высоту у самого изображения ради подписи, которую и так знают.
            AnimatedVisibility(
                chrome,
                Modifier.align(Alignment.TopCenter),
                enter = fadeIn() + slideInVertically { -it },
                exit = fadeOut() + slideOutVertically { -it },
            ) {
                Row(
                    Modifier.fillMaxWidth()
                        .background(Tokens.Colors.mediaScrim)
                        .statusBarsPadding()
                        .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    BarIcon(Icons.Filled.Close, stringResource(R.string.common_back), onClick = onClose)
                    Spacer(Modifier.width(Tokens.Space.s))
                    Column(Modifier.weight(1f)) {
                        Text(
                            item.author,
                            color = Color.White,
                            fontSize = Tokens.Font.label,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        // Счётчик — во второй строке, а не третьим элементом ряда:
                        // ряд из трёх смысловых блоков читается как панель, а не
                        // как подпись к снимку.
                        Text(
                            if (items.size > 1) {
                                stringResource(
                                    R.string.viewer_of,
                                    fmtViewerDate(item.time),
                                    pager.currentPage + 1,
                                    items.size,
                                )
                            } else {
                                fmtViewerDate(item.time)
                            },
                            color = HoloSecondary,
                            fontSize = Tokens.Font.small,
                        )
                    }
                    Box {
                        BarIcon(Icons.Filled.MoreVert, stringResource(R.string.common_more)) { menu = true }
                        PopMenu(menu, onDismiss = { menu = false }) {
                            ViewerMenuItem(Icons.Filled.Download, R.string.viewer_save) {
                                menu = false
                                withContent(model, item, bytes) { saveWithPermission(ctx, storagePerm, it, item) }
                            }
                            ViewerMenuItem(Icons.AutoMirrored.Filled.Send, R.string.forward_action) {
                                menu = false
                                onForward(item)
                            }
                            ViewerMenuItem(Icons.Filled.Share, R.string.viewer_share) {
                                menu = false
                                withContent(model, item, bytes) { shareFile(ctx, it, item.mime, item.name) }
                            }
                            ViewerMenuItem(Icons.AutoMirrored.Filled.Message, R.string.viewer_show_in_chat) {
                                menu = false
                                onShowInChat(item)
                            }
                            PopMenuDivider()
                            ViewerMenuItem(Icons.Filled.Delete, R.string.viewer_delete, danger = true) {
                                menu = false
                                confirmDelete = true
                            }
                        }
                    }
                }
            }

            // Таймлайн прячется вместе с шапкой: это один слой управления,
            // разнесённый по краям кадра.
            if (video) {
                AnimatedVisibility(
                    chrome,
                    Modifier.align(Alignment.BottomCenter),
                    enter = fadeIn() + slideInVertically { it },
                    exit = fadeOut() + slideOutVertically { it },
                ) {
                    VideoBar(muted) { muted = it }
                }
            }
        }
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(R.string.viewer_delete)) },
            text = { Text(stringResource(R.string.viewer_delete_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    onDelete(item)
                    onClose()
                }) { Text(stringResource(R.string.folder_delete_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirmDelete = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}

/** Протяжка вниз, после которой просмотрщик закрывается. */
private val CloseDrag = 80.dp

/** На какой длине протяжки фон гаснет до предела. */
private val CloseFade = 320.dp

/** До Android 10 запись в галерею требует разрешения; начиная с неё MediaStore
 *  кладёт файл в свою папку и без него. */
private fun saveWithPermission(
    ctx: Context,
    ask: (Array<String>, Int, () -> Unit) -> Unit,
    content: Content,
    item: ViewerItem,
) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        saveToGallery(ctx, content, item.mime, item.name)
    } else {
        ask(
            arrayOf(android.Manifest.permission.WRITE_EXTERNAL_STORAGE),
            R.string.perm_denied_storage,
        ) { saveToGallery(ctx, content, item.mime, item.name) }
    }
}

/** Содержимое вложения для «сохранить» и «поделиться»: фото уже в памяти, видео —
 *  файлом, расшифрованным потоково. */
private sealed interface Content {
    class Bytes(val bytes: ByteArray) : Content
    class OnDisk(val file: java.io.File) : Content

    fun writeTo(out: java.io.OutputStream) = when (this) {
        is Bytes -> out.write(bytes)
        is OnDisk -> file.inputStream().use { it.copyTo(out) }
    }
}

/** Достать содержимое вложения и отдать его `use` на главном потоке. */
private fun withContent(model: AppModel, item: ViewerItem, bytes: ByteArray?, use: (Content) -> Unit) {
    if (!item.mime.startsWith("video/")) {
        bytes?.let { use(Content.Bytes(it)) }
        return
    }
    model.exportBlob(item.blobId, "shared", item.name.ifBlank { defaultName(true) }) { f ->
        f?.let { use(Content.OnDisk(it)) }
    }
}

@Composable
private fun ViewerMenuItem(icon: ImageVector, labelRes: Int, danger: Boolean = false, onClick: () -> Unit) {
    PopMenuItem(icon, stringResource(labelRes), danger = danger, onClick = onClick)
}

/** Одна страница пейджера: снимок с зумом или видео. */
@Composable
private fun MediaPage(
    model: AppModel,
    item: ViewerItem,
    bytes: ByteArray?,
    muted: Boolean,
    chrome: Boolean,
    onChrome: () -> Unit,
    onDrag: (Float) -> Unit,
    onDragEnd: () -> Unit,
) {
    var scale by remember(item.blobId) { mutableFloatStateOf(1f) }
    var offX by remember(item.blobId) { mutableFloatStateOf(0f) }
    var offY by remember(item.blobId) { mutableFloatStateOf(0f) }
    var box by remember { mutableStateOf(IntSize.Zero) }
    val zoom by animateFloatAsState(scale, label = "zoom")
    val video = item.mime.startsWith("video/")

    // Дальше своих краёв снимок не уезжает: без этого его можно утащить за экран
    // и потерять из виду, а вернуть — только сбросом зума.
    fun clamp(off: Float, side: Int): Float {
        val limit = (side * (scale - 1f) / 2f).coerceAtLeast(0f)
        return off.coerceIn(-limit, limit)
    }

    // Щипок обрабатываем сами, а не через `detectTransformGestures`: тот съедает
    // и одиночную протяжку, а её ждут пейджер (соседнее вложение) и жест закрытия.
    //
    // Потребляем ТОЛЬКО реальное движение — щипок или протяжку по увеличенному
    // снимку. Раньше на увеличенном потреблялось любое касание, и распознаватель
    // тапов переставал их получать (основная фаза идёт от последнего модификатора
    // к первому): из зума нельзя было выйти двойным тапом, а хром не возвращался.
    val zoomable = Modifier.pointerInput(item.blobId) {
        awaitEachGesture {
            awaitFirstDown(requireUnconsumed = false)
            var moved = false
            var event: PointerEvent
            do {
                event = awaitPointerEvent()
                val multi = event.changes.count { it.pressed } > 1
                val pan = event.calculatePan()
                if (multi) {
                    moved = true
                    scale = (scale * event.calculateZoom()).coerceIn(1f, 6f)
                } else if (scale > 1f && !moved && pan.getDistance() > viewConfiguration.touchSlop) {
                    moved = true
                }
                if (moved) {
                    if (scale > 1f) {
                        offX = clamp(offX + pan.x, box.width)
                        offY = clamp(offY + pan.y, box.height)
                    } else {
                        offX = 0f
                        offY = 0f
                    }
                    event.changes.forEach { it.consume() }
                }
            } while (event.changes.any { it.pressed })
        }
    }

    val taps = Modifier.pointerInput(item.blobId, video) {
        detectTapGestures(
            onTap = { onChrome() },
            onDoubleTap = {
                // У видео двойной тап не занят зумом: кадр и так вписан в экран.
                if (!video) {
                    if (scale > 1f) {
                        scale = 1f
                        offX = 0f
                        offY = 0f
                    } else {
                        scale = 2.5f
                    }
                }
            },
        )
    }

    // Закрытие протяжкой вниз — только на единичном масштабе: увеличенный снимок
    // двигают, а не закрывают.
    val closeDrag = Modifier.pointerInput(item.blobId) {
        detectVerticalDragGestures(
            onDragEnd = { onDragEnd() },
            onDragCancel = { onDragEnd() },
        ) { _, dy -> if (scale <= 1f) onDrag(dy) }
    }

    Box(
        Modifier.fillMaxSize()
            .onSizeChanged { box = it }
            .then(taps)
            .then(zoomable)
            .then(closeDrag),
        contentAlignment = Alignment.Center,
    ) {
        when {
            video -> VideoSurface(model, item, muted, chrome)
            bytes == null -> Text(
                stringResource(R.string.chat_downloading),
                color = HoloSecondary,
                fontSize = Tokens.Font.body,
            )
            else -> {
                // Под размер экрана: зум до 6× работает по этому же битмапу —
                // держать ради него оригинал на 48 МБ не за что.
                val bmp = rememberFullscreenBitmap(bytes, item.blobId)
                if (bmp != null) {
                    Image(
                        bmp,
                        null,
                        Modifier.fillMaxSize().graphicsLayer {
                            scaleX = zoom
                            scaleY = zoom
                            translationX = offX
                            translationY = offY
                        },
                        contentScale = ContentScale.Fit,
                    )
                }
            }
        }
    }
}

/** Плеер один на весь просмотрщик: таймлайн лежит в другой ветке дерева, а
 *  управляет тем же воспроизведением. */
private object Player {
    var view: android.widget.VideoView? = null
    var media: android.media.MediaPlayer? = null
}

/**
 * Видео проигрывается ВНУТРИ приложения, а не отдаётся системному плееру: файл
 * из чата — чужая переписка, и выпускать его в сторонние приложения ради кнопки
 * «play» не за что. Байты кладём в приватный кэш, откуда их никто не читает.
 */
@Composable
private fun VideoSurface(model: AppModel, item: ViewerItem, muted: Boolean, chrome: Boolean) {
    // Видео расшифровывается в приватный кэш потоково, по чанку: в память целиком
    // оно не поднимается. Копия живёт, пока открыт просмотр.
    var file by remember(item.blobId) { mutableStateOf<java.io.File?>(null) }
    LaunchedEffect(item.blobId) {
        model.exportBlob(item.blobId, "viewer", "${item.blobId.take(16)}-${item.name}") { file = it }
    }
    var mine by remember(item.blobId) { mutableStateOf<android.widget.VideoView?>(null) }
    DisposableEffect(file) {
        // Захватить файл ЭТОГО эффекта: при смене null→файл onDispose старого эффекта
        // читал бы уже новый и удалял его до того, как плеер успеет открыть.
        val own = file
        onDispose {
            // Останавливаем СВОЙ плеер, а общий слот чистим, только если он ещё наш:
            // при листании между видео уходящая страница освобождается позже, чем
            // встаёт новая, и раньше гасила уже чужое воспроизведение.
            mine?.stopPlayback()
            if (Player.view === mine) {
                Player.view = null
                Player.media = null
            }
            own?.delete()
        }
    }
    LaunchedEffect(muted) {
        val v = if (muted) 0f else 1f
        Player.media?.setVolume(v, v)
    }
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        val f = file
        if (f == null) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(36.dp), strokeWidth = 2.dp)
        } else {
            AndroidView(
                factory = { c ->
                    android.widget.VideoView(c).apply {
                        setVideoPath(f.absolutePath)
                        setOnPreparedListener { mp ->
                            // Зациклено: короткое видео из чата пересматривают, и
                            // жать «заново» после каждого прохода — работа на
                            // пустом месте.
                            mp.isLooping = true
                            if (Player.view === this) Player.media = mp
                            val v = if (muted) 0f else 1f
                            mp.setVolume(v, v)
                        }
                        start()
                        Player.view = this
                        mine = this
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
        }
        // Значок паузы по центру — только когда хром скрыт: с таймлайном на
        // экране состояние и так видно по его кнопке.
        if (!chrome && Player.view?.isPlaying == false) {
            Box(
                Modifier.size(72.dp).clip(CircleShape).background(Tokens.Colors.playScrim),
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.PlayArrow, null, tint = Color.White, modifier = Modifier.size(Tokens.Size.iconHero))
            }
        }
    }
}

/** Нижняя панель видео: пуск/пауза · полоса с перемоткой · время · звук. */
@Composable
private fun VideoBar(muted: Boolean, onMute: (Boolean) -> Unit) {
    var position by remember { mutableIntStateOf(0) }
    var duration by remember { mutableIntStateOf(0) }
    var playing by remember { mutableStateOf(true) }
    var width by remember { mutableIntStateOf(1) }
    // Положение читаем опросом: VideoView не сообщает о нём сам, а четыре раза в
    // секунду достаточно, чтобы полоса не дёргалась и не грела процессор.
    LaunchedEffect(Unit) {
        while (true) {
            Player.view?.let {
                position = it.currentPosition
                duration = it.duration.coerceAtLeast(0)
                playing = it.isPlaying
            }
            kotlinx.coroutines.delay(250)
        }
    }
    val seek = { x: Float ->
        if (duration > 0) {
            val ms = (x / width * duration).toInt().coerceIn(0, duration)
            Player.view?.seekTo(ms)
            position = ms
        }
    }
    Row(
        Modifier.fillMaxWidth()
            .background(Tokens.Colors.mediaScrim)
            .navigationBarsPadding()
            .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow,
            stringResource(if (playing) R.string.viewer_pause else R.string.viewer_play),
            tint = Color.White,
            modifier = Modifier.size(Tokens.Size.iconL).pointerInput(Unit) {
                detectTapGestures {
                    val v = Player.view ?: return@detectTapGestures
                    if (v.isPlaying) v.pause() else v.start()
                    playing = v.isPlaying
                }
            },
        )
        Spacer(Modifier.width(Tokens.Space.m))
        // Область касания намеренно выше самой линии: попадать пальцем в три
        // пикселя невозможно.
        Box(
            Modifier.weight(1f)
                .height(24.dp)
                .onSizeChanged { width = it.width.coerceAtLeast(1) }
                .pointerInput(duration) { detectTapGestures { p -> seek(p.x) } }
                .pointerInput(duration) {
                    detectHorizontalDragGestures { change, _ -> seek(change.position.x) }
                },
            contentAlignment = Alignment.CenterStart,
        ) {
            Box(Modifier.fillMaxWidth().height(3.dp).background(Tokens.Colors.trackOnMedia))
            if (duration > 0) {
                val done = (position.toFloat() / duration).coerceIn(0f, 1f)
                Box(Modifier.fillMaxWidth(done).height(3.dp).background(HoloAccent))
            }
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Text(
            if (duration > 0) {
                stringResource(R.string.viewer_progress, fmtDuration(position), fmtDuration(duration))
            } else {
                fmtDuration(position)
            },
            color = Tokens.Colors.quoteTextOnBubble,
            fontSize = Tokens.Font.caption,
            maxLines = 1,
        )
        Spacer(Modifier.width(Tokens.Space.s))
        Icon(
            if (muted) Icons.Filled.VolumeOff else Icons.Filled.VolumeUp,
            stringResource(if (muted) R.string.viewer_unmute else R.string.viewer_mute),
            tint = Color.White,
            modifier = Modifier.size(Tokens.Size.iconM).pointerInput(Unit) {
                detectTapGestures { onMute(!muted) }
            },
        )
    }
}

/** Дата снимка словами: «сегодня в 14:03» читается быстрее, чем «10.08 14:03». */
@Composable
private fun fmtViewerDate(time: Long): String {
    val ms = time * 1000
    val ctx = LocalContext.current
    val clock = android.text.format.DateFormat.getTimeFormat(ctx).format(java.util.Date(ms))
    return if (android.text.format.DateUtils.isToday(ms)) {
        stringResource(R.string.viewer_today, clock)
    } else {
        val day = android.text.format.DateFormat.getMediumDateFormat(ctx).format(java.util.Date(ms))
        stringResource(R.string.viewer_on_date, day, clock)
    }
}

/** Сохранить вложение в галерею — в свою папку приложения, как это делают камеры
 *  и мессенджеры: иначе чужие снимки смешиваются с собственными. */
private fun saveToGallery(ctx: Context, content: Content, mime: String, name: String) {
    val video = mime.startsWith("video/")
    val ok = runCatching {
        val target = if (video) {
            MediaStore.Video.Media.EXTERNAL_CONTENT_URI
        } else {
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name.ifBlank { defaultName(video) })
            put(MediaStore.MediaColumns.MIME_TYPE, mime.ifBlank { if (video) "video/mp4" else "image/jpeg" })
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val dir = if (video) {
                    android.os.Environment.DIRECTORY_MOVIES
                } else {
                    android.os.Environment.DIRECTORY_PICTURES
                }
                put(MediaStore.MediaColumns.RELATIVE_PATH, "$dir/${ctx.getString(R.string.app_name)}")
            }
        }
        val uri = ctx.contentResolver.insert(target, values) ?: return@runCatching false
        ctx.contentResolver.openOutputStream(uri)?.use { content.writeTo(it) } ?: return@runCatching false
        true
    }.getOrDefault(false)
    android.widget.Toast.makeText(
        ctx,
        ctx.getString(if (ok) R.string.viewer_saved else R.string.viewer_save_failed),
        android.widget.Toast.LENGTH_SHORT,
    ).show()
}

private fun defaultName(video: Boolean): String {
    val stamp = java.text.SimpleDateFormat("yyyyMMdd-HHmmss", java.util.Locale.US).format(java.util.Date())
    return if (video) "video-$stamp.mp4" else "photo-$stamp.jpg"
}

/** Отдать вложение наружу системным листом (файл живёт в приватном кэше). */
private fun shareFile(ctx: Context, content: Content, mime: String, name: String) {
    runCatching {
        val f = when (content) {
            is Content.OnDisk -> content.file
            is Content.Bytes -> {
                val dir = java.io.File(ctx.cacheDir, "shared").apply { mkdirs() }
                // Имя прислал собеседник: «../» вывело бы запись из кэша.
                java.io.File(dir, java.io.File(name).name.ifBlank { defaultName(mime.startsWith("video/")) })
                    .apply { writeBytes(content.bytes) }
            }
        }
        val uri = androidx.core.content.FileProvider.getUriForFile(ctx, "${ctx.packageName}.fileprovider", f)
        val send = android.content.Intent(android.content.Intent.ACTION_SEND)
            .setType(mime.ifBlank { "*/*" })
            .putExtra(android.content.Intent.EXTRA_STREAM, uri)
            .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION)
        ctx.startActivity(
            android.content.Intent.createChooser(send, ctx.getString(R.string.viewer_share))
                .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }
}
