@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package chat.peerage.ui

import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.provider.MediaStore
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.HoloAccent
import chat.peerage.HoloSecondary
import chat.peerage.Message
import chat.peerage.MsgRef
import chat.peerage.R
import chat.peerage.Tokens
import chat.peerage.viewerMedia
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.min

/** Можно ли показать сообщение в просмотрщике: фото или видео, и оно уже здесь. */
private fun viewable(model: AppModel, m: Message) =
    (m.kind == "image" || m.kind == "video") && m.blobId.isNotEmpty() &&
        (m.downloaded || model.savedBlobs[m.blobId] == true)

private fun Message.isVideo() = kind == "video" || mime.startsWith("video/")

private fun Message.ref() = MsgRef(id, sender)

/**
 * Где на экране лежат снимки ленты — туда просмотрщик уводит снимок, закрываясь
 * протяжкой (app-ui.md → «Просмотр»). Экранные координаты: просмотрщик живёт в своём
 * окне, и общая для двух окон система отсчёта — только экран.
 */
internal object MediaSpots {
    private val spots = HashMap<Any, Rect>()

    fun put(key: Any, r: Rect) {
        spots[key] = r
    }

    fun remove(key: Any) {
        spots.remove(key)
    }

    fun get(key: Any): Rect? = spots[key]?.takeIf { it.width > 0f && it.height > 0f }
}

/** Отметить снимок ленты (или аватар) как место возврата просмотрщика; [ref] — ключ страницы. */
internal fun Modifier.mediaSpot(ref: Any): Modifier = composed {
    val view = LocalView.current
    DisposableEffect(ref) { onDispose { MediaSpots.remove(ref) } }
    onGloballyPositioned {
        val at = IntArray(2).also { a -> view.getLocationOnScreen(a) }
        MediaSpots.put(ref, it.boundsInWindow().translate(at[0].toFloat(), at[1].toFloat()))
    }
}

/**
 * Закрытие протяжкой: пока палец тянет, снимок едет за ним и уменьшается, фон гаснет и
 * сквозь него виден чат; отпустили дальше порога — снимок улетает в свой пузырь (или
 * гаснет, если пузыря на экране нет), не дотянули — возвращается.
 */
private class CloseMotion {
    var dragY by mutableFloatStateOf(0f)
    val fly = Animatable(0f)

    /** Куда лететь, в координатах окна просмотрщика; null — пузыря на экране нет. */
    var target by mutableStateOf<Rect?>(null)

    /** Где снимок текущей страницы при единичном масштабе. */
    var image by mutableStateOf(Rect.Zero)
    var height = 1f

    /** 0..1: насколько протянули — от этого уменьшение, скругление и фон. */
    fun pulled(closeAt: Float) = (dragY / closeAt).coerceIn(0f, 1f)

    fun dragScale() = 1f - (dragY.coerceAtLeast(0f) / height).coerceIn(0f, 1f) * DRAG_SHRINK
}

/** На сколько снимок уменьшается, если протянуть на всю высоту экрана. */
private const val DRAG_SHRINK = 0.5f

/**
 * Страница просмотрщика: что показать и откуда взять. Один вид на вложение чата и на фото
 * профиля — просмотрщик у них один (п. 30).
 */
internal class ViewerPage(
    /** Ключ страницы и место возврата в [MediaSpots]: ссылка на сообщение или ключ аватара. */
    val key: Any,
    val blobId: String,
    val video: Boolean,
    val mime: String,
    val name: String,
    val caption: String = "",
    /** Снимок из галереи телефона (выбор вложения): видео играет прямо с него, без копии. */
    val uri: android.net.Uri? = null,
)

private fun Message.page() = ViewerPage(ref(), blobId, isVideo(), mime, name, text.trim())

/** Меню просмотрщика: закрыть его и общие для любого снимка «Сохранить» / «Поделиться». */
internal class ViewerMenuScope(val dismiss: () -> Unit, val save: () -> Unit, val share: () -> Unit)

