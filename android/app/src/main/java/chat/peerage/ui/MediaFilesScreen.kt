package chat.peerage.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale

/** Шапка «Медиа и файлы · чат · N»; у сетки — «⋮» с плотностью (тот же выбор, что щипком). */
@Composable
internal fun MediaFilesTopBar(model: AppModel, onBack: () -> Unit) {
    val b = model.mediaBrowse ?: return
    var menu by remember { mutableStateOf(false) }
    val total = b.counts?.all
    TopBar(
        stringResource(R.string.profile_media),
        onBack = onBack,
        subtitle = chatTitle(model, b.chat) + (total?.let { " · $it" } ?: ""),
    ) {
        if (b.tab == MediaTab.Media) {
            Box {
                BarIcon(TablerIcons.DotsVertical, stringResource(R.string.common_menu)) { menu = true }
                PopMenu(menu, { menu = false }) {
                    // Один пункт, а не три: плотностей мало, и перебор по кругу короче выбора.
                    PopMenuItem(
                        TablerIcons.LayoutGrid,
                        stringResource(R.string.media_density),
                        trailing = { Text("${model.mediaColumns}", color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label) },
                    ) { model.setMediaColumns(nextColumns(model.mediaColumns, wider = false)) }
                }
            }
        }
    }
}

/** Название чата для подписей: имя группы или собеседника. */
@Composable
private fun chatTitle(model: AppModel, chat: ChatTarget): String = when {
    chat.title.isNotBlank() -> chat.title
    chat.group -> model.groups.firstOrNull { it.gid == chat.id }?.name.orEmpty()
    else -> model.peerName(chat.id)
}

/**
 * «Медиа и файлы» (app-ui.md, макеты `screens/MediaFiles`, `MfFiles`, `MfVoice`, `MfLinks`):
 * вкладки по видам вложений, внутри — по месяцам, свежие сверху. Листается вся история
 * чата страницами движка, а не только подгруженная лента.
 *
 * Долгое нажатие на любой строке — «показать в чате»: экран отвечает на «где это было».
 */
@Composable
internal fun MediaFilesScreen(model: AppModel, onShowInChat: (MsgRef) -> Unit) {
    val b = model.mediaBrowse ?: return
    LaunchedEffect(b, b.tab) { model.moreMedia(b, b.tab) }
    Column(Modifier.fillMaxSize()) {
        MediaTabs(b.tab) { b.tab = it }
        val list = b.lists.getValue(b.tab)
        when {
            !list.loaded -> Unit
            list.items.isEmpty() -> MediaEmpty(b.tab)
            b.tab == MediaTab.Media -> MediaGrid(model, b, list, onShowInChat)
            else -> MediaRows(model, b, list, onShowInChat)
        }
    }
    ForwardSheet(model)
}

@Composable
private fun MediaTabs(selected: MediaTab, onSelect: (MediaTab) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.xs),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xl),
        ) {
            MediaTab.entries.forEach { tab ->
                FolderTab(stringResource(tabLabel(tab)), tab == selected, 0) { onSelect(tab) }
            }
        }
        HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider)
    }
}

private fun tabLabel(tab: MediaTab) = when (tab) {
    MediaTab.Media -> R.string.media_tab_media
    MediaTab.Files -> R.string.media_tab_files
    MediaTab.Voice -> R.string.media_tab_voice
    MediaTab.Links -> R.string.media_tab_links
}

