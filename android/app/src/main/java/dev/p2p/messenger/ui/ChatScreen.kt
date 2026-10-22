@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package dev.p2p.messenger.ui

import android.Manifest
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Reply
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.CloudQueue
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Edit
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
import androidx.compose.ui.graphics.graphicsLayer
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
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

@Composable
internal fun ChatTopBar(model: AppModel, onBack: () -> Unit, onProfile: () -> Unit) {
    // Выделение забирает верхнюю панель целиком: пока что-то выбрано, «назад» и
    // профиль не нужны, а нужен выход из режима и действия над выбранным.
    if (model.selection.isNotEmpty()) {
        SelectionTopBar(model)
        return
    }
    val key = model.currentChat
    val c = key?.let { k -> model.contacts.firstOrNull { it.key == k } }
    val name = c?.displayName ?: stringResource(R.string.chat_default_title)
    val online = key?.let { model.online[it] == true } ?: false
    val avatar = c?.avatarId?.let { model.contactAvatars[it] }
    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(56.dp).padding(horizontal = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { onProfile() }
                .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(
                name,
                size = Tokens.Size.avatarHead,
                online = online,
                avatarBytes = avatar,
                circle = true, // собеседник — человек
                ring = HoloBg,
            )
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
    val msgs = model.chat(key)
    var input by remember(key) { mutableStateOf("") }
    val listState = rememberLazyListState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    // Лента привязана к низу (reverseLayout): элемент 0 — самое свежее, чат с первого
    // кадра открыт на конце. На НОВОЕ сообщение доводим до низа. Ориентир — id
    // последнего, а не размер списка: догрузка старых тоже меняет размер.
    val lastId = msgs.lastOrNull()?.id
    LaunchedEffect(lastId) { if (msgs.isNotEmpty()) listState.animateScrollToItem(0) }
    // «Показать в чате» из просмотрщика: доводим ленту до сообщения, если оно в
    // уже загруженной странице истории. Индекс в ленте — от конца.
    LaunchedEffect(model.jumpTo) {
        val target = model.jumpTo
        if (target != 0L) {
            val i = msgs.indexOfFirst { it.id == target }
            if (i >= 0) listState.animateScrollToItem(msgs.size - 1 - i)
            model.jumpTo = 0L
        }
    }
    PagingOlder(listState, key) { model.loadOlder(key, {}) }
    // Правка начинается с ТЕКУЩЕГО текста: человек исправляет, а не набирает заново.
    LaunchedEffect(model.editTarget) { model.editTarget?.let { input = it.text } }
    LaunchedEffect(key) { model.refreshPins(key, group = false) }
    // Порог анимации: всё, что было в чате на момент открытия, показываем сразу.
    // Анимировать историю при входе — мельтешение; анимируем только НОВОЕ.
    val appearFrom = remember(key) { msgs.lastOrNull()?.id ?: Long.MIN_VALUE }

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
        PinnedBar(model, key, group = false, canUnpin = true)
        LazyColumn(
            state = listState,
            reverseLayout = true,
            modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
        ) {
            // Порядок элементов — от свежего к старому; i — индекс в хронологии.
            // Ключ — пара «автор + id»: состояние пузыря (просмотрщик, голосовое) и
            // позиция прокрутки держатся за сообщение, а не за место в ленте.
            items(msgs.size, key = { j -> msgs[msgs.size - 1 - j].let { "${it.sender}:${it.id}" } }) { j ->
                val i = msgs.size - 1 - j
                val m = msgs[i]
                val prev = msgs.getOrNull(i - 1)
                val next = msgs.getOrNull(i + 1)
                // Одна колонка на элемент: дата над пузырём должна остаться НАД ним и
                // в перевёрнутой ленте.
                Column {
                    if (prev == null || dayKey(prev.time) != dayKey(m.time)) DateChip(dayLabel(m.time))
                    // Склейка блоков: в DM «отправитель» — это сторона (я/собеседник).
                    val side = { msg: Message -> if (msg.mine) "me" else "peer" }
                    val pos = blockPos(
                        withPrev = prev != null && continuesBlock(side(prev), prev.time, side(m), m.time),
                        withNext = next != null && continuesBlock(side(m), m.time, side(next), next.time),
                    )
                    MessageRow(model, m, pos, animate = m.id > appearFrom)
                }
            }
            // Последний элемент = верх ленты: баннер над самой старой перепиской.
            item { E2EBanner() }
        }
        ForwardSheet(model)
        MessageInput(
            value = input,
            onChange = { input = it },
            onPickFile = { filePicker.launch(arrayOf("*/*")) },
            onPickAudio = { audioPicker.launch(arrayOf("audio/*")) },
            onCamera = openCamera,
            onSendAttachments = { list, caption ->
                // Подпись прикрепляем к ПЕРВОМУ вложению: серия из пяти фото с одной
                // подписью на каждом читалась бы как спам.
                list.forEachIndexed { i, g ->
                    sendUriAttachment(
                        ctx,
                        model,
                        key,
                        g.uri,
                        scope,
                        if (g.isVideo) "video.mp4" else "photo.jpg",
                        caption = if (i == 0) caption else "",
                    )
                }
            },
            recording = recording,
            onVoiceStart = startVoice,
            onVoiceStop = stopVoice,
            model = model,
            replyStrip = {
                model.editTarget?.let {
                    EditStrip(it.text) {
                        model.editTarget = null
                        input = ""
                    }
                }
                    ?: model.replyTarget?.let { ReplyStrip(model, it) }
            },
            onSend = {
                val editing = model.editTarget
                if (editing != null) {
                    if (input.isNotBlank()) model.applyEdit(key, false, editing.id, input)
                    model.editTarget = null
                    input = ""
                } else if (input.isNotBlank()) {
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
internal fun sendUriAttachment(
    ctx: android.content.Context,
    model: AppModel,
    key: String,
    uri: android.net.Uri,
    scope: kotlinx.coroutines.CoroutineScope,
    fallbackName: String,
    caption: String = "",
    group: Boolean = false,
) {
    scope.launch(Dispatchers.IO) {
        val mime = ctx.contentResolver.getType(uri) ?: "application/octet-stream"
        val name = displayName(ctx, uri) ?: fallbackName
        // Картинки идут через память: им нужен scrub (EXIF/GPS), а он требует всего
        // файла целиком. Весят они немного, так что это допустимо.
        if (mime.startsWith("image/")) {
            val bytes = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
            val meta = MediaMeta.extract(ctx, uri, mime, bytes)
            model.sendMediaWithMeta(key, bytes, mime, name, meta, caption, group)
            return@launch
        }
        // Всё остальное — ПОТОКОМ: сначала переливаем content:// в кэш-файл по 64 КБ,
        // потом движок считает манифест и отдаёт чанки с диска. Видео на гигабайт не
        // должно стоить гигабайта RAM только потому, что его выбрали в пикере.
        val cached = cacheFromUri(ctx, uri, name) ?: return@launch
        val meta = MediaMeta.extract(ctx, uri, mime, ByteArray(0))
        model.sendMediaFile(key, cached.absolutePath, mime, name, meta, caption, group)
    }
}

/** Перелить content:// в файл кэша порциями. Возвращает файл или null. */
internal fun cacheFromUri(ctx: android.content.Context, uri: android.net.Uri, name: String): java.io.File? =
    runCatching {
        val dir = java.io.File(ctx.cacheDir, "outgoing").apply { mkdirs() }
        val f = java.io.File(dir, "${System.currentTimeMillis()}-$name")
        ctx.contentResolver.openInputStream(uri)?.use { input ->
            f.outputStream().use { out -> input.copyTo(out, 64 * 1024) }
        } ?: return null
        f
    }.getOrNull()

/** Человекочитаемое имя файла из content-uri (OpenableColumns.DISPLAY_NAME). */
internal fun displayName(ctx: android.content.Context, uri: android.net.Uri): String? =
    runCatching {
        ctx.contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull()

/**
 * Цитата в пузыре. `src == null` — оригинал уже удалён (у себя или отозван): пишем
 * это прямо, а не рисуем пустую полоску, иначе ответ выглядит сломанным.
 */
@Composable
internal fun QuoteBlock(
    model: AppModel,
    src: Message?,
    quotedMine: Boolean,
    modifier: Modifier = Modifier,
) {
    // Автор цитаты: своё — «Вы», чужое в группе — имя участника, чужое в личном —
    // имя собеседника. Ключ автора у цитаты есть всегда (ADR-006), поэтому имя
    // ищется по нему, а не по тому, чей это чат.
    val who = when {
        quotedMine -> stringResource(R.string.chat_reply_you)
        src != null && src.sender.isNotEmpty() -> groupSenderName(model, src.sender)
        else -> model.contacts.firstOrNull { it.key == model.currentChat }?.displayName
            ?: stringResource(R.string.chat_default_title)
    }
    // Плашка цитаты живёт ВНУТРИ цветного пузыря, поэтому и подложка, и текст —
    // полупрозрачный белый: серый (`HoloSecondary`) на синем читается пятном чужого
    // слоя, а 6% белого на нём почти не видно — плашка казалась случайной рамкой.
    Row(
        modifier.padding(bottom = 4.dp).height(IntrinsicSize.Min).heightIn(min = 30.dp)
            .clip(RoundedCornerShape(Tokens.Radius.chip))
            .background(Color.White.copy(alpha = 0.10f)),
    ) {
        Box(Modifier.width(2.5.dp).fillMaxHeight().background(HoloAccent))
        // Миниатюра слева от подписи — если она у нас есть.
        val hasThumb = src != null && src.kind != "text" &&
            QuoteThumb(model, src.blobId, src.thumbId, src.kind)
        Column(Modifier.padding(horizontal = 9.dp, vertical = 4.dp)) {
            Text(who, color = HoloAccent, fontSize = 12.sp, fontWeight = FontWeight.Medium)
            Text(
                src?.let {
                    when {
                        it.text.isNotEmpty() -> it.text
                        // С миниатюрой имя файла лишнее — картинка уже сказала, что это.
                        hasThumb -> quoteKind(it.kind)
                        else -> it.name.ifEmpty { quoteKind(it.kind) }
                    }
                } ?: stringResource(R.string.chat_reply_gone),
                color = Color.White.copy(alpha = 0.7f),
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Как назвать вложение без имени в цитате. */
@Composable
internal fun quoteKind(kind: String): String = stringResource(
    when (kind) {
        "image" -> R.string.quote_kind_image
        "video" -> R.string.quote_kind_video
        "voice" -> R.string.quote_kind_voice
        else -> R.string.quote_kind_file
    },
)

/** Полоска «отвечаем на …» над полем ввода. */
@Composable
internal fun ReplyStrip(model: AppModel, m: Message) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloSurface).padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(2.dp).height(30.dp).background(HoloAccent))
        Column(Modifier.weight(1f).padding(horizontal = 8.dp)) {
            // Кому отвечаем: своё — «Вы», чужое — имя автора по его ключу. В личном
            // чате это собеседник, в группе — конкретный участник; отдельная полоска
            // для группы отличалась ровно этой строкой.
            Text(
                if (m.mine) stringResource(R.string.chat_reply_you) else groupSenderName(model, m.sender),
                color = HoloAccent,
                fontSize = 12.sp,
                fontWeight = FontWeight.Medium,
            )
            Text(
                if (m.text.isNotEmpty()) m.text else m.name.ifEmpty { quoteKind(m.kind) },
                color = HoloSecondary,
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Icon(
            Icons.Filled.Close,
            stringResource(R.string.common_cancel),
            tint = HoloSecondary,
            modifier = Modifier.size(20.dp).clickable { model.cancelReply() },
        )
    }
}

/** Системная строка «E2E» в начале переписки. */
@Composable
internal fun E2EBanner() {
    Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalArrangement = Arrangement.Center) {
        Row(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloSurface)
                .padding(horizontal = 10.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Lock, null, tint = HoloSecondary, modifier = Modifier.size(12.dp))
            Spacer(Modifier.width(5.dp))
            Text(stringResource(R.string.chat_e2e_banner), color = HoloSecondary, fontSize = 11.sp)
        }
    }
}

/**
 * Панель действий над выделенными сообщениями.
 *
 * Ответ доступен только когда выбрано ОДНО: ответ на пять сообщений сразу — не
 * действие, а вопрос «на какое из них», и отвечать за пользователя тут нельзя.
 */
@Composable
internal fun SelectionTopBar(model: AppModel, group: Boolean = false) {
    val key = (if (group) model.currentGroup else model.currentChat) ?: return
    val ctx = LocalContext.current
    val sel = model.selection
    var confirmDelete by remember { mutableStateOf(false) }
    // «Назад» в режиме выделения снимает выделение, а не уводит из чата.
    BackHandler { model.clearSelection() }

    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.Filled.Close, stringResource(R.string.common_cancel)) { model.clearSelection() }
        Text(
            sel.size.toString(),
            color = Color.White,
            fontSize = 18.sp,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f).padding(start = 8.dp),
        )
        if (sel.size == 1) {
            BarIcon(Icons.AutoMirrored.Filled.Reply, stringResource(R.string.chat_reply)) {
                val ref = sel.first()
                model.chat(key, group).firstOrNull { it.id == ref.id && it.sender == ref.sender }
                    ?.let { model.startReply(it) }
            }
        }
        BarIcon(Icons.AutoMirrored.Filled.Send, stringResource(R.string.forward_action)) {
            model.beginForward(key, group)
        }
        if (sel.size == 1 && model.isMine(sel.first())) {
            val ref = sel.first()
            BarIcon(Icons.Filled.Edit, stringResource(R.string.chat_edit)) {
                model.chat(key, group).firstOrNull { it.id == ref.id && it.mine }
                    ?.let { model.editTarget = it }
                model.clearSelection()
            }
        }
        val mayPin = !group || model.groups.firstOrNull { it.gid == key }?.admin == true
        if (sel.size == 1 && mayPin) {
            val ref = sel.first()
            val msg = model.chat(key, group).firstOrNull { it.id == ref.id && it.sender == ref.sender }
            val already = model.pinned[key].orEmpty().any { it.id == ref.id && it.sender == ref.sender }
            BarIcon(TablerIcons.Pin, stringResource(R.string.pin_action)) {
                msg?.let { model.setPinned(key, group, it, !already) }
                model.clearSelection()
            }
        }
        BarIcon(Icons.Filled.ContentCopy, stringResource(R.string.chat_copy)) {
            val text = model.selectedTextOf(key, group)
            if (text.isNotEmpty()) {
                val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                    as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("p2p", text))
                android.widget.Toast
                    .makeText(ctx, R.string.chat_copied, android.widget.Toast.LENGTH_SHORT).show()
            }
            model.clearSelection()
        }
        BarIcon(Icons.Filled.Delete, stringResource(R.string.chat_delete)) { confirmDelete = true }
    }

    if (confirmDelete) {
        val allMine = sel.all { model.isMine(it) }
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(R.string.chat_delete_title, sel.size)) },
            // Честно про границу: отзыв — просьба, а не гарантия. Обещать «удалено у
            // всех» нельзя: собеседник мог прочитать сообщение или снять экран раньше.
            text = {
                Text(
                    stringResource(
                        if (allMine) R.string.chat_delete_note_all else R.string.chat_delete_note_foreign,
                    ),
                )
            },
            confirmButton = {
                Column(horizontalAlignment = Alignment.End) {
                    if (allMine) {
                        TextButton(onClick = {
                            model.retractSelected(key, group)
                            confirmDelete = false
                        }) { Text(stringResource(R.string.chat_delete_for_all), color = HoloError) }
                    }
                    TextButton(onClick = {
                        model.deleteSelected(key, group)
                        confirmDelete = false
                    }) { Text(stringResource(R.string.chat_delete_for_me), color = HoloError) }
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmDelete = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}

/** Чип-разделитель даты. */
@Composable
internal fun DateChip(label: String) {
    // Тот же ярлык, что у системных событий («Артём добавил Веру»): и то и другое
    // стоит вне диалога и комментирует ленту — значит и выглядеть должно одинаково.
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
        Box(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.labelScrim)
                .padding(horizontal = 11.dp, vertical = 3.dp),
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
    onSendAttachments: (List<GalleryItem>, String) -> Unit = { _, _ -> },
    recording: Boolean = false,
    onVoiceStart: () -> Unit = {},
    onVoiceStop: () -> Unit = {},
    model: AppModel? = null,
    /** Полоска «отвечаем на …» над полем: экран решает, чья цель ответа. */
    replyStrip: (@Composable () -> Unit)? = null,
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
    // Открытую панель «назад» закрывает, а не уводит из чата.
    BackHandler(enabled = emojiOpen || attachOpen) {
        emojiOpen = false
        attachOpen = false
    }

    Column(Modifier.fillMaxWidth().background(HoloBg)) {
        // Цитата видна ровно там, где человек печатает ответ: иначе легко забыть,
        // что сообщение уйдёт ответом, и получить «ответ» не на то.
        replyStrip?.invoke()
        // Панель ВЫЕЗЖАЕТ из строки ввода: движение объясняет, откуда она взялась.
        // Появление «из ниоткуда» читается как подмена экрана.
        //
        // Анимируется ВЫСОТА (expand/shrink), а не только сдвиг содержимого: со
        // сдвигом место под панель выделялось сразу целиком, и лента прыгала вверх
        // мгновенно, а меню догоняло плавно. Содержимое прижато к низу растущей
        // области — поэтому оно по-прежнему выезжает из строки ввода, а лента едет
        // вместе с ним.
        val panelEnter = expandVertically(tween(Tokens.Anim.normal), expandFrom = Alignment.Bottom) +
            fadeIn(tween(Tokens.Anim.normal))
        val panelExit = shrinkVertically(tween(Tokens.Anim.normal), shrinkTowards = Alignment.Bottom) +
            fadeOut(tween(Tokens.Anim.fast))
        AnimatedVisibility(
            visible = attachOpen,
            enter = panelEnter,
            exit = panelExit,
        ) {
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
        AnimatedVisibility(
            visible = emojiOpen,
            enter = panelEnter,
            exit = panelExit,
        ) {
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
                onClose = { emojiOpen = false },
            )
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Поле ввода: скрепка и эмодзи ВНУТРИ. Скругление такое же, как у кнопок
            // и карточек — пилюля выбивалась из общего языка форм.
            Row(
                Modifier.weight(1f).clip(RoundedCornerShape(Tokens.Radius.button)).background(HoloSurface),
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
                        // Текст поля становится ПОДПИСЬЮ вложения, а не отдельным
                        // сообщением: иначе фото с подписью — это два пакета, и
                        // наблюдатель узнаёт их по характерной паре (M5).
                        onSendAttachments(picked, value.trim())
                        picked = emptyList()
                        attachOpen = false
                        setField(TextFieldValue(""))
                    } else if (hasText) {
                        onSend()
                    }
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
                Modifier.clip(RoundedCornerShape(Tokens.Radius.button))
                    .background(if (recording) HoloError else HoloAccent)
                    .size(Tokens.Size.inputField).then(btnMod),
                contentAlignment = Alignment.Center,
            ) {
                val send = hasText || hasPicked
                Crossfade(send, animationSpec = tween(Tokens.Anim.fast), label = "send") { s ->
                    Icon(
                        if (s) Icons.AutoMirrored.Filled.Send else Icons.Filled.Mic,
                        stringResource(if (s) R.string.common_send else R.string.chat_voice),
                        tint = if (recording) Color.White else HoloAccentDark,
                        modifier = Modifier.size(20.dp),
                    )
                }
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
        Modifier.size(44.dp).clip(CircleShape).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(22.dp)) }
}

/**
 * Строка ленты: системный ярлык, своё сообщение или чужое с аватаром.
 *
 * Одна на личный чат и группу. Различий ровно два: в группе у входящих есть
 * аватар и имя автора (собеседников много), а системные строки бывают только
 * там. Всё остальное — тот же пузырь.
 */
@Composable
internal fun MessageRow(
    model: AppModel,
    m: Message,
    pos: BlockPos = BlockPos.Solo,
    group: Boolean = false,
    animate: Boolean = false,
) {
    when {
        m.system -> {
            val who = groupSenderName(model, m.sender)
            val text = if (m.text.isBlank()) {
                stringResource(R.string.group_system_membership, who)
            } else {
                stringResource(R.string.group_system_event, who, m.text)
            }
            // Ярлык того же вида, что разделитель даты: оба комментируют ленту
            // со стороны, а не участвуют в разговоре.
            Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
                Box(
                    Modifier.clip(RoundedCornerShape(Tokens.Radius.chip))
                        .background(Tokens.Colors.labelScrim)
                        .padding(horizontal = 11.dp, vertical = 3.dp),
                ) {
                    Text(text, color = HoloSecondary, fontSize = 11.sp)
                }
            }
        }
        // Входящее в группе: аватар — ТОЛЬКО у нижнего сообщения блока, у остальных
        // на его месте распорка, чтобы левый край блока не гулял. Имя автора — на
        // первом сообщении блока, внутри пузыря.
        group && !m.mine -> Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Start,
            verticalAlignment = Alignment.Bottom,
        ) {
            val who = groupSenderName(model, m.sender)
            val avatarBottom = pos == BlockPos.Solo || pos == BlockPos.Last
            if (avatarBottom) {
                SquareAvatar(
                    who,
                    size = Tokens.Size.avatarSeries,
                    online = model.online[m.sender] == true,
                    circle = true,
                    ring = HoloBg,
                )
            } else {
                Spacer(Modifier.width(Tokens.Size.avatarSeries))
            }
            Spacer(Modifier.width(8.dp))
            MessageBubble(
                m,
                model,
                pos,
                animate,
                senderName = if (pos == BlockPos.Solo || pos == BlockPos.First) who else null,
                senderColor = nameColor(m.sender),
            )
        }
        else -> MessageBubble(m, model, pos, animate)
    }
}

@Composable
internal fun MessageBubble(
    m: Message,
    model: AppModel,
    pos: BlockPos = BlockPos.Solo,
    animate: Boolean = false,
    /** Имя автора внутри пузыря — только в группе и только на первом сообщении
     *  блока: в личном чате собеседник один и подписывать его нечем. */
    senderName: String? = null,
    senderColor: Color = Color.White,
) {
    // Байты в память берём только у фото и голосового: видео рисуется миниатюрой,
    // файл — именем и размером из сообщения. Иначе прокрутка чата с видео тянула
    // бы в кучу файлы целиком ради одного кадра.
    val needsBytes = m.kind == "image" || m.kind == "voice"
    val blobBytes = if (needsBytes) model.blob(m.blobId) else null
    val ready = m.downloaded || model.savedBlobs[m.blobId] == true
    var viewer by remember { mutableStateOf(false) }
    // Новое сообщение всплывает снизу: видно, что оно ПРИШЛО, а не было всегда.
    var shown by remember(m.id) { mutableStateOf(!animate) }
    LaunchedEffect(m.id) { shown = true }
    val appear by animateFloatAsState(
        if (shown) 1f else 0f,
        animationSpec = tween(Tokens.Anim.normal),
        label = "bubble",
    )
    val selecting = model.selection.isNotEmpty()
    val selected = MsgRef(m.id, m.sender) in model.selection
    // Мета одна на все ветки пузыря: и когда она встроена в текст, и когда лежит
    // поверх вложения — это один и тот же набор значков.
    val meta: @Composable () -> Unit = {
        MsgMeta(m.time, m.mine && m.status.isNotEmpty(), m.editedAt > 0, m.status)
    }
    Row(
        Modifier.fillMaxWidth().padding(bottom = blockGap(pos))
            .graphicsLayer {
                alpha = appear
                translationY = (1f - appear) * 24f
            }
            // Подсветка на ВСЮ строку, а не на пузырь: так видно, что выбрано
            // сообщение целиком, включая вложение и подпись.
            .background(if (selected) HoloAccent.copy(alpha = 0.16f) else Color.Transparent),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start,
    ) {
        // Карточка «скачать» превращается в готовое вложение — это подмена
        // содержимого прямо под пальцем, поэтому переход плавный.
        val pending = m.blobId.isNotEmpty() && m.kind != "text" &&
            if (needsBytes) !m.downloaded && blobBytes == null else !ready
        // Фото без подписи — САМО себе пузырь: рамка вокруг картинки не несёт
        // смысла, только съедает 10dp с каждой стороны и делает снимок меньше.
        // Подпись возвращает подложку — под текстом она нужна. А вот цитата и
        // пометка пересылки НЕ возвращают: они уходят шапкой НАД картинкой, на
        // своей подложке, и сама картинка остаётся голой (так в Telegram).
        val photoBubble = (m.kind == "image" || m.kind == "video") && !pending && m.text.isEmpty()
        // Шапка над фото: пометка пересылки и цитата ответа.
        val photoHeader = photoBubble &&
            (m.fwdFrom.isNotEmpty() || m.fwdAnon || m.replyTo != 0L || senderName != null)
        Box(
            Modifier.widthIn(max = 290.dp).clip(bubbleShape(m.mine, pos))
                .then(
                    if (photoBubble) Modifier else Modifier.background(if (m.mine) BubbleOut else BubbleIn),
                )
                .then(
                    if (selected) {
                        Modifier.border(1.dp, HoloAccent, bubbleShape(m.mine, pos))
                    } else {
                        Modifier
                    },
                )
                // Длинный тап — вход в режим выделения; в режиме короткий тап
                // переключает, а не открывает вложение (иначе выбор был бы ловушкой).
                .combinedClickable(
                    onLongClick = { model.toggleSelect(m) },
                    onClick = { if (selecting) model.toggleSelect(m) },
                )
                .then(
                    if (photoBubble) Modifier else Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                ),
        ) {
            Crossfade(pending, animationSpec = tween(Tokens.Anim.normal), label = "attach") { isPending ->
                Column {
                    // Ответ рисуется ССЫЛКОЙ в локальную историю (ADR-006): текст цитаты
                    // не копируется в сообщение, поэтому удаление оригинала не оставляет
                    // расходящегося дубля.
                    //
                    // У фото шапка живёт на СВОЕЙ подложке шириной со снимок: подложка
                    // нужна тексту цитаты, а не картинке.
                    val head: @Composable () -> Unit = {
                        ForwardMark(model, m.fwdFrom, m.fwdAnon)
                        if (m.replyTo != 0L) {
                            // Ключ строки — пара «автор + id»: id посеян временем
                            // отправителя и у обеих сторон может совпасть.
                            val group = senderName != null || model.currentGroup != null
                            val src = model.chat(
                                if (group) model.currentGroup else model.currentChat,
                                group,
                            ).firstOrNull { it.id == m.replyTo && it.sender == m.replySender }
                            QuoteBlock(
                                model,
                                src,
                                m.replySender == model.me?.pubkey,
                                // Над фото плашка тянется во всю ширину шапки: узкая
                                // посреди широкой полосы читается обрывком.
                                modifier = if (photoHeader) Modifier.fillMaxWidth() else Modifier,
                            )
                        }
                        // Имя автора — под цитатой и пометкой пересылки: сначала «на
                        // что отвечают», потом «кто говорит».
                        if (senderName != null) {
                            Text(
                                senderName,
                                color = senderColor,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Medium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    if (photoHeader) {
                        Column(
                            Modifier.width(PhotoBubbleSize)
                                .background(if (m.mine) BubbleOut else BubbleIn)
                                .padding(horizontal = 10.dp, vertical = 6.dp),
                        ) { head() }
                    } else if (!photoBubble) {
                        head()
                    }
                    when {
                        isPending -> PendingAttachmentCard(
                            model,
                            m.kind,
                            m.name,
                            m.size,
                            m.durationMs,
                            m.thumbId,
                            m.blobId,
                        ) { model.currentChat?.let { model.downloadMedia(it, m.blobId) } }
                        else -> when (m.kind) {
                            // В режиме выделения тап по вложению переключает выбор,
                            // а не открывает файл: иначе выбор был бы ловушкой.
                            "image" -> ImageAttachment(
                                blobBytes,
                                onOpen = { if (selecting) model.toggleSelect(m) else viewer = true },
                                bare = photoBubble,
                                onLongClick = { model.toggleSelect(m) },
                                cacheKey = m.blobId,
                            )
                            // Видео — тот же кадр, что и фото, плюс метка
                            // длительности: тап открывает просмотрщик, а не отдаёт
                            // файл наружу системному плееру.
                            "video" -> VideoAttachment(
                                model.blob(m.thumbId),
                                ready,
                                m.durationMs,
                                onOpen = { if (selecting) model.toggleSelect(m) else viewer = true },
                                bare = photoBubble,
                                onLongClick = { model.toggleSelect(m) },
                                cacheKey = m.thumbId,
                            )
                            "voice" -> VoiceAttachment(blobBytes)
                            "file" -> FileAttachment(model, m.blobId, m.name, m.size, ready) { model.toggleSelect(m) }
                            // Текст обтекает мету: она едет в конец последней строки
                            // и переносится сама, если та её не вмещает.
                            else -> BubbleText(m.text) { meta() }
                        }
                    }
                    // Подпись к вложению — в том же пузыре, под содержимым. Она пришла
                    // ОДНИМ сообщением с файлом, поэтому и показывается как его часть.
                    if (m.kind != "text" && m.text.isNotEmpty()) {
                        BubbleText(m.text, modifier = Modifier.padding(top = 6.dp)) { meta() }
                    }
                }
            }
            // Мета всегда в правом нижнем углу ПУЗЫРЯ: место под неё уже оставлено
            // в тексте, а прижимать её к концу строки нельзя — у пузыря шире текста
            // (метка «Переслано», имя автора) она повисла бы посередине.
            // На голом фото — на затемнении: снимок бывает светлым, и белое время
            // на нём иначе теряется.
            Box(Modifier.align(Alignment.BottomEnd)) {
                if (photoBubble) MetaOnPhoto { meta() } else meta()
            }
        }
    }
    if (viewer) {
        // Листаем по вложениям ЭТОГО чата, а не показываем одно: список берётся из
        // уже загруженной ленты (полная история медиа — задача #45).
        val chat = model.currentChat.orEmpty()
        val me = stringResource(R.string.chat_reply_you)
        val peer = model.contacts.firstOrNull { it.key == chat }?.displayName
            ?: stringResource(R.string.chat_default_title)
        val items = remember(chat, model.chat(chat).size) {
            model.chat(chat)
                .filter { (it.kind == "image" || it.kind == "video") && it.blobId.isNotEmpty() }
                .map { ViewerItem(it.id, it.sender, if (it.mine) me else peer, it.time, it.mime, it.name, it.blobId) }
        }
        val start = items.indexOfFirst { it.id == m.id && it.sender == m.sender }
        MediaViewer(
            model,
            items,
            start.coerceAtLeast(0),
            onClose = { viewer = false },
            onForward = {
                viewer = false
                // Пересылка идёт через выделение — тот же путь, что из панели
                // чата, чтобы «переслать» везде означало ровно одно.
                model.toggleSelect(m)
                model.beginForward(chat)
            },
            onDelete = {
                model.toggleSelect(m)
                model.deleteSelected(chat)
            },
            onShowInChat = { target ->
                viewer = false
                model.jumpTo = target.id
            },
        )
    }
}

/** Сторона снимка, который сам себе пузырь. Шапка над ним (цитата, «Переслано»)
 *  берёт ту же ширину — иначе она торчала бы за край картинки. */
internal val PhotoBubbleSize = 240.dp

/** Картинка-вложение в пузыре: миниатюра (тап → полный экран) или плейсхолдер, пока
 *  байты докачиваются. */
@Composable
internal fun ImageAttachment(
    bytes: ByteArray?,
    onOpen: () -> Unit,
    bare: Boolean = false,
    onLongClick: () -> Unit = {},
    cacheKey: String = "",
) {
    // Снимок декодируется под сторону пузыря, а не в оригинале: в ленте он всё
    // равно не крупнее 240dp, а полноразмерный битмап стоит десятки мегабайт.
    val side = if (bare) PhotoBubbleSize else 220.dp
    val bmp = rememberBitmap(bytes, cacheKey, side)
    // [bare] — картинка САМА является пузырём: своё скругление и фон ей не нужны,
    // их даёт пузырь. Иначе внутри пузыря получалась бы вторая скруглённая рамка.
    //
    // Долгое нажатие обрабатывается ЗДЕСЬ, а не только у пузыря: обычный
    // `clickable` на картинке съедал жест целиком, и выделить сообщение-фото было
    // нельзя — палец всегда попадал в картинку, а не в поля пузыря.
    Box(
        Modifier.size(if (bare) PhotoBubbleSize else 220.dp)
            .then(if (bare) Modifier else Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloSurface))
            .combinedClickable(
                enabled = bmp != null,
                onClick = onOpen,
                onLongClick = onLongClick,
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (bmp != null) {
            Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        } else {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
        }
    }
}

/**
 * Видео-вложение: первый кадр + метка длительности, тап → просмотрщик.
 *
 * Кадр и длительность приезжают ВМЕСТЕ с сообщением (миниатюра — отдельным мелким
 * блобом), поэтому видно, что предлагают, ещё до скачивания самого файла.
 */
@Composable
internal fun VideoAttachment(
    thumb: ByteArray?,
    ready: Boolean,
    durationMs: Int,
    onOpen: () -> Unit,
    bare: Boolean = false,
    onLongClick: () -> Unit = {},
    cacheKey: String = "",
) {
    // Кадр — миниатюра, приехавшая с сообщением. Сам файл ради картинки не
    // читаем: раньше видео целиком шло в память и расшифрованной копией на диск.
    val side = if (bare) PhotoBubbleSize else 220.dp
    val frame = rememberBitmap(thumb, cacheKey, side)
    Box(
        Modifier.size(if (bare) PhotoBubbleSize else 220.dp)
            .then(if (bare) Modifier else Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)))
            .background(Color.Black)
            .combinedClickable(
                enabled = ready,
                onClick = onOpen,
                onLongClick = onLongClick,
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (frame != null) {
            Image(frame, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        }
        if (!ready) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
        } else {
            // Кнопка «play» — на затемнении: кадр бывает светлым, и белый значок
            // на нём иначе теряется.
            Box(
                Modifier.size(52.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.45f)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.PlayArrow, null, tint = Color.White, modifier = Modifier.size(32.dp))
            }
        }
        if (durationMs > 0) {
            Text(
                fmtDuration(durationMs),
                color = Color.White,
                fontSize = 11.sp,
                modifier = Modifier.align(Alignment.BottomStart)
                    .padding(6.dp)
                    .clip(RoundedCornerShape(Tokens.Radius.badge))
                    .background(Color.Black.copy(alpha = 0.55f))
                    .padding(horizontal = 5.dp, vertical = 1.dp),
            )
        }
    }
}

/** Файл-вложение: иконка + имя + размер; тап (когда скачан) → открыть системой.
 *  Размер — из сообщения: читать файл ради подписи под иконкой незачем. */
@Composable
internal fun FileAttachment(
    model: AppModel,
    blobId: String,
    name: String,
    size: Long,
    ready: Boolean,
    onLongClick: () -> Unit = {},
) {
    val ctx = LocalContext.current
    Row(
        // Долгое нажатие пробрасывается наружу — иначе своим `clickable` карточка
        // забирала бы жест и выделить сообщение-файл было бы нельзя.
        Modifier.padding(end = 44.dp).widthIn(min = 160.dp).combinedClickable(
            enabled = ready,
            onClick = { model.exportBlob(blobId, "shared", name) { f -> f?.let { openWithSystem(ctx, it) } } },
            onLongClick = onLongClick,
        ),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!ready) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
        } else {
            Icon(Icons.Filled.AttachFile, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.width(8.dp))
        Column {
            Text(name.ifBlank { stringResource(R.string.chat_file) }, color = Color.White, fontSize = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (ready) fmtSizeLong(size) else stringResource(R.string.chat_downloading),
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
            Modifier.size(38.dp).clip(CircleShape).background(HoloAccent)
                .clickable(enabled = bytes != null) {
                    val mp = player.value
                    if (mp != null && mp.isPlaying) {
                        mp.pause()
                        playing = false
                    } else if (mp != null) {
                        mp.start()
                        playing = true
                    } else if (bytes != null) {
                        // В `viewer/`: расшифрованные копии оттуда убираются на старте.
                        val dir = java.io.File(ctx.cacheDir, "viewer").apply { mkdirs() }
                        val f = java.io.File(dir, "play_${bytes.size}.m4a").apply { writeBytes(bytes) }
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
private fun openWithSystem(ctx: android.content.Context, f: java.io.File) {
    runCatching {
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
internal fun PendingAttachmentCard(
    model: AppModel,
    kind: String,
    name: String,
    size: Long,
    durationMs: Int,
    thumbId: String,
    blobId: String,
    onDownload: () -> Unit,
) {
    val thumb = model.blob(thumbId)
    // Состояние докачки живёт в движке, а не в этом composable: заказ персистентный,
    // продолжается сам при появлении пира и переживает перезапуск. Отсюда — только показ.
    val progress = model.downloads[blobId]
    Row(
        Modifier.padding(end = 44.dp).widthIn(min = 180.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(48.dp).clip(RoundedCornerShape(Tokens.Radius.chip)).background(Color.Black.copy(alpha = 0.25f)),
            contentAlignment = Alignment.Center,
        ) {
            val bmp = rememberBitmap(thumb, thumbId, 48.dp)
            if (bmp != null) {
                Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
            } else {
                Icon(kindIcon(kind), null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            }
        }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f, fill = false)) {
            Text(
                name.ifBlank { stringResource(R.string.chat_file) },
                color = Color.White,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                buildList {
                    if (durationMs > 0) add(fmtDuration(durationMs))
                    if (size > 0) add(fmtSizeLong(size))
                }.joinToString(" · "),
                color = HoloSecondary,
                fontSize = 11.sp,
            )
        }
        Spacer(Modifier.width(8.dp))
        // Кнопка → крутилка → кружок прогресса: три состояния одного действия,
        // поэтому подменяются затуханием, а не морганием.
        Crossfade(progress, animationSpec = tween(Tokens.Anim.fast), label = "dl") { progress ->
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
                else -> IconButton(onClick = onDownload) {
                    Icon(Icons.Filled.Download, stringResource(R.string.chat_download), tint = HoloAccent)
                }
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

/** Статус доставки исходящего сообщения — честно, без приукрашивания.
 *
 *  `queued` (получатель офлайн, сообщение ждёт в очереди) намеренно отличается от
 *  `pending` (уходит прямо сейчас) НЕ только иконкой, но и подписью: раньше оба
 *  рисовались одинаковыми часами, и «ждёт неделю» выглядело как «сейчас отправится».
 *  Галочка `sent` — доставка НЕ подтверждена; ✓✓ ставится только по Ack. */
/**
 * Мета сообщения: «изм.», время и — у своих — галочка доставки. Один компонент на
 * все ленты (личные чаты, группы, заметки): это подпись под одним и тем же фактом,
 * и расходиться она не должна.
 */
@Composable
internal fun MsgMeta(time: Long, mine: Boolean, edited: Boolean, status: String = "") {
    // 11sp вместо 10 и прозрачность 0.6 вместо 0.5: на 10sp в полупрозрачном белом
    // время буквально пропадало. 11 — нижняя граница читаемого в интерфейсе, ниже
    // опускаться уже нечему.
    val fg = Color.White.copy(alpha = 0.6f)
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (edited) {
            // Текст подменился — молчать об этом нельзя, иначе «исправил»
            // неотличимо от «так и было написано».
            Text(stringResource(R.string.chat_edited), color = fg, fontSize = 11.sp)
            Spacer(Modifier.width(4.dp))
        }
        // Моноширинные цифры: время у каждого сообщения своё, а колонка меты общая —
        // на пропорциональных цифрах она слегка гуляла бы от строки к строке.
        Text(
            fmtMsgTime(time),
            color = fg,
            fontSize = 11.sp,
            style = TextStyle(fontFeatureSettings = "tnum"),
        )
        if (mine) {
            Spacer(Modifier.width(3.dp))
            StatusTick(status)
        }
    }
}

/** Мета поверх фотографии: та же строка на затемнённой плашке. Без неё белое время
 *  пропадает на светлом снимке — а тень читается хуже плашки и на тёмном, и на светлом. */
@Composable
internal fun MetaOnPhoto(content: @Composable () -> Unit) {
    Box(
        Modifier.padding(6.dp)
            .clip(RoundedCornerShape(Tokens.Radius.badge))
            .background(Color.Black.copy(alpha = 0.45f))
            .padding(horizontal = 6.dp, vertical = 2.dp),
    ) { content() }
}

@Composable
internal fun StatusTick(status: String) {
    val tint = Color.White.copy(alpha = 0.7f)
    when (status) {
        "delivered" -> Icon(Icons.Filled.DoneAll, stringResource(R.string.status_delivered), tint = tint, modifier = Modifier.size(13.dp))
        "sent" -> Icon(Icons.Filled.Done, stringResource(R.string.status_sent), tint = tint, modifier = Modifier.size(13.dp))
        "pending" -> Icon(Icons.Filled.Schedule, stringResource(R.string.status_sending), tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(12.dp))
        // Только значок, без слова: мета живёт в углу пузыря и должна оставаться
        // узкой — слово растянуло бы её на половину строки. Расшифровка статуса —
        // в описании для screen reader и на экране очереди отправки.
        "queued" -> Icon(
            Icons.Filled.CloudQueue,
            stringResource(R.string.status_queued),
            tint = tint.copy(alpha = 0.7f),
            modifier = Modifier.size(12.dp),
        )
        "failed" -> Text("!", color = HoloError, fontSize = 11.sp, fontWeight = FontWeight.Bold)
    }
}