/** «Сохранить» и «Поделиться» — есть у любого снимка. */
@Composable
internal fun SaveShareItems(vm: ViewerMenuScope) {
    ViewerMenuItem(TablerIcons.Download, R.string.viewer_save) {
        vm.dismiss()
        vm.save()
    }
    ViewerMenuItem(TablerIcons.Share, R.string.viewer_share) {
        vm.dismiss()
        vm.share()
    }
}

/**
 * Полноэкранный просмотр фото и видео чата (app-ui.md → «Просмотр», макеты `viewer/`):
 * каркас [ViewerFrame] и меню действий над сообщением.
 *
 * Листается ВСЁ медиа чата (запрос к движку), а не подгруженный кусок ленты: [seed] —
 * то, что есть сразу, пока список едет, и «N из M» считает по чату.
 */
@Composable
internal fun MediaViewer(
    model: AppModel,
    chat: ChatTarget,
    seed: List<Message>,
    start: MsgRef,
    onClose: () -> Unit,
    onReply: ((Message) -> Unit)?,
    onShowInChat: (Message) -> Unit,
    onDeleted: (MsgRef) -> Unit = {},
) {
    var list by remember { mutableStateOf(seed.filter { viewable(model, it) }) }
    if (list.isEmpty()) return
    val pager = rememberPagerState(list.indexOfFirst { it.ref() == start }.coerceAtLeast(0)) { list.size }
    val m = list[pager.currentPage.coerceIn(0, list.lastIndex)]
    val pages = remember(list) { list.map { it.page() } }
    var confirmDelete by remember { mutableStateOf(false) }

    // Полный список медиа чата подменяет затравку на той же странице: снимок под
    // пальцем не меняется, меняется только «N из M».
    LaunchedEffect(chat) {
        model.viewerMedia(chat) { full ->
            val shown = full.filter { viewable(model, it) }
            val cur = list.getOrNull(pager.currentPage)?.ref()
            val at = shown.indexOfFirst { it.ref() == cur }
            if (at >= 0) {
                list = shown
                pager.requestScrollToPage(at)
            }
        }
    }

    ViewerFrame(
        model,
        pages,
        pager,
        bytesOf = { p -> if (p.video) null else model.blob(p.blobId) },
        title = viewerAuthor(model, chat, m),
        sub = if (list.size > 1) {
            stringResource(R.string.viewer_of, fmtViewerDate(m.time), pager.currentPage + 1, list.size)
        } else {
            fmtViewerDate(m.time)
        },
        onClose = onClose,
        menu = { vm ->
            if (onReply != null) {
                ViewerMenuItem(TablerIcons.Reply, R.string.chat_reply) {
                    vm.dismiss()
                    onReply(m)
                    onClose()
                }
            }
            ViewerMenuItem(TablerIcons.Forward, R.string.forward_action) {
                vm.dismiss()
                // Пересылка — через выделение, как из панели чата: «переслать»
                // везде означает одно. Шторку рисует экран под просмотрщиком.
                model.selection = setOf(m.ref())
                model.beginForward(chat.id, chat.group)
                onClose()
            }
            SaveShareItems(vm)
            ViewerMenuItem(TablerIcons.Message, R.string.viewer_show_in_chat) {
                vm.dismiss()
                onShowInChat(m)
                onClose()
            }
            PopMenuDivider()
            ViewerMenuItem(TablerIcons.Trash, R.string.viewer_delete, danger = true) {
                vm.dismiss()
                confirmDelete = true
            }
        },
    ) {
        if (confirmDelete) {
            ConfirmSheet(
                title = stringResource(R.string.viewer_delete),
                text = stringResource(R.string.viewer_delete_confirm),
                ok = stringResource(R.string.folder_delete_ok),
                onOk = {
                    model.selection = setOf(m.ref())
                    model.deleteSelected(chat.id, chat.group)
                    onDeleted(m.ref())
                    onClose()
                },
                onClose = { confirmDelete = false },
            )
        }
    }
}

