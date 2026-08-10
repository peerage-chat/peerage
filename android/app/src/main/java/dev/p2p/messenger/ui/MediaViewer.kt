package dev.p2p.messenger.ui

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
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.p2p.messenger.HoloSecondary
import dev.p2p.messenger.R
import dev.p2p.messenger.Tokens
import kotlin.math.abs

/**
 * Полноэкранный просмотр вложения — фото и видео.
 *
 * Экран сознательно «голый»: чёрный фон во весь экран, никаких карточек и
 * поверхностей. Здесь единственный предмет — сам снимок, и всё, что не он,
 * соревнуется с ним за внимание. Поэтому шапка появляется только по тапу.
 *
 * Жесты (как везде, где смотрят медиа, — им не учатся, их ожидают):
 * щипок — зум, двойной тап — 1× ↔ 2.5×, потянуть вниз — закрыть, тап — шапка.
 * Пока картинка увеличена, протяжка двигает её, а не закрывает экран: иначе
 * рассмотреть угол снимка было бы невозможно.
 */
@Composable
internal fun MediaViewer(
    bytes: ByteArray,
    mime: String,
    name: String,
    author: String,
    time: Long,
    onClose: () -> Unit,
) {
    val ctx = LocalContext.current
    val storagePerm = rememberPermissionRunner()
    val video = mime.startsWith("video/")
    var chromeShown by remember { mutableStateOf(true) }
    val chrome = chromeShown || video
    var scale by remember { mutableFloatStateOf(1f) }
    var offX by remember { mutableFloatStateOf(0f) }
    var offY by remember { mutableFloatStateOf(0f) }
    // Смещение «оттягивания» вниз. Фон гаснет вместе с ним — жест виден до того,
    // как отпустишь палец, и понятно, что произойдёт.
    var dragY by remember { mutableFloatStateOf(0f) }
    val dim = (1f - (abs(dragY) / 900f)).coerceIn(0.35f, 1f)
    val zoom by animateFloatAsState(scale, label = "zoom")

    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = dim)),
            contentAlignment = Alignment.Center,
        ) {
            val gestures = Modifier
                .pointerInput(video) {
                    detectTapGestures(
                        onTap = { chromeShown = !chromeShown },
                        onDoubleTap = {
                            // Второй тап возвращает в исходное — иначе из зума
                            // выбираться приходилось бы щипком двумя пальцами.
                            if (scale > 1f) {
                                scale = 1f
                                offX = 0f
                                offY = 0f
                            } else {
                                scale = 2.5f
                            }
                        },
                    )
                }
                .pointerInput(video) {
                    detectTransformGestures { _, pan, gestureZoom, _ ->
                        scale = (scale * gestureZoom).coerceIn(1f, 6f)
                        if (scale > 1f) {
                            offX += pan.x
                            offY += pan.y
                            dragY = 0f
                        } else {
                            // На единичном масштабе вертикальная протяжка — это
                            // «закрыть», а не панорама: двигать нечего.
                            offX = 0f
                            offY = 0f
                            dragY += pan.y
                            if (dragY > 220f) onClose()
                        }
                    }
                }

            if (video) {
                VideoSurface(bytes, name, Modifier.fillMaxSize().then(gestures))
            } else {
                val bmp = remember(bytes) {
                    runCatching {
                        android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
                            ?.asImageBitmap()
                    }.getOrNull()
                }
                if (bmp != null) {
                    Image(
                        bmp,
                        null,
                        Modifier.fillMaxSize()
                            .graphicsLayerZoom(zoom, offX, offY + dragY)
                            .then(gestures),
                        contentScale = ContentScale.Fit,
                    )
                }
            }

            // Шапка поверх снимка, а не над ним: отдельная полоса откусывала бы
            // высоту у самого изображения ради подписи, которую и так знают.
            AnimatedVisibility(
                chrome,
                enter = fadeIn() + slideInVertically { -it },
                exit = fadeOut() + slideOutVertically { -it },
                modifier = Modifier.align(Alignment.TopCenter),
            ) {
                Row(
                    Modifier.fillMaxWidth()
                        .background(Color.Black.copy(alpha = 0.55f))
                        .statusBarsPadding()
                        .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    BarIcon(Icons.Filled.Close, stringResource(R.string.common_cancel), onClick = onClose)
                    Spacer(Modifier.width(Tokens.Space.s))
                    Column(Modifier.weight(1f)) {
                        Text(
                            author,
                            color = Color.White,
                            fontSize = 15.sp,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(fmtViewerDate(time), color = HoloSecondary, fontSize = 12.sp)
                    }
                    BarIcon(Icons.Filled.Download, stringResource(R.string.viewer_save)) {
                        // До Android 10 запись в галерею требует разрешения; начиная
                        // с неё MediaStore кладёт файл в свою папку и без него.
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                            saveToGallery(ctx, bytes, mime, name)
                        } else {
                            storagePerm(
                                arrayOf(android.Manifest.permission.WRITE_EXTERNAL_STORAGE),
                                R.string.perm_denied_storage,
                            ) { saveToGallery(ctx, bytes, mime, name) }
                        }
                    }
                    BarIcon(Icons.Filled.Share, stringResource(R.string.viewer_share)) {
                        shareFile(ctx, bytes, mime, name)
                    }
                }
            }
        }
    }
}

