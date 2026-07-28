@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
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
import androidx.compose.material.icons.filled.Download
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
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
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

    // Произвольный файл (любой mime) → sendMedia (kind file/voice по mime в движке).
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendUriAttachment(ctx, model, key, it, scope, "file") }
    }
    val audioPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendUriAttachment(ctx, model, key, it, scope, "audio") }
    }
    // Камера: снимок уходит тем же путём, что и выбранный из галереи.
    val camUri = remember { mutableStateOf<android.net.Uri?>(null) }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        if (ok) camUri.value?.let { sendUriAttachment(ctx, model, key, it, scope, "photo.jpg") }
    }
    val camPerm = rememberPermissionRunner()
    val openCamera = {
        camPerm(arrayOf(Manifest.permission.CAMERA), R.string.perm_denied_camera) {
            camUri.value = newCameraUri(ctx)
            camUri.value?.let { camera.launch(it) }
        }
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
                val next = msgs.getOrNull(i + 1)
                if (prev == null || dayKey(prev.time) != dayKey(m.time)) DateChip(dayLabel(m.time))
                // Склейка блоков: в DM «отправитель» — это сторона (я/собеседник).
                val side = { msg: Message -> if (msg.mine) "me" else "peer" }
                val pos = blockPos(
                    withPrev = prev != null && continuesBlock(side(prev), prev.time, side(m), m.time),
                    withNext = next != null && continuesBlock(side(m), m.time, side(next), next.time),
                )
                MessageBubble(m, model, pos)
            }
        }
        MessageInput(
            value = input,
            onChange = { input = it },
            onPickFile = { filePicker.launch(arrayOf("*/*")) },
            onPickAudio = { audioPicker.launch(arrayOf("audio/*")) },
            onCamera = openCamera,
            onSendAttachments = { list ->
                list.forEach { g ->
                    sendUriAttachment(ctx, model, key, g.uri, scope, if (g.isVideo) "video.mp4" else "photo.jpg")
                }
            },
            recording = recording,
            onVoiceStart = startVoice,
            onVoiceStop = stopVoice,
            model = model,
            onSend = {
                if (input.isNotBlank()) {
                    model.send(key, input)
                    input = ""
                }
            },
        )
    }
}