/**
 * Каркас просмотрщика — один на вложения чата и на фото профиля.
 *
 * Экран сознательно «голый»: чёрный фон, никаких поверхностей. Хром — шапка сверху,
 * подпись и таймлайн снизу — лежит на затемнении поверх снимка, прячется по тапу и сам
 * через пять секунд. Жесты: щипок — до 6×, двойной тап у фото — 1× ↔ 2.5×, вбок —
 * соседняя страница, вниз — снимок уходит туда, откуда открыт ([CloseMotion]).
 * [title] и [sub] — шапка текущей страницы, [menu] — пункты «⋮», [overlay] — шторки
 * поверх просмотра (в том же окне).
 */
@Composable
internal fun ViewerFrame(
    model: AppModel,
    pages: List<ViewerPage>,
    pager: androidx.compose.foundation.pager.PagerState,
    /** Байты снимка; composable — галерее их надо дочитать с диска, не на главном потоке. */
    bytesOf: @Composable (ViewerPage) -> ByteArray?,
    title: String,
    sub: String,
    onClose: () -> Unit,
    menu: @Composable ColumnScope.(ViewerMenuScope) -> Unit,
    overlay: @Composable () -> Unit = {},
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val storagePerm = rememberPermissionRunner()
    val p = pages[pager.currentPage.coerceIn(0, pages.lastIndex)]
    val video = p.video
    // Видео в память не читаем: оно проигрывается и отдаётся файлом (см. [Content]).
    val bytes = bytesOf(p)

    var chrome by remember { mutableStateOf(true) }
    var menuOpen by remember { mutableStateOf(false) }
    var muted by remember { mutableStateOf(false) }
    val motion = remember { CloseMotion() }
    // Пороги — в dp: в пикселях жест на плотном экране требовал протянуть вдвое дальше.
    val density = LocalDensity.current
    val closeAt = with(density) { CloseDrag.toPx() }
    val fadeOver = with(density) { CloseFade.toPx() }

    // Хром уходит сам: пока он на экране, он закрывает часть кадра. Отсчёт
    // перезапускается на каждое переключение и на смену страницы; открытое меню
    // держит его — иначе пункты исчезали бы из-под пальца.
    LaunchedEffect(chrome, pager.currentPage, menuOpen) {
        if (chrome && !menuOpen) {
            kotlinx.coroutines.delay(5000)
            chrome = false
        }
    }

    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false)) {
        val view = LocalView.current
        val release: () -> Unit = {
            if (motion.dragY > closeAt) {
                // Видео — SurfaceView, слой его не уменьшает: оно просто закрывается.
                if (video) {
                    onClose()
                } else {
                    motion.target = MediaSpots.get(p.key)?.let { r ->
                        val at = IntArray(2).also { view.getLocationOnScreen(it) }
                        r.translate(-at[0].toFloat(), -at[1].toFloat())
                    }
                    scope.launch {
                        motion.fly.animateTo(1f, tween(Tokens.Anim.normal))
                        onClose()
                    }
                }
            } else {
                scope.launch { animate(motion.dragY, 0f, animationSpec = tween(Tokens.Anim.fast)) { v, _ -> motion.dragY = v } }
            }
        }
        val dim = (1f - abs(motion.dragY) / fadeOver).coerceIn(CLOSE_DIM_MIN, 1f) * (1f - motion.fly.value)
        Box(
            Modifier.fillMaxSize()
                .onSizeChanged { motion.height = it.height.toFloat().coerceAtLeast(1f) }
                .background(Color.Black.copy(alpha = dim)),
            contentAlignment = Alignment.Center,
        ) {
            HorizontalPager(pager, Modifier.fillMaxSize(), userScrollEnabled = motion.dragY == 0f) { page ->
                val it = pages[page]
                MediaPage(
                    model,
                    it,
                    bytesOf(it),
                    muted,
                    chrome,
                    motion = if (page == pager.currentPage) motion else null,
                    closeAt = closeAt,
                    onChrome = { chrome = !chrome },
                    onDrag = { dy ->
                        if (motion.dragY == 0f && dy > 0f) chrome = false
                        motion.dragY = (motion.dragY + dy).coerceAtLeast(0f)
                    },
                    onDragEnd = release,
                )
            }

            val shown = chrome && motion.dragY == 0f && motion.fly.value == 0f
            // Шапка поверх снимка, а не над ним: отдельная полоса откусывала бы
            // высоту у самого изображения ради подписи, которую и так знают.
            AnimatedVisibility(shown, Modifier.align(Alignment.TopCenter), enter = fadeIn(), exit = fadeOut()) {
                ViewerTop(author = title, sub = sub, onClose = onClose, onMenu = { menuOpen = true }) {
                    PopMenu(menuOpen, onDismiss = { menuOpen = false }) {
                        menu(
                            ViewerMenuScope(
                                dismiss = { menuOpen = false },
                                save = { withContent(model, p, bytes) { saveWithPermission(ctx, storagePerm, it, p) } },
                                share = { withContent(model, p, bytes) { shareFile(ctx, it, p.mime, p.name) } },
                            ),
                        )
                    }
                }
            }

            // Подпись и таймлайн — один нижний слой: прячутся вместе с шапкой.
            if (p.caption.isNotEmpty() || video) {
                AnimatedVisibility(shown, Modifier.align(Alignment.BottomCenter), enter = fadeIn(), exit = fadeOut()) {
                    ViewerBottom(p.caption, video, muted) { muted = it }
                }
            }
        }
        overlay()
    }
}