@Composable
private fun MediaEmpty(tab: MediaTab) {
    val text = when (tab) {
        MediaTab.Media -> R.string.media_empty_media
        MediaTab.Files -> R.string.media_empty_files
        MediaTab.Voice -> R.string.media_empty_voice
        MediaTab.Links -> R.string.media_empty_links
    }
    Box(Modifier.fillMaxSize().padding(Tokens.Space.xxl), contentAlignment = Alignment.Center) {
        Text(stringResource(text), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
    }
}

// ── Месяцы ──────────────────────────────────────────────────────────────────

/** Подряд идущие сообщения одного месяца: подпись и сами сообщения. */
private class MonthGroup(val label: String, val items: List<Message>)

/** «Октябрь», в прошлых годах — «Октябрь 2025». Сообщения уже идут свежими сверху. */
private fun byMonth(items: List<Message>, locale: Locale): List<MonthGroup> {
    val nowYear = Calendar.getInstance().get(Calendar.YEAR)
    val cal = Calendar.getInstance()
    val month = SimpleDateFormat("LLLL", locale)
    val monthYear = SimpleDateFormat("LLLL yyyy", locale)
    val out = ArrayList<MonthGroup>()
    var key = -1
    var bucket = ArrayList<Message>()
    var label = ""
    for (m in items) {
        cal.timeInMillis = m.time * 1000
        val k = cal.get(Calendar.YEAR) * 12 + cal.get(Calendar.MONTH)
        if (k != key) {
            if (bucket.isNotEmpty()) out += MonthGroup(label, bucket)
            key = k
            bucket = ArrayList()
            val fmt = if (cal.get(Calendar.YEAR) == nowYear) month else monthYear
            label = fmt.format(cal.time).replaceFirstChar { it.titlecase(locale) }
        }
        bucket += m
    }
    if (bucket.isNotEmpty()) out += MonthGroup(label, bucket)
    return out
}

@Composable
private fun MonthLabel(text: String, start: androidx.compose.ui.unit.Dp, bottom: androidx.compose.ui.unit.Dp) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = start, end = start, top = Tokens.Space.m + Tokens.Space.xxs, bottom = bottom),
    )
}

/** Подпись строки: «12 окт.». */
private fun shortDate(time: Long, locale: Locale): String = SimpleDateFormat("d MMM", locale).format(Date(time * 1000))

@Composable
private fun senderLabel(model: AppModel, m: Message): String =
    if (m.mine) stringResource(R.string.media_you) else model.peerName(m.sender)

/** Догрузка вкладки, когда до конца загруженного осталось немного. */
@Composable
private fun LoadMoreAt(lastVisible: () -> Int, size: Int, onMore: () -> Unit) {
    val near by remember(size) { derivedStateOf { lastVisible() >= size - 6 } }
    LaunchedEffect(near, size) { if (near) onMore() }
}

private fun LazyListState.lastIndex() = layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
private fun LazyGridState.lastIndex() = layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0

/** Вложение скачано (из истории или докачано за эту сессию). */
private fun ready(model: AppModel, m: Message) = m.downloaded || model.savedBlobs[m.blobId] == true

/** Скачать у автора: в личном чате это собеседник, в группе — приславший. */
private fun download(model: AppModel, chat: ChatTarget, m: Message) =
    model.downloadMedia(if (chat.group) m.sender else chat.id, m.blobId)

// ── Медиа: сетка ────────────────────────────────────────────────────────────

/** Следующая плотность: щипок «развести» — крупнее (меньше столбцов), «свести» — мельче. */
private fun nextColumns(cols: Int, wider: Boolean): Int {
    val i = MEDIA_COLUMNS.indexOf(cols).coerceAtLeast(0)
    return if (wider) {
        MEDIA_COLUMNS[(i - 1).coerceAtLeast(0)]
    } else if (i == MEDIA_COLUMNS.lastIndex) {
        MEDIA_COLUMNS.first() // из «⋮» — по кругу; щипок дальше 5 не уводит, см. pinchColumns
    } else {
        MEDIA_COLUMNS[i + 1]
    }
}

/** Порог щипка: насколько развести или свести пальцы, чтобы сменить плотность. */
private const val PINCH_STEP = 1.25f

/**
 * Щипок двумя пальцами меняет плотность сетки. Один палец сетке не мешает — жест
 * перехватывается только при двух касаниях, и тогда прокрутка не дёргается.
 */