/** Зум и сдвиг одним слоем: масштаб общий, поэтому и точка отсчёта общая. */
private fun Modifier.graphicsLayerZoom(scale: Float, x: Float, y: Float): Modifier =
    this.graphicsLayer {
        scaleX = scale
        scaleY = scale
        translationX = x
        translationY = y
    }

/**
 * Видео проигрывается ВНУТРИ приложения, а не отдаётся системному плееру: файл
 * из чата — чужая переписка, и выпускать его в сторонние приложения ради кнопки
 * «play» не за что. Байты кладём в приватный кэш, откуда их никто не читает.
 */
@Composable
private fun VideoSurface(bytes: ByteArray, name: String, modifier: Modifier) {
    val ctx = LocalContext.current
    val file = remember(bytes) {
        runCatching {
            java.io.File(ctx.cacheDir, "viewer").apply { mkdirs() }
                .let { java.io.File(it, name.ifBlank { "video" }) }
                .apply { writeBytes(bytes) }
        }.getOrNull()
    }
    var playing by remember { mutableStateOf(true) }
    var position by remember { mutableIntStateOf(0) }
    var duration by remember { mutableIntStateOf(0) }
    val view = remember { mutableStateOf<android.widget.VideoView?>(null) }
    DisposableEffect(file) {
        onDispose {
            view.value?.stopPlayback()
            file?.delete()
        }
    }
    // Положение читаем опросом: VideoView не сообщает о нём сам, а четыре раза в
    // секунду достаточно, чтобы полоса не дёргалась и не грела процессор.
    LaunchedEffect(playing) {
        while (playing) {
            view.value?.let {
                position = it.currentPosition
                duration = it.duration.coerceAtLeast(0)
            }
            kotlinx.coroutines.delay(250)
        }
    }
    Box(modifier, contentAlignment = Alignment.Center) {
        if (file != null) {
            AndroidView(
                factory = { c ->
                    android.widget.VideoView(c).apply {
                        setVideoPath(file.absolutePath)
                        // Зациклено: короткое видео из чата пересматривают, и жать
                        // «заново» после каждого прохода — работа на пустом месте.
                        setOnPreparedListener {
                            it.isLooping = true
                            duration = this.duration.coerceAtLeast(0)
                        }
                        start()
                        view.value = this
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
            // Пауза/продолжить по тапу — на самом видео, поверх поверхности.
            Box(
                Modifier.matchParentSize().pointerInput(Unit) {
                    detectTapGestures {
                        val v = view.value ?: return@detectTapGestures
                        if (v.isPlaying) v.pause() else v.start()
                        playing = v.isPlaying
                    }
                },
            )
            if (!playing) {
                // Значок на затемнении: кадр бывает светлым, и белый треугольник
                // на нём иначе теряется.
                Box(
                    Modifier.size(72.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.45f)),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(Icons.Filled.PlayArrow, null, tint = Color.White, modifier = Modifier.size(44.dp))
                }
            }
            if (duration > 0) {
                VideoProgress(position, duration, Modifier.align(Alignment.BottomCenter)) { ms ->
                    view.value?.seekTo(ms)
                    position = ms
                }
            }
        }
    }
}

/**
 * Полоса прогресса с перемоткой. Тап и протяжка по ней переносят воспроизведение
 * одинаково, а область касания намеренно выше самой линии: попадать пальцем в
 * три пикселя невозможно.
 */
@Composable
private fun VideoProgress(
    position: Int,
    duration: Int,
    modifier: Modifier,
    onSeek: (Int) -> Unit,
) {
    var width by remember { mutableIntStateOf(1) }
    Column(
        modifier.fillMaxWidth()
            .background(Color.Black.copy(alpha = 0.55f))
            .navigationBarsPadding()
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
    ) {
        Box(
            Modifier.fillMaxWidth()
                .height(24.dp)
                .onSizeChanged { width = it.width.coerceAtLeast(1) }
                .pointerInput(duration) {
                    detectTapGestures { p -> onSeek((p.x / width * duration).toInt().coerceIn(0, duration)) }
                }
                .pointerInput(duration) {
                    detectHorizontalDragGestures { change, _ ->
                        onSeek((change.position.x / width * duration).toInt().coerceIn(0, duration))
                    }
                },
            contentAlignment = Alignment.CenterStart,
        ) {
            Box(Modifier.fillMaxWidth().height(3.dp).background(Color.White.copy(alpha = 0.25f)))
            Box(
                Modifier.fillMaxWidth(position.toFloat() / duration)
                    .height(3.dp)
                    .background(Color.White),
            )
        }
        Text(
            stringResource(R.string.viewer_progress, fmtDuration(position), fmtDuration(duration)),
            color = Color.White.copy(alpha = 0.75f),
            fontSize = 11.sp,
        )
    }
}

/** Дата снимка словами: «сегодня в 14:03» читается быстрее, чем «10.08 14:03». */
@Composable
private fun fmtViewerDate(time: Long): String {
    val ms = time * 1000
    val clock = android.text.format.DateFormat.getTimeFormat(LocalContext.current).format(java.util.Date(ms))
    return if (android.text.format.DateUtils.isToday(ms)) {
        stringResource(R.string.viewer_today, clock)
    } else {
        val day = android.text.format.DateFormat.getMediumDateFormat(LocalContext.current).format(java.util.Date(ms))
        stringResource(R.string.viewer_on_date, day, clock)
    }
}

/**
 * Сохранить вложение в галерею. На Android 10+ это MediaStore без единого
 * разрешения; на более старых системах тот же путь требует записи во внешнее
 * хранилище, поэтому там сначала спрашивают.
 */
private fun saveToGallery(ctx: Context, bytes: ByteArray, mime: String, name: String) {
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
                val dir = if (video) android.os.Environment.DIRECTORY_MOVIES else android.os.Environment.DIRECTORY_PICTURES
                put(MediaStore.MediaColumns.RELATIVE_PATH, "$dir/${ctx.getString(R.string.app_name)}")
            }
        }
        val uri = ctx.contentResolver.insert(target, values) ?: return@runCatching false
        ctx.contentResolver.openOutputStream(uri)?.use { it.write(bytes) } ?: return@runCatching false
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
private fun shareFile(ctx: Context, bytes: ByteArray, mime: String, name: String) {
    runCatching {
        val dir = java.io.File(ctx.cacheDir, "shared").apply { mkdirs() }
        val f = java.io.File(dir, name.ifBlank { defaultName(mime.startsWith("video/")) }).apply { writeBytes(bytes) }
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