/** Протяжка вниз, после которой просмотрщик закрывается. */
private val CloseDrag = 80.dp

/** На какой длине протяжки фон гаснет до предела. */
private val CloseFade = 320.dp

/** Сквозь фон на протяжке виден чат, но не ярче этого. */
private const val CLOSE_DIM_MIN = 0.35f

/** Автор вложения для шапки: «Вы», участник группы или собеседник. */
@Composable
private fun viewerAuthor(model: AppModel, chat: ChatTarget, m: Message): String = when {
    m.mine -> stringResource(R.string.chat_reply_you)
    chat.group -> groupSenderName(model, m.sender)
    else -> model.peerName(m.sender)
}

/** Шапка (макет `viewer/Main`): закрыть · автор и «дата · N из M» · «⋮» — на затемнении сверху. */
@Composable
private fun ViewerTop(author: String, sub: String, onClose: () -> Unit, onMenu: () -> Unit, menu: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .background(Brush.verticalGradient(listOf(Tokens.Colors.viewerShadeTop, Color.Transparent)))
            .statusBarsPadding()
            .padding(start = Tokens.Space.s, end = Tokens.Space.s, top = Tokens.Space.m + Tokens.Space.xxs, bottom = Tokens.Space.xxxl + Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
    ) {
        ViewerButton(TablerIcons.X, stringResource(R.string.common_back), onClick = onClose)
        Column(Modifier.weight(1f)) {
            Text(author, color = Color.White, fontSize = Tokens.Font.name, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(sub, color = Tokens.Colors.onMediaSoft, fontSize = Tokens.Font.small, style = Tokens.Text.tabular, maxLines = 1)
        }
        Box {
            ViewerButton(TablerIcons.DotsVertical, stringResource(R.string.common_more), onClick = onMenu)
            menu()
        }
    }
}

/** Кнопка хрома просмотрщика: значок без плашки — плашки спорили бы со снимком. */
@Composable
private fun ViewerButton(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.size(Tokens.Size.viewerButton).clip(RoundedCornerShape(Tokens.Radius.button)).clickable(onClickLabel = desc, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, desc, tint = Color.White, modifier = Modifier.size(Tokens.Size.iconRow))
    }
}