private fun Modifier.pinchColumns(cols: () -> Int, onCols: (Int) -> Unit) = pointerInput(Unit) {
    awaitEachGesture {
        awaitFirstDown(requireUnconsumed = false)
        var zoom = 1f
        do {
            val event = awaitPointerEvent(PointerEventPass.Initial)
            if (event.changes.count { it.pressed } >= 2) {
                zoom *= event.calculateZoom()
                val c = cols()
                when {
                    zoom > PINCH_STEP && c != MEDIA_COLUMNS.first() -> {
                        onCols(nextColumns(c, wider = true))
                        zoom = 1f
                    }
                    zoom < 1f / PINCH_STEP && c != MEDIA_COLUMNS.last() -> {
                        onCols(nextColumns(c, wider = false))
                        zoom = 1f
                    }
                }
                event.changes.forEach { it.consume() }
            }
        } while (event.changes.any { it.pressed })
    }
}

@Composable
private fun MediaGrid(model: AppModel, b: MediaBrowse, list: MediaList, onShowInChat: (MsgRef) -> Unit) {
    val locale = LocalContext.current.resources.configuration.locales[0]
    val groups = remember(list.items.size, locale) { byMonth(list.items.toList(), locale) }
    val state = rememberLazyGridState()
    var viewerAt by remember { mutableStateOf<MsgRef?>(null) }
    val cols = model.mediaColumns
    LoadMoreAt({ state.lastIndex() }, list.items.size + groups.size) { model.moreMedia(b, MediaTab.Media) }
    LazyVerticalGrid(
        GridCells.Fixed(cols),
        Modifier.fillMaxSize().pinchColumns({ model.mediaColumns }) { model.setMediaColumns(it) },
        state = state,
        contentPadding = PaddingValues(horizontal = Tokens.Space.series, vertical = 0.dp),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.series),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.series),
    ) {
        groups.forEach { g ->
            item(key = "m:" + g.label, span = { GridItemSpan(maxLineSpan) }) {
                MonthLabel(g.label, Tokens.Space.l - Tokens.Space.series, Tokens.Space.s - Tokens.Space.series)
            }
            items(g.items, key = { "${it.sender}:${it.id}" }) { m ->
                MediaTile(model, b.chat, m, cols, onLong = { onShowInChat(MsgRef(m.id, m.sender)) }) {
                    viewerAt = MsgRef(m.id, m.sender)
                }
            }
        }
    }
    viewerAt?.let { at ->
        // Ответ — в ленте чата: туда и уходим, плашка ответа встаёт уже там.
        MediaViewer(
            model,
            b.chat,
            list.items.toList(),
            at,
            onClose = { viewerAt = null },
            onReply = { m ->
                onShowInChat(MsgRef(m.id, m.sender))
                model.startReply(m)
            },
            onShowInChat = { m -> onShowInChat(MsgRef(m.id, m.sender)) },
            onDeleted = { ref ->
                list.items.removeAll { it.id == ref.id && it.sender == ref.sender }
                list.total = (list.total - 1).coerceAtLeast(0)
            },
        )
    }
}

