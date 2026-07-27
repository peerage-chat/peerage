@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CloudQueue
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.EmojiEmotions
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

@Composable
internal fun ChatTopBar(model: AppModel, onBack: () -> Unit, onProfile: () -> Unit) {
    val key = model.currentChat
    val c = key?.let { k -> model.contacts.firstOrNull { it.key == k } }
    val name = c?.displayName ?: stringResource(R.string.chat_default_title)
    val online = key?.let { model.online[it] == true } ?: false
    val avatar = c?.avatarId?.let { model.contactAvatars[it] }
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).clickable { onProfile() }
                .padding(horizontal = 4.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(name, size = 38.dp, online = online, avatarBytes = avatar)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        name,
                        color = Color.White,
                        fontSize = 16.sp,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (c?.verified == true) {
                        Spacer(Modifier.width(4.dp))
                        Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(14.dp))
                    }
                }
                Text(
                    stringResource(if (online) R.string.status_in_network else R.string.status_offline),
                    color = if (online) HoloOnline else HoloSecondary,
                    fontSize = 11.sp,
                )
            }
        }
        if (key != null) {
            // Микрофон спрашиваем здесь, а не на старте приложения: пользователь жмёт
            // «позвонить» — смысл запроса очевиден.
            val runWithPerm = rememberPermissionRunner()
            BarIcon(Icons.Filled.Call, stringResource(R.string.cd_audio_call)) {
                runWithPerm(arrayOf(Manifest.permission.RECORD_AUDIO), R.string.perm_denied_mic) { model.callDm(key) }
            }
        }
        BarIcon(Icons.Filled.MoreVert, stringResource(R.string.common_more), tint = Color.White, onClick = onProfile)
    }
}

@Composable
internal fun ChatScreen(model: AppModel) {
    val key = model.currentChat ?: return
    val msgs = model.messages[key] ?: emptyList()
    var input by remember(key) { mutableStateOf("") }
    val listState = rememberLazyListState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    // Держим прокрутку у последнего сообщения при появлении новых.
    LaunchedEffect(msgs.size) { if (msgs.isNotEmpty()) listState.animateScrollToItem(msgs.size) }

    // M2: картинка → чтение байт в IO → sendMedia (scrub+блоб+сообщение в движке).
    val imagePicker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let { sendUriAttachment(ctx, model, key, it, scope, "image.jpg") }
    }
    // M2: произвольный файл (любой mime) → sendMedia (kind file/voice по mime в движке).
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendUriAttachment(ctx, model, key, it, scope, "file") }
    }

    // Голосовые (M2): held-to-record. RECORD_AUDIO спрашиваем при первой попытке.
    val recorder = remember { VoiceRecorder(ctx) }
    var recording by remember { mutableStateOf(false) }
    val micPerm = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    val startVoice = {
        if (ctx.hasPermission(Manifest.permission.RECORD_AUDIO)) {
            recording = recorder.start()
        } else {
            micPerm.launch(Manifest.permission.RECORD_AUDIO)
        }
    }
    val stopVoice = {
        if (recording) {
            recording = false
            recorder.stop()?.let { bytes ->
                scope.launch(Dispatchers.IO) { model.sendMedia(key, bytes, "audio/mp4", "voice.m4a") }
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
        ) {
            item { E2EBanner() }
            itemsIndexed(msgs) { i, m ->
                val prev = msgs.getOrNull(i - 1)
                if (prev == null || dayKey(prev.time) != dayKey(m.time)) DateChip(dayLabel(m.time))
                MessageBubble(m, model)
            }
        }
        MessageInput(
            value = input,
            onChange = { input = it },
            onPickImage = { imagePicker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
            onPickFile = { filePicker.launch(arrayOf("*/*")) },
            recording = recording,
            onVoiceStart = startVoice,
            onVoiceStop = stopVoice,
            onSend = {
                if (input.isNotBlank()) {
                    model.send(key, input)
                    input = ""
                }
            },
        )
    }
}

/** Прочитать содержимое uri (байты + mime + имя) в IO и отправить как вложение (M2). */
private fun sendUriAttachment(
    ctx: android.content.Context,
    model: AppModel,
    key: String,
    uri: android.net.Uri,
    scope: kotlinx.coroutines.CoroutineScope,
    fallbackName: String,
) {
    scope.launch(Dispatchers.IO) {
        val bytes = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
        val mime = ctx.contentResolver.getType(uri) ?: "application/octet-stream"
        val name = displayName(ctx, uri) ?: fallbackName
        model.sendMedia(key, bytes, mime, name)
    }
}

/** Человекочитаемое имя файла из content-uri (OpenableColumns.DISPLAY_NAME). */
internal fun displayName(ctx: android.content.Context, uri: android.net.Uri): String? =
    runCatching {
        ctx.contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull()

/** Системная строка «E2E» в начале переписки. */
@Composable
internal fun E2EBanner() {
    Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalArrangement = Arrangement.Center) {
        Row(
            Modifier.clip(RoundedCornerShape(8.dp)).background(HoloSurface)
                .padding(horizontal = 10.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Lock, null, tint = HoloSecondary, modifier = Modifier.size(12.dp))
            Spacer(Modifier.width(5.dp))
            Text(stringResource(R.string.chat_e2e_banner), color = HoloSecondary, fontSize = 11.sp)
        }
    }
}

/** Чип-разделитель даты. */
@Composable
internal fun DateChip(label: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
        Box(
            Modifier.clip(RoundedCornerShape(10.dp)).background(HoloSurface)
                .padding(horizontal = 10.dp, vertical = 3.dp),
        ) { Text(label, color = HoloSecondary, fontSize = 11.sp) }
    }
}