/** Низ (макеты `viewer/Main`, `viewer/Video`): подпись к снимку и таймлайн видео на затемнении. */
@Composable
private fun ViewerBottom(caption: String, video: Boolean, muted: Boolean, onMute: (Boolean) -> Unit) {
    Column(
        Modifier.fillMaxWidth()
            .background(Brush.verticalGradient(listOf(Color.Transparent, Tokens.Colors.viewerShadeBottom)))
            .navigationBarsPadding()
            .padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.hero, bottom = Tokens.Space.xl),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        if (caption.isNotEmpty()) Text(caption, color = Color.White, fontSize = Tokens.Font.label)
        if (video) VideoBar(muted, onMute)
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

/** До Android 10 запись в галерею требует разрешения; начиная с неё MediaStore
 *  кладёт файл в свою папку и без него. */
private fun saveWithPermission(
    ctx: Context,
    ask: (Array<String>, Int, () -> Unit) -> Unit,
    content: Content,
    p: ViewerPage,
) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        saveToGallery(ctx, content, p.mime, p.name)
    } else {
        ask(
            arrayOf(android.Manifest.permission.WRITE_EXTERNAL_STORAGE),
            R.string.perm_denied_storage,
        ) { saveToGallery(ctx, content, p.mime, p.name) }
    }
}

/** Достать содержимое страницы и отдать его `use` на главном потоке. */
private fun withContent(model: AppModel, p: ViewerPage, bytes: ByteArray?, use: (Content) -> Unit) {
    if (!p.video) {
        bytes?.let { use(Content.Bytes(it)) }
        return
    }
    model.exportBlob(p.blobId, "shared", p.name.ifBlank { defaultName(true) }) { f ->
        f?.let { use(Content.OnDisk(it)) }
    }
}

@Composable
private fun ViewerMenuItem(icon: ImageVector, labelRes: Int, danger: Boolean = false, onClick: () -> Unit) {
    PopMenuItem(icon, stringResource(labelRes), danger = danger, onClick = onClick)
}

/** Одна страница пейджера: снимок с зумом или видео. [motion] — только у текущей. */
@Composable
private fun MediaPage(
    model: AppModel,
    item: ViewerPage,
    bytes: ByteArray?,
    muted: Boolean,
    chrome: Boolean,
    motion: CloseMotion?,
    closeAt: Float,
    onChrome: () -> Unit,
    onDrag: (Float) -> Unit,
    onDragEnd: () -> Unit,
) {
    var scale by remember(item.key) { mutableFloatStateOf(1f) }
    var offX by remember(item.key) { mutableFloatStateOf(0f) }
    var offY by remember(item.key) { mutableFloatStateOf(0f) }
    var box by remember { mutableStateOf(IntSize.Zero) }
    val zoom by animateFloatAsState(scale, label = "zoom")
    val video = item.video
    val radius = with(LocalDensity.current) { Tokens.Radius.bubble.toPx() }

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
    val zoomable = Modifier.pointerInput(item.key) {
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

    val taps = Modifier.pointerInput(item.key, video) {
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
    val closeDrag = Modifier.pointerInput(item.key) {
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
                val bmp = rememberFullscreenBitmap(bytes, item.blobId.ifEmpty { null })
                // GIF играет и здесь (Android 9+): тот же Image, зум и протяжка — по нему же.
                val cfg = LocalConfiguration.current
                val anim = rememberAnimatedPainter(bytes, maxOf(cfg.screenWidthDp, cfg.screenHeightDp).dp)
                val painter = anim ?: bmp?.let { remember(it) { BitmapPainter(it) } }
                if (painter != null) {
                    // Снимок занимает ровно свой прямоугольник, а не весь экран: иначе
                    // скругление на протяжке легло бы на поля, а не на углы снимка.
                    Image(
                        painter,
                        null,
                        Modifier.aspectRatio(painter.intrinsicSize.width / painter.intrinsicSize.height.coerceAtLeast(1f))
                            .then(if (motion != null) Modifier.onGloballyPositioned { motion.image = it.boundsInRoot() } else Modifier)
                            .graphicsLayer {
                                var s = zoom
                                var tx = offX
                                var ty = offY
                                var a = 1f
                                var r = 0f
                                if (motion != null) {
                                    val p = motion.fly.value
                                    val s0 = motion.dragScale()
                                    val t = motion.target
                                    val img = motion.image
                                    r = radius * maxOf(motion.pulled(closeAt), p)
                                    if (t != null && img.width > 0f && img.height > 0f) {
                                        // В пузырь: центр к центру, размер — вписать в него.
                                        val sT = min(t.width / img.width, t.height / img.height)
                                        s *= lerp(s0, sT, p)
                                        tx += lerp(0f, t.center.x - img.center.x, p)
                                        ty += lerp(motion.dragY, t.center.y - img.center.y, p)
                                    } else {
                                        s *= s0 * (1f - FADE_SHRINK * p)
                                        ty += motion.dragY
                                        a = 1f - p
                                    }
                                }
                                scaleX = s
                                scaleY = s
                                translationX = tx
                                translationY = ty
                                alpha = a
                                if (r > 0f) {
                                    shape = RoundedCornerShape(r / s.coerceAtLeast(0.01f))
                                    clip = true
                                }
                            },
                        contentScale = ContentScale.Fit,
                    )
                }
            }
        }
    }
}