/** Файл для снимка с камеры в кэше приложения + content:// через FileProvider. */
internal fun newCameraUri(ctx: android.content.Context): android.net.Uri? = runCatching {
    val dir = java.io.File(ctx.cacheDir, "camera").apply { mkdirs() }
    val f = java.io.File(dir, "shot-${System.currentTimeMillis()}.jpg")
    androidx.core.content.FileProvider.getUriForFile(ctx, "${ctx.packageName}.fileprovider", f)
}.getOrNull()

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
        // Метаданные (длительность/кадр/превью) вытаскиваем здесь: движок медиа не
        // декодирует, а получателю нужна информативная карточка ДО скачивания.
        val meta = MediaMeta.extract(ctx, uri, mime, bytes)
        model.sendMediaWithMeta(key, bytes, mime, name, meta)
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
    onPickFile: () -> Unit = {},
    onPickAudio: () -> Unit = {},
    onCamera: () -> Unit = {},
    onSendAttachments: (List<GalleryItem>) -> Unit = {},
    recording: Boolean = false,
    onVoiceStart: () -> Unit = {},
    onVoiceStop: () -> Unit = {},
    model: AppModel? = null,
    onSend: () -> Unit,
) {
    // Поле держим как TextFieldValue: панели эмодзи нужна ПОЗИЦИЯ КУРСОРА, иначе
    // символ всегда падал бы в конец, даже если человек правит середину строки.
    var field by remember { mutableStateOf(TextFieldValue(value)) }
    if (field.text != value) {
        // Внешнее изменение (очистка после отправки) — синхронизируем, курсор в конец.
        field = TextFieldValue(value, TextRange(value.length))
    }
    val setField = { v: TextFieldValue ->
        field = v
        onChange(v.text)
    }
    var emojiOpen by remember { mutableStateOf(false) }
    var attachOpen by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf<List<GalleryItem>>(emptyList()) }

    Column(Modifier.fillMaxWidth().background(HoloBg)) {
        if (attachOpen) {
            AttachSheet(
                selected = picked,
                onToggle = { g ->
                    picked = if (picked.any { it.uri == g.uri }) {
                        picked.filterNot { it.uri == g.uri }
                    } else {
                        picked + g
                    }
                },
                onCamera = onCamera,
                onPickFile = onPickFile,
                onPickAudio = onPickAudio,
            )
        }
        if (emojiOpen) {
            EmojiPanel(
                recent = model?.recentEmoji ?: emptyList(),
                onPick = { e ->
                    val start = field.selection.start
                    val end = field.selection.end
                    val text = field.text.take(start) + e + field.text.substring(end)
                    setField(TextFieldValue(text, TextRange(start + e.length)))
                    model?.useEmoji(e)
                },
                onBackspace = {
                    val end = field.selection.end
                    if (field.selection.length > 0) {
                        val text = field.text.take(field.selection.start) + field.text.substring(end)
                        setField(TextFieldValue(text, TextRange(field.selection.start)))
                    } else if (end > 0) {
                        // Удаляем ГРАФЕМУ, а не char: эмодзи — суррогатная пара, часто с
                        // модификаторами; посимвольный забой рвал бы их на половинки.
                        val from = graphemeStart(field.text, end)
                        val text = field.text.take(from) + field.text.substring(end)
                        setField(TextFieldValue(text, TextRange(from)))
                    }
                },
            )
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Поле-пилюля: скрепка и эмодзи ВНУТРИ.
            Row(
                Modifier.weight(1f).clip(RoundedCornerShape(24.dp)).background(HoloSurface),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // Скрепка выдвигает лист вложений из этой же строки; эмодзи ведёт
                // себя так же. Активная подсвечена акцентом, повторный тап закрывает.
                PillIcon(
                    Icons.Filled.AttachFile,
                    stringResource(R.string.cd_attach),
                    tint = if (attachOpen) HoloAccent else HoloSecondary,
                ) {
                    attachOpen = !attachOpen
                    if (attachOpen) emojiOpen = false
                }
                BasicTextField(
                    value = field,
                    onValueChange = setField,
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
                // Скрепка и эмодзи ведут себя одинаково: обе выдвигают панель из этой же
                // строки, поэтому активная подсвечена акцентом и закрывается повторным тапом.
                PillIcon(
                    Icons.Filled.EmojiEmotions,
                    stringResource(R.string.cd_emoji),
                    tint = if (emojiOpen) HoloAccent else HoloSecondary,
                ) {
                    emojiOpen = !emojiOpen
                    if (emojiOpen) attachOpen = false
                }
            }
            Spacer(Modifier.width(8.dp))
            // Отправка (есть текст) / голосовое удержанием (поле пустое).
            val hasText = value.isNotBlank()
            val hasPicked = picked.isNotEmpty()
            val btnMod = if (hasText || hasPicked) {
                Modifier.clickable {
                    if (hasPicked) {
                        onSendAttachments(picked)
                        picked = emptyList()
                        attachOpen = false
                    }
                    if (hasText) onSend()
                }
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
                val send = hasText || hasPicked
                Icon(
                    if (send) Icons.AutoMirrored.Filled.Send else Icons.Filled.Mic,
                    stringResource(if (send) R.string.common_send else R.string.chat_voice),
                    tint = if (recording) Color.White else HoloAccentDark,
                    modifier = Modifier.size(20.dp),
                )
                // Счётчик того, сколько файлов уйдёт этим нажатием.
                if (hasPicked) {
                    Text(
                        "${picked.size}",
                        color = HoloAccentDark,
                        fontSize = 10.sp,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier.align(Alignment.TopEnd).padding(top = 2.dp, end = 5.dp),
                    )
                }
            }
        }
    }
}

/** Начало графемы, заканчивающейся в [end]: эмодзи занимает несколько char (суррогатная
 *  пара, вариационный селектор, ZWJ-последовательность), и забой обязан снимать её
 *  целиком, иначе в строке остаётся «половина символа». */
internal fun graphemeStart(text: String, end: Int): Int {
    val it = java.text.BreakIterator.getCharacterInstance()
    it.setText(text)
    val prev = it.preceding(end)
    return if (prev == java.text.BreakIterator.DONE) 0 else prev
}

/** Иконка внутри поля-пилюли (44px касание). */
@Composable
internal fun PillIcon(icon: ImageVector, desc: String, tint: Color = HoloSecondary, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(RoundedCornerShape(percent = 30)).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(22.dp)) }
}