@Composable
internal fun MessageInput(
    value: String,
    onChange: (String) -> Unit,
    onPickImage: () -> Unit = {},
    onPickFile: () -> Unit = {},
    recording: Boolean = false,
    onVoiceStart: () -> Unit = {},
    onVoiceStop: () -> Unit = {},
    onSend: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().background(HoloBg).padding(horizontal = 8.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Поле-пилюля: скрепка и эмодзи ВНУТРИ.
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(24.dp)).background(HoloSurface),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Скрепка → меню «Фото / Файл» (M2).
            Box {
                var attachOpen by remember { mutableStateOf(false) }
                PillIcon(Icons.Filled.AttachFile, stringResource(R.string.cd_attach)) { attachOpen = true }
                DropdownMenu(expanded = attachOpen, onDismissRequest = { attachOpen = false }) {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.attach_photo)) },
                        leadingIcon = { Icon(Icons.Filled.Image, null) },
                        onClick = {
                            attachOpen = false
                            onPickImage()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.attach_file)) },
                        leadingIcon = { Icon(Icons.Filled.AttachFile, null) },
                        onClick = {
                            attachOpen = false
                            onPickFile()
                        },
                    )
                }
            }
            BasicTextField(
                value = value,
                onValueChange = onChange,
                modifier = Modifier.weight(1f).padding(vertical = 12.dp),
                textStyle = TextStyle(color = Color.White, fontSize = 15.sp),
                cursorBrush = SolidColor(HoloAccent),
                maxLines = 4,
                decorationBox = { inner ->
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (value.isEmpty()) {
                            Text(
                                stringResource(if (recording) R.string.chat_recording else R.string.chat_input_hint),
                                color = if (recording) HoloError else HoloSecondary,
                                fontSize = 15.sp,
                            )
                        }
                        inner()
                    }
                },
            )
            PillIcon(Icons.Filled.EmojiEmotions, stringResource(R.string.cd_emoji)) { /* TODO: эмодзи */ }
        }
        Spacer(Modifier.width(8.dp))
        // Отправка (есть текст) / голосовое удержанием (поле пустое).
        val hasText = value.isNotBlank()
        val btnMod = if (hasText) {
            Modifier.clickable { onSend() }
        } else {
            // Held-to-record: запись на нажатии, отправка на отпускании.
            Modifier.pointerInput(Unit) {
                detectTapGestures(onPress = {
                    onVoiceStart()
                    tryAwaitRelease()
                    onVoiceStop()
                })
            }
        }
        Box(
            Modifier.clip(RoundedCornerShape(percent = 30))
                .background(if (recording) HoloError else HoloAccent).size(44.dp).then(btnMod),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                if (hasText) Icons.AutoMirrored.Filled.Send else Icons.Filled.Mic,
                stringResource(if (hasText) R.string.common_send else R.string.chat_voice),
                tint = if (recording) Color.White else HoloAccentDark,
                modifier = Modifier.size(20.dp),
            )
        }
    }
}