/** Плитка сетки: миниатюра; у видео — длительность; нескачанное — кнопка загрузки и размер. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MediaTile(model: AppModel, chat: ChatTarget, m: Message, cols: Int, onLong: () -> Unit, onOpen: () -> Unit) {
    val ok = ready(model, m)
    val video = m.kind == "video"
    // Присланный кадр — у видео всегда, у фото если есть; иначе само фото, когда оно здесь.
    val src = when {
        m.thumbId.isNotEmpty() -> m.thumbId
        !video && ok -> m.blobId
        else -> ""
    }
    val side = Tokens.Size.attachGrid / cols
    val bmp = rememberBitmap(model.blob(src), src, side)
    val shape = RoundedCornerShape(Tokens.Radius.gridTile)
    val progress = model.downloads[m.blobId]
    Box(
        // Клетка — место возврата просмотрщика: протяжкой вниз снимок уходит сюда же.
        Modifier.aspectRatio(1f).mediaSpot(MsgRef(m.id, m.sender))
            .clip(shape).background(Tokens.Colors.tile).imageOutline(shape)
            .combinedClickable(onLongClick = onLong) {
                if (ok) {
                    onOpen()
                } else if (progress == null) {
                    download(model, chat, m)
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        bmp?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
        if (!ok) {
            Box(Modifier.size(Tokens.Size.ib).clip(CircleShape).background(Tokens.Colors.mediaScrim), contentAlignment = Alignment.Center) {
                DownloadState(progress, tint = Color.White, size = Tokens.Size.iconM)
            }
        }
        val badge = when {
            !ok && m.size > 0 -> fmtSizeLong(m.size)
            video && m.durationMs > 0 -> fmtDuration(m.durationMs)
            else -> null
        }
        if (badge != null && cols < 5) {
            Text(
                badge,
                color = Color.White,
                fontSize = Tokens.Font.caption,
                modifier = Modifier.align(Alignment.BottomStart)
                    .padding(Tokens.Space.xs + Tokens.Space.xxs)
                    .clip(RoundedCornerShape(Tokens.Radius.badge))
                    .background(Tokens.Colors.mediaScrim)
                    .padding(horizontal = Tokens.Space.xs + 1.dp, vertical = 1.dp),
            )
        }
    }
}

/** Значок загрузки → крутилка (ждём пира) → заполняющийся кружок (идут байты). */
@Composable
private fun DownloadState(progress: Float?, tint: Color, size: androidx.compose.ui.unit.Dp) {
    when {
        progress == AppModel.DOWNLOAD_INIT ->
            CircularProgressIndicator(color = tint, modifier = Modifier.size(size), strokeWidth = 2.dp)
        progress != null ->
            CircularProgressIndicator(progress = { progress }, color = tint, modifier = Modifier.size(size), strokeWidth = 2.dp)
        else -> Icon(TablerIcons.Download, stringResource(R.string.chat_download), tint = tint, modifier = Modifier.size(size))
    }
}

// ── Файлы, голосовые, ссылки: строки ────────────────────────────────────────

/** Отступ разделителя: поле строки + плитка + зазор (макеты `Mf*`). */
private val RowInset = Tokens.Space.xl + Tokens.Size.mediaTile + Tokens.Space.m + Tokens.Space.xxs

@Composable
private fun MediaRows(model: AppModel, b: MediaBrowse, list: MediaList, onShowInChat: (MsgRef) -> Unit) {
    val locale = LocalContext.current.resources.configuration.locales[0]
    val groups = remember(list.items.size, locale) { byMonth(list.items.toList(), locale) }
    val state = rememberLazyListState()
    val playback = rememberVoicePlayback()
    LoadMoreAt({ state.lastIndex() }, list.items.size + groups.size) { model.moreMedia(b, b.tab) }
    LazyColumn(Modifier.fillMaxSize(), state = state) {
        groups.forEach { g ->
            item(key = "m:" + g.label) { MonthLabel(g.label, Tokens.Space.xl, Tokens.Space.xs) }
            items(g.items, key = { "${it.sender}:${it.id}" }) { m ->
                if (m !== g.items.first()) {
                    HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = RowInset))
                }
                val show = { onShowInChat(MsgRef(m.id, m.sender)) }
                when (b.tab) {
                    MediaTab.Files -> FileRow(model, b.chat, m, locale, show)
                    MediaTab.Voice -> VoiceRow(model, b.chat, m, locale, playback, show)
                    else -> LinkRow(model, m, locale, show)
                }
            }
        }
    }
}