@Composable
internal fun MessageBubble(m: Message, model: AppModel, pos: BlockPos = BlockPos.Solo) {
    // M2: байты вложения из реактивного кэша (появятся по докачке без рефреша).
    val blobBytes = if (m.blobId.isNotEmpty()) model.mediaBlobs[m.blobId] else null
    var viewer by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().padding(bottom = blockGap(pos)),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start,
    ) {
        Box(
            Modifier.widthIn(max = 290.dp).clip(bubbleShape(m.mine, pos))
                .background(if (m.mine) BubbleOut else BubbleIn)
                .padding(horizontal = 10.dp, vertical = 6.dp),
        ) {
            when {
                // Крупное вложение не тянется автоматически: сначала карточка с тем,
                // что известно из сообщения (имя/размер/длительность/превью), байты —
                // по кнопке. Решает получатель, а не отправитель.
                !m.downloaded && blobBytes == null && m.blobId.isNotEmpty() && m.kind != "text" ->
                    PendingAttachmentCard(m, model)
                else -> when (m.kind) {
                    "image" -> ImageAttachment(blobBytes) { viewer = true }
                    "voice" -> VoiceAttachment(blobBytes)
                    "file" -> FileAttachment(m.name, blobBytes)
                    else -> Text(
                        m.text,
                        color = Color.White,
                        fontSize = 15.sp,
                        modifier = Modifier.padding(end = bubbleTimeReserve(m.mine)),
                    )
                }
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

/** Компактная карточка ещё НЕ скачанного вложения: превью (если прислали), имя,
 *  тип·длительность·размер и кнопка загрузки.
 *
 *  Смысл — решение принимает получатель: пока он не нажал, байты не качаются, то есть
 *  чужое видео на 800 МБ не съедает его трафик само. Всё показанное здесь приходит в
 *  самом сообщении, до единого байта содержимого. */
@Composable
private fun PendingAttachmentCard(m: Message, model: AppModel) {
    val thumb = if (m.thumbId.isNotEmpty()) model.mediaBlobs[m.thumbId] else null
    // Состояние докачки живёт в движке, а не в этом composable: заказ персистентный,
    // продолжается сам при появлении пира и переживает перезапуск. Отсюда — только показ.
    val progress = model.downloads[m.blobId]
    Row(
        Modifier.padding(end = 44.dp).widthIn(min = 180.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(48.dp).clip(RoundedCornerShape(8.dp)).background(Color.Black.copy(alpha = 0.25f)),
            contentAlignment = Alignment.Center,
        ) {
            val bmp = remember(thumb) {
                thumb?.let { runCatching { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size) }.getOrNull() }
            }
            if (bmp != null) {
                Image(bmp.asImageBitmap(), null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
            } else {
                Icon(kindIcon(m.kind), null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            }
        }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f, fill = false)) {
            Text(
                m.name.ifBlank { stringResource(R.string.chat_file) },
                color = Color.White,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                buildList {
                    if (m.durationMs > 0) add(fmtDuration(m.durationMs))
                    if (m.size > 0) add(fmtSizeLong(m.size))
                }.joinToString(" · "),
                color = HoloSecondary,
                fontSize = 11.sp,
            )
        }
        Spacer(Modifier.width(8.dp))
        when {
            // Заказано, но чанки ещё не пошли (пир офлайн / едет манифест) — крутилка.
            progress == AppModel.DOWNLOAD_INIT ->
                CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
            // Пошли байты — тот же кружок, но заполняется: видно, что и насколько идёт.
            progress != null ->
                CircularProgressIndicator(
                    progress = progress,
                    color = HoloAccent,
                    modifier = Modifier.size(22.dp),
                    strokeWidth = 2.dp,
                )
            else -> IconButton(onClick = { model.currentChat?.let { model.downloadMedia(it, m.blobId) } }) {
                Icon(Icons.Filled.Download, stringResource(R.string.chat_download), tint = HoloAccent)
            }
        }
    }
}

private fun kindIcon(kind: String) = when (kind) {
    "image" -> Icons.Filled.Image
    "voice" -> Icons.Filled.Mic
    else -> Icons.Filled.AttachFile
}

internal fun fmtSizeLong(bytes: Long): String = when {
    bytes >= 1_048_576L * 1024 -> "%.1f GB".format(bytes / (1_048_576.0 * 1024))
    bytes >= 1_048_576 -> "%.1f MB".format(bytes / 1_048_576.0)
    bytes >= 1024 -> "%.0f KB".format(bytes / 1024.0)
    else -> "$bytes B"
}

private fun fmtSize(bytes: Int): String = when {
    bytes >= 1_048_576 -> "%.1f MB".format(bytes / 1_048_576.0)
    bytes >= 1024 -> "%.0f KB".format(bytes / 1024.0)
    else -> "$bytes B"
}

internal fun fmtDuration(ms: Int): String {
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
        // Только значок, без слова: строка времени в пузыре имеет фиксированный
        // резерв места (см. bubbleTimeReserve), а слово его переполняло и наезжало
        // на короткий текст. Расшифровка статуса — в описании для screen reader и
        // на экране очереди отправки.
        "queued" -> Icon(
            Icons.Filled.CloudQueue,
            stringResource(R.string.status_queued),
            tint = tint.copy(alpha = 0.7f),
            modifier = Modifier.size(12.dp),
        )
        "failed" -> Text("!", color = HoloError, fontSize = 11.sp, fontWeight = FontWeight.Bold)
    }
}