/** Иконка внутри поля-пилюли (44px касание). */
@Composable
internal fun PillIcon(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(RoundedCornerShape(percent = 30)).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = HoloSecondary, modifier = Modifier.size(22.dp)) }
}

@Composable
internal fun MessageBubble(m: Message, model: AppModel) {
    // M2: байты вложения из реактивного кэша (появятся по докачке без рефреша).
    val blobBytes = if (m.blobId.isNotEmpty()) model.mediaBlobs[m.blobId] else null
    var viewer by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(vertical = 3.dp),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start,
    ) {
        Box(
            Modifier.widthIn(max = 290.dp).clip(RoundedCornerShape(14.dp))
                .background(if (m.mine) BubbleOut else BubbleIn)
                .padding(horizontal = 10.dp, vertical = 6.dp),
        ) {
            when (m.kind) {
                "image" -> ImageAttachment(blobBytes) { viewer = true }
                "voice" -> VoiceAttachment(blobBytes)
                "file" -> FileAttachment(m.name, blobBytes)
                else -> Text(
                    m.text,
                    color = Color.White,
                    fontSize = 15.sp,
                    modifier = Modifier.padding(end = if (m.mine) 52.dp else 40.dp),
                )
            }
            Row(
                Modifier.align(Alignment.BottomEnd),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(fmtMsgTime(m.time), color = Color.White.copy(alpha = 0.5f), fontSize = 10.sp)
                if (m.mine) {
                    Spacer(Modifier.width(3.dp))
                    StatusTick(m.status)
                }
            }
        }
    }
    if (viewer && blobBytes != null) ImageViewer(blobBytes) { viewer = false }
}

/** Картинка-вложение в пузыре: миниатюра (тап → полный экран) или плейсхолдер, пока
 *  байты докачиваются. */
@Composable
internal fun ImageAttachment(bytes: ByteArray?, onOpen: () -> Unit) {
    val bmp = bytes?.let {
        remember(it) { runCatching { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }.getOrNull() }
    }
    Box(
        Modifier.size(220.dp).clip(RoundedCornerShape(10.dp)).background(HoloSurface).clickable(enabled = bmp != null, onClick = onOpen),
        contentAlignment = Alignment.Center,
    ) {
        if (bmp != null) {
            Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        } else {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
        }
    }
}

/** Файл-вложение: иконка + имя + размер; тап (когда скачан) → открыть системой. */
@Composable
internal fun FileAttachment(name: String, bytes: ByteArray?) {
    val ctx = LocalContext.current
    Row(
        Modifier.padding(end = 44.dp).widthIn(min = 160.dp).clickable(enabled = bytes != null) { bytes?.let { openWithSystem(ctx, name, it) } },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (bytes == null) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
        } else {
            Icon(Icons.Filled.AttachFile, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.width(8.dp))
        Column {
            Text(name.ifBlank { stringResource(R.string.chat_file) }, color = Color.White, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (bytes != null) fmtSize(bytes.size) else stringResource(R.string.chat_downloading),
                color = HoloSecondary,
                fontSize = 11.sp,
            )
        }
    }
}