/** Пузыря на экране нет — снимок гаснет, чуть уменьшаясь. */
private const val FADE_SHRINK = 0.1f

/** Плеер один на весь просмотрщик: таймлайн лежит в другой ветке дерева, а
 *  управляет тем же воспроизведением. */
private object Player {
    var view: android.widget.VideoView? = null
    var media: android.media.MediaPlayer? = null
}

/** Как часто опрашивать плеер: VideoView не сообщает о положении сам, а четыре раза в
 *  секунду хватает, чтобы полоса не дёргалась и не грела процессор. */
private const val PLAYER_POLL_MS = 250L

/**
 * Видео проигрывается ВНУТРИ приложения, а не отдаётся системному плееру: файл
 * из чата — чужая переписка, и выпускать его в сторонние приложения ради кнопки
 * «play» не за что. Байты кладём в приватный кэш, откуда их никто не читает.
 */
@Composable
private fun VideoSurface(model: AppModel, item: ViewerPage, muted: Boolean, chrome: Boolean) {
    // Видео расшифровывается в приватный кэш потоково, по чанку: в память целиком
    // оно не поднимается. Копия живёт, пока открыт просмотр.
    var file by remember(item.blobId) { mutableStateOf<java.io.File?>(null) }
    LaunchedEffect(item.blobId) {
        // Видео из галереи телефона играет со своего адреса: копировать нечего.
        if (item.uri == null) model.exportBlob(item.blobId, "viewer", "${item.blobId.take(16)}-${item.name}") { file = it }
    }
    var mine by remember(item.blobId) { mutableStateOf<android.widget.VideoView?>(null) }
    var playing by remember(item.blobId) { mutableStateOf(true) }
    LaunchedEffect(mine) {
        while (true) {
            mine?.let { playing = it.isPlaying }
            kotlinx.coroutines.delay(PLAYER_POLL_MS)
        }
    }
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
        if (f == null && item.uri == null) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(36.dp), strokeWidth = 2.dp)
        } else {
            AndroidView(
                factory = { c ->
                    android.widget.VideoView(c).apply {
                        if (item.uri != null) setVideoURI(item.uri) else setVideoPath(f!!.absolutePath)
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
            // Крупная пауза/пуск по центру (макет `viewer/Video`): с хромом — всегда,
            // без него — пока стоит на паузе, чтобы было видно, что видео не идёт.
            if (chrome || !playing) {
                val desc = stringResource(if (playing) R.string.viewer_pause else R.string.viewer_play)
                Box(
                    Modifier.size(Tokens.Size.viewerPlay).clip(CircleShape).background(Tokens.Colors.viewerPlayScrim)
                        .clickable(onClickLabel = desc) {
                            val v = mine ?: return@clickable
                            if (v.isPlaying) v.pause() else v.start()
                            playing = v.isPlaying
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        if (playing) TablerIcons.PauseBars else TablerIcons.PlayOutline,
                        desc,
                        tint = Color.White,
                        modifier = Modifier.size(Tokens.Size.viewerPlayIcon),
                    )
                }
            }
        }
    }
}

/** Таймлайн видео (макет `viewer/Video`): время · полоса с перемоткой · длительность · звук.
 *  Неизвестная длительность — без полосы. Пуск и пауза — крупной кнопкой по центру. */
@Composable
private fun VideoBar(muted: Boolean, onMute: (Boolean) -> Unit) {
    var position by remember { mutableIntStateOf(0) }
    var duration by remember { mutableIntStateOf(0) }
    var width by remember { mutableIntStateOf(1) }
    LaunchedEffect(Unit) {
        while (true) {
            Player.view?.let {
                position = it.currentPosition
                duration = it.duration.coerceAtLeast(0)
            }
            kotlinx.coroutines.delay(PLAYER_POLL_MS)
        }
    }
    val seek = { x: Float ->
        if (duration > 0) {
            val ms = (x / width * duration).toInt().coerceIn(0, duration)
            Player.view?.seekTo(ms)
            position = ms
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.panel)) {
        Text(fmtDuration(position), color = Color.White, fontSize = Tokens.Font.small, style = Tokens.Text.tabular)
        if (duration > 0) {
            // Область касания намеренно выше самой линии: попадать пальцем в четыре
            // пикселя невозможно.
            Box(
                Modifier.weight(1f)
                    .height(Tokens.Size.iconL)
                    .onSizeChanged { width = it.width.coerceAtLeast(1) }
                    .pointerInput(duration) { detectTapGestures { p -> seek(p.x) } }
                    .pointerInput(duration) { detectHorizontalDragGestures { change, _ -> seek(change.position.x) } },
                contentAlignment = Alignment.CenterStart,
            ) {
                val done = (position.toFloat() / duration).coerceIn(0f, 1f)
                val track = RoundedCornerShape(Tokens.Radius.thumb)
                Box(Modifier.fillMaxWidth().height(Tokens.Size.mediaTrack).clip(track).background(Tokens.Colors.trackOnMedia))
                Box(Modifier.fillMaxWidth(done).height(Tokens.Size.mediaTrack).clip(track).background(Color.White))
                // Бегунок: его положение — доля ширины минус половина бегунка.
                BoxWithConstraints(Modifier.fillMaxWidth()) {
                    Box(
                        Modifier.offset(x = maxWidth * done - Tokens.Size.mediaThumb / 2)
                            .size(Tokens.Size.mediaThumb).clip(CircleShape).background(Color.White),
                    )
                }
            }
            Text(fmtDuration(duration), color = Color.White, fontSize = Tokens.Font.small, style = Tokens.Text.tabular)
        } else {
            Spacer(Modifier.weight(1f))
        }
        val desc = stringResource(if (muted) R.string.viewer_unmute else R.string.viewer_mute)
        Box(
            Modifier.size(Tokens.Size.viewerButton).clip(RoundedCornerShape(Tokens.Radius.button)).clickable(onClickLabel = desc) { onMute(!muted) },
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (muted) TablerIcons.VolumeOff else TablerIcons.Volume, desc, tint = Color.White, modifier = Modifier.size(Tokens.Size.iconRow))
        }
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
