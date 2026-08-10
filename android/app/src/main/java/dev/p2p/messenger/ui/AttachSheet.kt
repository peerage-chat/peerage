package dev.p2p.messenger.ui

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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Лист вложений (макет `docs/design/mockups/attach-sheet.html`).
 *
 * Выезжает ИЗ строки ввода, сама строка остаётся на месте: подпись пишется в обычном
 * поле сообщения. Лента галереи видна сразу — частый случай «последнее фото»
 * закрывается одним касанием, без похода в системный пикер.
 */

/** Снимок из галереи устройства. */
internal data class GalleryItem(
    val uri: Uri,
    val size: Long,
    val durationMs: Long, // 0 — картинка
    val isVideo: Boolean,
)

/** Крупнее этого вложение у получателя не качается само — он увидит карточку.
 *  Совпадает с `AUTO_DOWNLOAD_LIMIT` в движке; чип такого размера красим янтарём. */
private const val BIG_ATTACHMENT = 2L * 1024 * 1024

private val MEDIA_PERMS: Array<String> =
    if (Build.VERSION.SDK_INT >= 33) {
        arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO)
    } else {
        arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
    }

/** Прочитать последние N элементов галереи (картинки + видео) одним запросом. */
private suspend fun loadGallery(ctx: Context, limit: Int = 120): List<GalleryItem> = withContext(Dispatchers.IO) {
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
 * @param selected выбранные элементы в порядке выбора (номер на плитке = позиция)
 */
@Composable
internal fun AttachSheet(
    selected: List<GalleryItem>,
    onToggle: (GalleryItem) -> Unit,
    onCamera: () -> Unit,
    onPickFile: () -> Unit,
    onPickAudio: () -> Unit,
) {
    val ctx = LocalContext.current
    var granted by remember { mutableStateOf(MEDIA_PERMS.all { ctx.hasPermission(it) }) }
    var items by remember { mutableStateOf<List<GalleryItem>>(emptyList()) }
    val runWithPerm = rememberPermissionRunner()

    LaunchedEffect(granted) { if (granted) items = loadGallery(ctx) }

    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp))
            .background(HoloSurface),
    ) {
        Box(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.s), contentAlignment = Alignment.Center) {
            Box(Modifier.size(width = 36.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(HoloSecondary))
        }

        // Счётчик выбранного с суммарным весом: в сети без серверов размер решает,
        // поедут байты сразу или получателю прилетит карточка «скачать».
        if (selected.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().height(36.dp).padding(horizontal = Tokens.Space.l),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(
                        R.string.attach_selected,
                        selected.size,
                        fmtSizeLong(selected.sumOf { it.size }),
                    ),
                    color = HoloSecondary,
                    fontSize = 13.sp,
                )
            }
        }

        // Источники. Только то, что работает: неактивных ячеек тут быть не должно.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.xs),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            SourceTile(Icons.Filled.AttachFile, R.string.attach_file, Modifier.weight(1f), onPickFile)
            SourceTile(Icons.Filled.MusicNote, R.string.attach_audio, Modifier.weight(1f), onPickAudio)
        }

        if (!granted) {
            // Отказ — не тупик: объясняем, зачем, и даём кнопку. Файл при этом
            // по-прежнему можно отправить через системный пикер выше.
            Column(
                Modifier.fillMaxWidth().padding(Tokens.Space.l),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    stringResource(R.string.attach_need_gallery),
                    color = HoloSecondary,
                    fontSize = 13.sp,
                )
                Spacer(Modifier.height(Tokens.Space.m))
                AccentButton(stringResource(R.string.attach_allow)) {
                    runWithPerm(MEDIA_PERMS, R.string.attach_gallery_denied) { granted = true }
                }
            }
        } else {
            LazyVerticalGrid(
                columns = GridCells.Fixed(3),
                modifier = Modifier.fillMaxWidth().height(300.dp),
                horizontalArrangement = Arrangement.spacedBy(2.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                item { CameraTile(onCamera) }
                items(items) { g ->
                    GalleryTile(g, selected.indexOfFirst { it.uri == g.uri }) { onToggle(g) }
                }
            }
        }
    }
}

@Composable
private fun SourceTile(icon: ImageVector, labelRes: Int, modifier: Modifier, onClick: () -> Unit) {
    Column(
        modifier.clip(RoundedCornerShape(Tokens.Radius.tile)).background(Tokens.Colors.tile)
            .clickable { onClick() }.padding(vertical = Tokens.Space.s),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(23.dp))
        Spacer(Modifier.height(4.dp))
        Text(stringResource(labelRes), color = Color.White, fontSize = 11.sp)
    }
}

/** Плитка «Камера». Живой предпросмотр — задел на будущее (ui-completion-plan.md). */
@Composable
private fun CameraTile(onClick: () -> Unit) {
    Box(
        Modifier.fillMaxWidth().aspectRatio(1f).background(Tokens.Colors.tile).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(Icons.Filled.PhotoCamera, null, tint = HoloAccent, modifier = Modifier.size(26.dp))
            Spacer(Modifier.height(4.dp))
            Text(stringResource(R.string.attach_camera), color = HoloAccent, fontSize = 11.sp)
        }
    }
}

/** Плитка галереи: превью, номер выбора, вес и длительность. Скруглений нет —
 *  это галерея, а не набор карточек; кадры различает их содержимое, не рамка. */
@Composable
private fun GalleryTile(g: GalleryItem, pickedAt: Int, onClick: () -> Unit) {
    val bmp = rememberThumb(g)
    Box(
        Modifier.fillMaxWidth().aspectRatio(1f).background(Tokens.Colors.avatar).clickable { onClick() },
    ) {
        if (bmp != null) {
            Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        }
        if (pickedAt >= 0) {
            Box(Modifier.fillMaxSize().background(HoloAccent.copy(alpha = 0.18f)))
        }
        // Номер выбора — видно порядок, в котором уйдут файлы. Светлая обводка
        // обязательна: без неё кружок теряется на светлом кадре и выглядит
        // чёрной кляксой на тёмном.
        Box(
            Modifier.align(Alignment.TopEnd).padding(5.dp).size(22.dp)
                .clip(CircleShape)
                .background(if (pickedAt >= 0) HoloAccent else Tokens.Colors.tileScrim)
                .border(
                    2.dp,
                    if (pickedAt >= 0) HoloAccent else Color.White.copy(alpha = 0.85f),
                    CircleShape,
                ),
            contentAlignment = Alignment.Center,
        ) {
            if (pickedAt >= 0) {
                Text(
                    "${pickedAt + 1}",
                    color = HoloAccentDark,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                )
            }
        }
        TileChip(fmtSizeLong(g.size), Alignment.BottomStart, big = g.size >= BIG_ATTACHMENT)
        if (g.durationMs > 0) TileChip(fmtDuration(g.durationMs.toInt()), Alignment.BottomEnd)
    }
}

@Composable
private fun BoxScope.TileChip(text: String, align: Alignment, big: Boolean = false) {
    Box(
        Modifier.align(align).padding(4.dp).clip(RoundedCornerShape(Tokens.Radius.badge))
            .background(Tokens.Colors.tileScrim).padding(horizontal = 5.dp, vertical = 1.dp),
    ) {
        Text(text, color = if (big) WarnColor else Color.White, fontSize = 10.sp)
    }
}
