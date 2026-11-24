package chat.peerage.ui

import android.Manifest
import android.content.Context
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Снимок из галереи устройства. */
internal data class GalleryItem(
    val uri: Uri,
    val size: Long,
    val durationMs: Long, // 0 — картинка
    val isVideo: Boolean,
)

private val MEDIA_PERMS: Array<String> =
    if (Build.VERSION.SDK_INT >= 33) {
        arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO)
    } else {
        arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
    }

/** Страница галереи — столько снимков за раз: вся галерея в память не нужна. */
private const val GALLERY_PAGE = 120

/** Прочитать [limit] элементов галереи (картинки + видео), начиная с [from]-го от свежих. */
private suspend fun loadGallery(ctx: Context, from: Int, limit: Int = GALLERY_PAGE): List<GalleryItem> = withContext(Dispatchers.IO) {
    val out = mutableListOf<GalleryItem>()
    val uri = MediaStore.Files.getContentUri("external")
    val cols = arrayOf(
        MediaStore.Files.FileColumns._ID,
        MediaStore.Files.FileColumns.SIZE,
        MediaStore.Files.FileColumns.MEDIA_TYPE,
        MediaStore.Files.FileColumns.DURATION,
    )
    val sel = "${MediaStore.Files.FileColumns.MEDIA_TYPE} IN (?, ?)"
    val args = arrayOf(
        MediaStore.Files.FileColumns.MEDIA_TYPE_IMAGE.toString(),
        MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO.toString(),
    )
    runCatching {
        ctx.contentResolver.query(
            uri,
            cols,
            sel,
            args,
            "${MediaStore.Files.FileColumns.DATE_MODIFIED} DESC",
        )?.use { c ->
            val idI = c.getColumnIndexOrThrow(MediaStore.Files.FileColumns._ID)
            val szI = c.getColumnIndexOrThrow(MediaStore.Files.FileColumns.SIZE)
            val tpI = c.getColumnIndexOrThrow(MediaStore.Files.FileColumns.MEDIA_TYPE)
            val duI = c.getColumnIndexOrThrow(MediaStore.Files.FileColumns.DURATION)
            if (!c.moveToPosition(from - 1) && from > 0) return@use
            while (c.moveToNext() && out.size < limit) {
                val video = c.getInt(tpI) == MediaStore.Files.FileColumns.MEDIA_TYPE_VIDEO
                out += GalleryItem(
                    uri = android.content.ContentUris.withAppendedId(uri, c.getLong(idI)),
                    size = c.getLong(szI),
                    durationMs = if (video) c.getLong(duI) else 0,
                    isVideo = video,
                )
            }
        }
    }
    out
}

/** Миниатюра под размер плитки. Грузим по требованию: полная галерея в память не влезет. */
@Composable
private fun rememberThumb(item: GalleryItem): ImageBitmap? {
    val ctx = LocalContext.current
    var bmp by remember(item.uri) { mutableStateOf<ImageBitmap?>(null) }
    LaunchedEffect(item.uri) {
        bmp = withContext(Dispatchers.IO) {
            runCatching {
                if (Build.VERSION.SDK_INT >= 29) {
                    ctx.contentResolver.loadThumbnail(item.uri, android.util.Size(256, 256), null)
                } else {
                    @Suppress("DEPRECATION")
                    MediaStore.Images.Media.getBitmap(ctx.contentResolver, item.uri)
                }
            }.getOrNull()?.asImageBitmap()
        }
    }
    return bmp
}

/**
 * Панель «Прикрепить» (app-ui.md → «Поле ввода», макет `chatact/Attach`): выезжает из строки
 * ввода, как эмодзи, — поле остаётся под ней, и подпись набирается сразу. Камера и последние
 * снимки сеткой с номерами выбора, «Файл», «Аудио»; под ними — сколько выбрано и вес.
 * Отправляет кнопка поля: текст поля — подпись к первому вложению.
 *
 * @param selected выбранные элементы в порядке выбора (номер на плитке = позиция)
 */