/** Голосовое-вложение: play/pause + длительность (M2). Плеер на байтах блоба. */
@Composable
internal fun VoiceAttachment(bytes: ByteArray?) {
    val ctx = LocalContext.current
    var playing by remember { mutableStateOf(false) }
    var durationMs by remember(bytes) { mutableStateOf(0) }
    val player = remember { mutableStateOf<android.media.MediaPlayer?>(null) }
    DisposableEffect(bytes) {
        onDispose {
            player.value?.release()
            player.value = null
        }
    }
    Row(
        Modifier.padding(end = 44.dp).widthIn(min = 150.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(38.dp).clip(RoundedCornerShape(percent = 50)).background(HoloAccent)
                .clickable(enabled = bytes != null) {
                    val mp = player.value
                    if (mp != null && mp.isPlaying) {
                        mp.pause()
                        playing = false
                    } else if (mp != null) {
                        mp.start()
                        playing = true
                    } else if (bytes != null) {
                        val f = java.io.File(ctx.cacheDir, "play_${bytes.size}.m4a").apply { writeBytes(bytes) }
                        val np = android.media.MediaPlayer().apply {
                            setDataSource(f.absolutePath)
                            prepare()
                            durationMs = duration
                            setOnCompletionListener {
                                playing = false
                                seekTo(0)
                            }
                            start()
                        }
                        player.value = np
                        playing = true
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            if (bytes == null) {
                CircularProgressIndicator(color = HoloAccentDark, modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
            } else {
                Icon(
                    if (playing) Icons.Filled.Pause else Icons.Filled.PlayArrow,
                    null,
                    tint = HoloAccentDark,
                    modifier = Modifier.size(22.dp),
                )
            }
        }
        Spacer(Modifier.width(10.dp))
        Icon(Icons.Filled.Mic, null, tint = HoloSecondary, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(6.dp))
        Text(
            if (durationMs > 0) fmtDuration(durationMs) else stringResource(R.string.chat_voice),
            color = Color.White,
            fontSize = 13.sp,
            modifier = Modifier.padding(end = 8.dp),
        )
    }
}

/** Открыть скачанный блоб системным приложением через FileProvider. */
private fun openWithSystem(ctx: android.content.Context, name: String, bytes: ByteArray) {
    runCatching {
        val dir = java.io.File(ctx.cacheDir, "shared").apply { mkdirs() }
        val f = java.io.File(dir, name.ifBlank { "file" }).apply { writeBytes(bytes) }
        val uri = androidx.core.content.FileProvider.getUriForFile(ctx, "${ctx.packageName}.fileprovider", f)
        val mime = ctx.contentResolver.getType(uri) ?: "*/*"
        val intent = android.content.Intent(android.content.Intent.ACTION_VIEW)
            .setDataAndType(uri, mime)
            .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
        ctx.startActivity(intent)
    }
}

private fun fmtSize(bytes: Int): String = when {
    bytes >= 1_048_576 -> "%.1f MB".format(bytes / 1_048_576.0)
    bytes >= 1024 -> "%.0f KB".format(bytes / 1024.0)
    else -> "$bytes B"
}

private fun fmtDuration(ms: Int): String {
    val s = ms / 1000
    return "%d:%02d".format(s / 60, s % 60)
}

/** Полноэкранный просмотр картинки (тап по фону — закрыть). */
@Composable
internal fun ImageViewer(bytes: ByteArray, onClose: () -> Unit) {
    val bmp = remember(bytes) { runCatching { android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() }.getOrNull() }
    Dialog(onDismissRequest = onClose) {
        Box(
            Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.92f)).clickable(onClick = onClose),
            contentAlignment = Alignment.Center,
        ) {
            if (bmp != null) Image(bmp, null, Modifier.fillMaxWidth(), contentScale = ContentScale.Fit)
        }
    }
}

/** Статус доставки исходящего сообщения — честно, без приукрашивания.
 *
 *  `queued` (получатель офлайн, сообщение ждёт в очереди) намеренно отличается от
 *  `pending` (уходит прямо сейчас) НЕ только иконкой, но и подписью: раньше оба
 *  рисовались одинаковыми часами, и «ждёт неделю» выглядело как «сейчас отправится».
 *  Галочка `sent` — доставка НЕ подтверждена; ✓✓ ставится только по Ack. */
@Composable
internal fun StatusTick(status: String) {
    val tint = Color.White.copy(alpha = 0.7f)
    when (status) {
        "delivered" -> Icon(Icons.Filled.DoneAll, stringResource(R.string.status_delivered), tint = tint, modifier = Modifier.size(13.dp))
        "sent" -> Icon(Icons.Filled.Done, stringResource(R.string.status_sent), tint = tint, modifier = Modifier.size(13.dp))
        "pending" -> Icon(Icons.Filled.Schedule, stringResource(R.string.status_sending), tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(12.dp))
        "queued" -> Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.status_queued_label),
                color = tint.copy(alpha = 0.7f),
                fontSize = 9.sp,
            )
            Spacer(Modifier.width(2.dp))
            Icon(
                Icons.Filled.CloudQueue,
                stringResource(R.string.status_queued),
                tint = tint.copy(alpha = 0.7f),
                modifier = Modifier.size(12.dp),
            )
        }
        "failed" -> Text("!", color = HoloError, fontSize = 11.sp, fontWeight = FontWeight.Bold)
    }
}