/** Цвет плитки файла по расширению: документ, таблица, PDF, архив, остальное. */
private fun fileColors(ext: String): Pair<Color, Color> = when (ext) {
    "pdf" -> Tokens.Colors.filePdf to Tokens.Colors.filePdfText
    "zip", "rar", "7z", "tar", "gz", "xz" -> Tokens.Colors.fileArchive to Tokens.Colors.fileArchiveText
    "xls", "xlsx", "ods", "csv" -> Tokens.Colors.fileSheet to Tokens.Colors.fileSheetText
    "doc", "docx", "odt", "rtf", "txt", "md" -> Tokens.Colors.fileDoc to Tokens.Colors.fileDocText
    else -> Tokens.Colors.fileOther to Tokens.Colors.chipText
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun FileRow(model: AppModel, chat: ChatTarget, m: Message, locale: Locale, onLong: () -> Unit) {
    val ctx = LocalContext.current
    val ok = ready(model, m)
    val progress = model.downloads[m.blobId]
    val ext = m.name.substringAfterLast('.', "").lowercase(Locale.ROOT)
    val (bg, fg) = fileColors(ext)
    Row(
        Modifier.fillMaxWidth().height(Tokens.Size.mediaFileRow)
            .combinedClickable(onLongClick = onLong) {
                if (ok) {
                    model.exportBlob(m.blobId, "shared", m.name) { f -> f?.let { openWithSystem(ctx, it) } }
                } else if (progress == null) {
                    download(model, chat, m)
                }
            }
            .padding(start = Tokens.Space.xl, end = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Box(
            Modifier.size(Tokens.Size.mediaTile).clip(RoundedCornerShape(Tokens.Radius.mediaTile)).background(bg),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                ext.take(4).uppercase(Locale.ROOT).ifBlank { "•" },
                color = fg,
                fontSize = Tokens.Font.caption,
                fontWeight = FontWeight.Bold,
                letterSpacing = Tokens.Font.tileTracking,
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs)) {
            Text(
                m.name.ifBlank { stringResource(R.string.chat_file) },
                color = Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                listOf(fmtSizeLong(m.size), senderLabel(model, m), shortDate(m.time, locale)).joinToString(" · "),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.small,
                maxLines = 1,
            )
        }
        if (!ok) {
            Box(
                Modifier.size(Tokens.Size.ib).clip(CircleShape).background(Tokens.Colors.bubbleIn),
                contentAlignment = Alignment.Center,
            ) { DownloadState(progress, tint = Tokens.Colors.accent, size = Tokens.Size.iconS) }
        }
    }
}

/** Один плеер на экран: включённое голосовое выключает прежнее. */
private class VoicePlayback {
    var playing by mutableStateOf<String?>(null)
    var posMs by mutableIntStateOf(0)
    var player: android.media.MediaPlayer? = null
    var blobId: String? = null

    fun release() {
        player?.release()
        player = null
        blobId = null
        playing = null
        posMs = 0
    }
}

@Composable
private fun rememberVoicePlayback(): VoicePlayback {
    val p = remember { VoicePlayback() }
    DisposableEffect(p) { onDispose { p.release() } }
    // Позиция — для прогресса по волне и «0:05 / 0:12».
    LaunchedEffect(p.playing) {
        while (p.playing != null) {
            p.player?.let { runCatching { p.posMs = it.currentPosition } }
            delay(VOICE_TICK_MS)
        }
    }
    return p
}

private const val VOICE_TICK_MS = 50L

/** Столбиков в волне: длиннее запись — длиннее волна, но в разумных пределах. */
private fun voiceBars(durationMs: Int) = (10 + durationMs / 2000).coerceIn(10, 30)

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun VoiceRow(model: AppModel, chat: ChatTarget, m: Message, locale: Locale, p: VoicePlayback, onLong: () -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val ok = ready(model, m)
    val bytes = if (ok) model.blob(m.blobId) else null
    val key = "${m.sender}:${m.id}"
    val playing = p.playing == key
    val bars = voiceBars(m.durationMs)
    val wave by rememberVoiceLevels(m.blobId, bytes, bars)
    // У голосовых, отправленных без длительности, она берётся из самого звука.
    val durationMs = m.durationMs.takeIf { it > 0 } ?: wave?.durationMs ?: 0
    val progress = model.downloads[m.blobId]
    val toggle: () -> Unit = {
        val mp = p.player
        when {
            !ok -> if (progress == null) download(model, chat, m)
            playing && mp != null -> {
                mp.pause()
                p.playing = null
            }
            p.blobId == m.blobId && mp != null -> {
                mp.start()
                p.playing = key
            }
            bytes != null -> scope.launch {
                p.release()
                // Файл и подготовка плеера — не на главном потоке; копия — в `viewer/`,
                // откуда расшифрованное убирается на старте.
                val np = withContext(Dispatchers.IO) {
                    runCatching {
                        val dir = java.io.File(ctx.cacheDir, "viewer").apply { mkdirs() }
                        val f = java.io.File(dir, "voice_${m.blobId}.m4a").apply { writeBytes(bytes) }
                        android.media.MediaPlayer().apply {
                            setDataSource(f.absolutePath)
                            prepare()
                        }
                    }.getOrNull()
                } ?: return@launch
                np.setOnCompletionListener {
                    p.playing = null
                    p.posMs = 0
                    it.seekTo(0)
                }
                p.player = np
                p.blobId = m.blobId
                np.start()
                p.playing = key
            }
        }
    }
    Row(
        Modifier.fillMaxWidth().height(Tokens.Size.mediaVoiceRow)
            .combinedClickable(onLongClick = onLong, onClick = toggle)
            .padding(horizontal = Tokens.Space.xl),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Box(
            Modifier.size(Tokens.Size.mediaTile).clip(CircleShape)
                .background(if (playing) Tokens.Colors.accent else Tokens.Colors.bubbleIn),
            contentAlignment = Alignment.Center,
        ) {
            when {
                !ok -> DownloadState(progress, tint = Tokens.Colors.accent, size = Tokens.Size.iconS)
                playing -> Icon(Icons.Filled.Pause, stringResource(R.string.media_pause), tint = Tokens.Colors.accentDark, modifier = Modifier.size(Tokens.Size.iconS))
                else -> Icon(Icons.Filled.PlayArrow, stringResource(R.string.media_play), tint = Tokens.Colors.accent, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs)) {
            val played = if (playing || p.blobId == m.blobId) {
                p.posMs.toFloat() / durationMs.coerceAtLeast(1)
            } else {
                0f
            }
            VoiceBars(wave?.levels, bars, played)
            val rest = listOf(senderLabel(model, m), shortDate(m.time, locale)).joinToString(" · ")
            Text(
                buildAnnotatedString {
                    if (playing || (p.blobId == m.blobId && p.posMs > 0)) {
                        withStyle(SpanStyle(color = Tokens.Colors.accent)) { append(fmtDuration(p.posMs)) }
                        append(" / ")
                    }
                    if (durationMs > 0) {
                        append(fmtDuration(durationMs))
                        append(" · ")
                    }
                    append(rest)
                },
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.small,
                maxLines = 1,
            )
        }
    }
}

/** Волна: столбики громкости; проигранная часть — акцентом. Пока не посчитана — ровная черта. */
@Composable
private fun VoiceBars(levels: FloatArray?, bars: Int, played: Float) {
    Row(
        Modifier.height(Tokens.Size.mediaWave),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xxs),
    ) {
        repeat(bars) { i ->
            val level = levels?.getOrNull(i) ?: 0f
            val h = Tokens.Size.waveMin + (Tokens.Size.mediaWave - Tokens.Size.waveMin) * level
            val on = (i + 0.5f) / bars <= played
            Box(
                Modifier.width(Tokens.Size.waveBar).height(h)
                    .clip(RoundedCornerShape(Tokens.Radius.thumb))
                    .background(if (on) Tokens.Colors.accent else Tokens.Colors.waveIdle),
            )
        }
    }
}

/** Первая ссылка в тексте; хвостовая пунктуация — не часть адреса. */
private val UrlRe = Regex("""https?://[^\s<>"«»]+""", RegexOption.IGNORE_CASE)

private fun firstUrl(text: String): String? = UrlRe.find(text)?.value?.trimEnd('.', ',', ')', '!', '?', ';', ':')

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun LinkRow(model: AppModel, m: Message, locale: Locale, onLong: () -> Unit) {
    val ctx = LocalContext.current
    val url = firstUrl(m.text) ?: return
    val uri = android.net.Uri.parse(url)
    val host = uri.host.orEmpty().lowercase(Locale.ROOT).removePrefix("www.")
    val shown = url.substringAfter("://")
    // Строка сообщения без самой ссылки: адрес уже стоит строкой выше.
    val rest = m.text.replace(url, "").trim()
    Row(
        Modifier.fillMaxWidth()
            .combinedClickable(onLongClick = onLong) {
                runCatching {
                    ctx.startActivity(
                        android.content.Intent(android.content.Intent.ACTION_VIEW, uri)
                            .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
                    )
                }
            }
            .padding(horizontal = Tokens.Space.xl, vertical = Tokens.Space.m),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Box(
            Modifier.size(Tokens.Size.mediaTile).clip(RoundedCornerShape(Tokens.Radius.mediaTile)).background(Tokens.Colors.bubbleIn),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                host.take(1).uppercase(locale),
                color = Tokens.Colors.chipText,
                fontSize = Tokens.Font.tileLetter,
                fontWeight = FontWeight.Bold,
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs + 1.dp)) {
            Text(host, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(shown, color = Tokens.Colors.accent, fontSize = Tokens.Font.body, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (rest.isNotEmpty()) {
                Text(
                    senderLabel(model, m) + ": " + rest,
                    color = Tokens.Colors.snippet,
                    fontSize = Tokens.Font.body,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(shortDate(m.time, locale), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.small)
        }
    }
}

// ── Вход из профиля ─────────────────────────────────────────────────────────

/** Снимков в ряду превью профиля (макет `prof/Main`). */
private const val PREVIEW_COUNT = 4

/**
 * Строка «Медиа и файлы» профиля с общим счётом и рядом последних снимков (макет
 * `prof/Main`). Тап по строке или снимку — экран медиа.
 */
@Composable
internal fun ProfileMediaEntry(model: AppModel, chat: ChatTarget, onOpen: () -> Unit) {
    var counts by remember(chat) { mutableStateOf<MediaCounts?>(null) }
    var preview by remember(chat) { mutableStateOf<List<Message>>(emptyList()) }
    LaunchedEffect(chat) {
        model.refreshMediaCounts(chat) { counts = it }
        model.mediaPreview(chat, PREVIEW_COUNT) { preview = it }
    }
    NavRow(
        TablerIcons.Photo,
        stringResource(R.string.profile_media),
        value = counts?.all?.toString().orEmpty(),
        divider = false,
        onClick = onOpen,
    )
    if (preview.isNotEmpty()) {
        Row(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.m + Tokens.Space.xxs, end = Tokens.Space.m + Tokens.Space.xxs, bottom = Tokens.Space.m + Tokens.Space.xxs),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
        ) {
            repeat(PREVIEW_COUNT) { i ->
                val m = preview.getOrNull(i)
                Box(Modifier.weight(1f)) {
                    if (m != null) PreviewThumb(model, m, onOpen) else Spacer(Modifier.aspectRatio(1f))
                }
            }
        }
    }
}

@Composable
private fun PreviewThumb(model: AppModel, m: Message, onOpen: () -> Unit) {
    val src = m.thumbId.ifEmpty { if (m.kind == "image" && ready(model, m)) m.blobId else "" }
    val bmp = rememberBitmap(model.blob(src), src, Tokens.Size.attachGrid / PREVIEW_COUNT)
    val shape = RoundedCornerShape(Tokens.Radius.thumb)
    Box(
        Modifier.fillMaxWidth().aspectRatio(1f).clip(shape).background(Tokens.Colors.tile).imageOutline(shape)
            .clickable(onClick = onOpen),
    ) {
        bmp?.let { Image(it, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
    }
}