@Composable
internal fun AttachPanel(
    selected: List<GalleryItem>,
    onToggle: (GalleryItem) -> Unit,
    onCamera: () -> Unit,
    onPickFile: () -> Unit,
    onPickAudio: () -> Unit,
    onClose: () -> Unit,
) {
    val ctx = LocalContext.current
    var granted by remember { mutableStateOf(MEDIA_PERMS.all { ctx.hasPermission(it) }) }
    var items by remember { mutableStateOf<List<GalleryItem>>(emptyList()) }
    val runWithPerm = rememberPermissionRunner()
    val grid = rememberLazyGridState()
    // Галерея страницами (п. 15 с телефона): прежде показывались только последние 120.
    // Следующая — когда до конца загруженного осталось меньше двух экранов плиток.
    LaunchedEffect(granted) {
        if (!granted) return@LaunchedEffect
        var done = false
        snapshotFlow {
            val last = grid.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            last >= items.size - GALLERY_PAGE / 4
        }.collect { near ->
            if (!near || done) return@collect
            val page = loadGallery(ctx, from = items.size)
            if (page.size < GALLERY_PAGE) done = true
            items = items + page
        }
    }
    val gap = Tokens.Space.xs + Tokens.Space.xxs

    Column(
        Modifier.fillMaxWidth()
            .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
            .background(Tokens.Colors.group),
    ) {
        PanelGrip(onClose)
        Column(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.l, end = Tokens.Space.l, bottom = Tokens.Space.m),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            if (granted) {
                LazyVerticalGrid(
                    columns = GridCells.Fixed(3),
                    state = grid,
                    modifier = Modifier.fillMaxWidth().heightIn(max = Tokens.Size.attachGrid),
                    horizontalArrangement = Arrangement.spacedBy(gap),
                    verticalArrangement = Arrangement.spacedBy(gap),
                ) {
                    item { CameraTile(onCamera) }
                    items(items) { g ->
                        GalleryTile(g, selected.indexOfFirst { it.uri == g.uri }) { onToggle(g) }
                    }
                }
            } else {
                // Отказ — не тупик: объясняем, зачем, и даём кнопку. Камера, файл и аудио
                // работают и без галереи.
                Text(stringResource(R.string.attach_need_gallery), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                    PlateButton(stringResource(R.string.attach_camera), TablerIcons.Camera, Modifier.weight(1f), onClick = onCamera)
                    PlateButton(stringResource(R.string.attach_allow), modifier = Modifier.weight(1f)) {
                        runWithPerm(MEDIA_PERMS, R.string.attach_gallery_denied) { granted = true }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                PlateButton(stringResource(R.string.attach_file), TablerIcons.File, Modifier.weight(1f), onClick = onPickFile)
                PlateButton(stringResource(R.string.attach_audio), TablerIcons.Music, Modifier.weight(1f), onClick = onPickAudio)
            }
            // Вес — на виду: в сети без серверов размер решает, поедут байты сразу или
            // получателю прилетит карточка «скачать».
            if (selected.isNotEmpty()) {
                val n = selected.size
                val size = fmtSizeLong(selected.sumOf { it.size })
                Text(
                    if (selected.none { it.isVideo }) {
                        pluralStringResource(R.plurals.attach_picked_photos, n, n, size)
                    } else {
                        pluralStringResource(R.plurals.attach_picked_items, n, n, size)
                    },
                    color = Tokens.Colors.accent,
                    fontSize = Tokens.Font.body,
                )
            }
        }
    }
}

/** Плитка «Камера» — первая в сетке. */
@Composable
private fun CameraTile(onClick: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(Tokens.Radius.inner))
            .background(Tokens.Colors.raised).clickable(role = Role.Button, onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(TablerIcons.Camera, null, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconL))
        Spacer(Modifier.height(Tokens.Space.xs + Tokens.Space.xxs))
        Text(stringResource(R.string.attach_camera), color = Color.White, fontSize = Tokens.Font.body)
    }
}

/** Плитка галереи: превью и номер выбора; у видео — длительность. */
@Composable
private fun GalleryTile(g: GalleryItem, pickedAt: Int, onClick: () -> Unit) {
    val bmp = rememberThumb(g)
    val shape = RoundedCornerShape(Tokens.Radius.inner)
    Box(
        Modifier.fillMaxWidth().aspectRatio(1f).clip(shape).background(Tokens.Colors.avatar)
            .imageOutline(shape).toggleable(pickedAt >= 0, role = Role.Checkbox) { onClick() },
    ) {
        if (bmp != null) {
            Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        }
        // Номер выбора — видно порядок, в котором уйдут файлы. Светлая обводка
        // обязательна: без неё кружок теряется на светлом кадре.
        Box(
            Modifier.align(Alignment.TopEnd).padding(Tokens.Space.xs + Tokens.Space.xxs).size(Tokens.Size.check)
                .clip(CircleShape)
                .background(if (pickedAt >= 0) HoloAccent else Tokens.Colors.tileScrim)
                .then(if (pickedAt >= 0) Modifier else Modifier.border(1.5.dp, Color.White.copy(alpha = 0.85f), CircleShape)),
            contentAlignment = Alignment.Center,
        ) {
            if (pickedAt >= 0) {
                Text("${pickedAt + 1}", color = HoloAccentDark, fontSize = Tokens.Font.small, fontWeight = FontWeight.Bold)
            }
        }
        if (g.durationMs > 0) TileChip(fmtDuration(g.durationMs.toInt()), Alignment.BottomEnd)
    }
}

@Composable
private fun BoxScope.TileChip(text: String, align: Alignment) {
    Box(
        Modifier.align(align).padding(Tokens.Space.xs).clip(RoundedCornerShape(Tokens.Radius.badge))
            .background(Tokens.Colors.tileScrim).padding(horizontal = Tokens.Space.xs, vertical = 1.dp),
    ) {
        Text(text, color = Color.White, fontSize = Tokens.Font.caption)
    }
}
