@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package chat.peerage.ui

import android.Manifest
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
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
import androidx.compose.foundation.content.MediaType
import androidx.compose.foundation.content.consume
import androidx.compose.foundation.content.contentReceiver
import androidx.compose.foundation.content.hasMediaType
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CloudQueue
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.HorizontalAlignmentLine
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.layoutId
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@Composable
internal fun ChatTopBar(model: AppModel, onBack: () -> Unit, onProfile: () -> Unit, onMedia: () -> Unit) {
    // Заблокированный перестаёт быть контактом — в его чате делать нечего.
    var confirm by remember { mutableStateOf<ChatDanger?>(null) }
    // Выделение забирает верхнюю панель целиком: пока что-то выбрано, «назад» и
    // профиль не нужны, а нужен выход из режима и действия над выбранным.
    if (model.selection.isNotEmpty()) {
        SelectionTopBar(model)
        return
    }
    model.chatSearch?.takeIf { !it.chat.group && it.chat.id == model.currentChat }?.let {
        ChatSearchTopBar(model, it)
        return
    }
    val key = model.currentChat
    val c = key?.let { k -> model.contacts.firstOrNull { it.key == k } }
    val name = c?.displayName ?: stringResource(R.string.chat_default_title)
    val presence = key?.let { model.presence(it) } ?: Presence.Offline
    val avatar = c?.avatarId?.let { model.contactAvatars[it] }
    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(Tokens.Size.topBar).padding(horizontal = Tokens.Space.s),
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
                presence = presence,
                avatarBytes = avatar,
                circle = true, // собеседник — человек
                ring = HoloBg,
            )
            Spacer(Modifier.width(Tokens.Space.m))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        name,
                        color = Color.White,
                        fontSize = Tokens.Font.label,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (c?.verified == true) {
                        Spacer(Modifier.width(Tokens.Space.xs))
                        Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(Tokens.Size.iconXs))
                    }
                }
                if (key != null && isTyping(model, key)) {
                    TypingLabel(Tokens.Colors.accent, Tokens.Font.caption)
                } else if (c?.awaiting == true) {
                    Text(stringResource(R.string.chat_request_sent), color = HoloSecondary, fontSize = Tokens.Font.caption)
                } else {
                    Text(
                        presenceLabel(presence),
                        color = if (presence == Presence.Offline) HoloSecondary else presenceDot(presence),
                        fontSize = Tokens.Font.caption,
                    )
                }
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
        if (key != null) {
            Box {
                var menu by remember { mutableStateOf(false) }
                BarIcon(TablerIcons.DotsVertical, stringResource(R.string.common_more), tint = Color.White) { menu = true }
                ChatMenu(model, key, menu, onClose = { menu = false }, onProfile = onProfile, onMedia = onMedia) { confirm = it }
            }
        }
    }
    val k = key ?: return
    when (confirm) {
        ChatDanger.Clear -> ConfirmSheet(
            title = stringResource(R.string.chat_clear_title),
            text = stringResource(R.string.profile_clear_history_confirm),
            ok = stringResource(R.string.profile_clear_history_ok),
            onOk = { model.clearHistory(k) },
            onClose = { confirm = null },
        )
        ChatDanger.Block -> ConfirmSheet(
            title = stringResource(R.string.chat_block_title),
            text = stringResource(R.string.profile_block_confirm),
            ok = stringResource(R.string.profile_block),
            onOk = {
                model.blockRequest(k)
                onBack()
            },
            onClose = { confirm = null },
        )
        null -> {}
    }
}

/** Опасные пункты «⋮» личного чата — каждый через шторку-подтверждение. */
private enum class ChatDanger { Clear, Block }

/** «⋮» личного чата (app-ui.md → «Чат», макет `shared/Main`). */
@Composable
private fun ChatMenu(
    model: AppModel,
    key: String,
    open: Boolean,
    onClose: () -> Unit,
    onProfile: () -> Unit,
    onMedia: () -> Unit,
    onDanger: (ChatDanger) -> Unit,
) {
    var notify by remember(key) { mutableStateOf(false) }
    if (notify) NotifySheet(model, ChatTarget(key, group = false)) { notify = false }
    PopMenu(open, onClose) {
        PopMenuItem(TablerIcons.Search, stringResource(R.string.chat_menu_search)) {
            onClose()
            model.openChatSearch(ChatTarget(key, group = false))
        }
        PopMenuItem(TablerIcons.Photo, stringResource(R.string.profile_media)) {
            onClose()
            onMedia()
        }
        NotifyMenuItem(model, ChatTarget(key, group = false)) {
            onClose()
            notify = true
        }
        PopMenuItem(TablerIcons.User, stringResource(R.string.chat_menu_profile)) {
            onClose()
            onProfile()
        }
        PopMenuDivider()
        PopMenuItem(TablerIcons.Trash, stringResource(R.string.profile_clear_history), danger = true) {
            onClose()
            onDanger(ChatDanger.Clear)
        }
        PopMenuItem(TablerIcons.Ban, stringResource(R.string.profile_block), danger = true) {
            onClose()
            onDanger(ChatDanger.Block)
        }
    }
}

@Composable
internal fun ChatScreen(model: AppModel, onMedia: (MediaTab) -> Unit) {
    val key = model.currentChat ?: return
    val msgs = model.chat(key)
    var input by remember(key) { mutableStateOf("") }
    val listState = rememberLazyListState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    // Лента привязана к низу (reverseLayout): элемент 0 — самое свежее, чат с первого
    // кадра открыт на конце.
    val fresh = FollowNewest(listState, msgs, key)
    JumpToMessage(model, listState, msgs)
    PagingOlder(listState, key) { model.loadOlder(key, {}) }
    val chat = ChatTarget(key, group = false)
    PagingNewer(model, listState, chat)
    // «Поделиться» в этот чат: текст — в поле, вложения ждут над ним до «Отправить».
    val staged = model.stagedShare(chat)
    LaunchedEffect(model.shareDraft) { model.takeSharedText(chat)?.let { input = it } }
    // Лента на экране — от этого зависят непрочитанные и «прочитано» (см. AppModel.viewing).
    DisposableEffect(chat) {
        model.startViewing(chat)
        onDispose { model.stopViewing(chat) }
    }
    val search = model.chatSearch?.takeIf { it.chat == chat }
    // Правка начинается с ТЕКУЩЕГО текста: человек исправляет, а не набирает заново.
    LaunchedEffect(model.editTarget) { model.editTarget?.let { input = it.text } }
    LaunchedEffect(key) { model.refreshPins(key, group = false) }

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

    // Голосовые (M2): удержание, замок, отмена — VoiceSession.
    val voice = rememberVoiceSession { bytes, ms ->
        scope.launch(Dispatchers.IO) { model.sendMediaWithMeta(key, bytes, "audio/mp4", "voice.m4a", MediaMeta(durationMs = ms)) }
    }

    // Две панели: лента и поле — не шире feedMax, по центру листа (макет `home/SheetTablet`).
    Column(Modifier.fillMaxSize().feedWidth()) {
        if (search == null) PinnedBar(model, key, group = false, canUnpin = true)
        if (search != null && search.coversFeed) {
            ChatSearchPanel(model, search, Modifier.weight(1f), onMedia)
        } else {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                CompositionLocalProvider(LocalSearchMarks provides searchMarks(search)) {
                    DmFeed(model, msgs, listState, fresh)
                }
                JumpCapsule(model, listState, chat, Modifier.align(Alignment.BottomEnd))
                FeedSeen(model, listState, msgs, chat)
            }
        }
        ForwardSheet(model)
        if (search != null) {
            ChatSearchBar(model, search)
            ChatSearchSheets(model, search)
            return@Column
        }
        MessageInput(
            value = input,
            onChange = {
                // Набор — только когда текст растёт: стирание и вставка правки не «печатают».
                if (it.length > input.length && model.editTarget == null) model.typing(key)
                input = it
            },
            onPickFile = { filePicker.launch(arrayOf("*/*")) },
            onPickAudio = { audioPicker.launch(arrayOf("audio/*")) },
            onCamera = openCamera,
            onSendAttachments = { list, caption ->
                model.stageGallery(ctx, chat, list)
                model.sendStaged(ctx, chat, caption)
            },
            onStageAttachments = { model.stageGallery(ctx, chat, it) },
            voice = voice,
            model = model,
            hint = stringResource(if (model.isSaved(key)) R.string.saved_hint else R.string.chat_input_hint),
            onKeyboardMedia = { model.stageKeyboardMedia(ctx, chat, it) },
            staged = staged != null,
            replyStrip = {
                staged?.let { SharedStrip(it) { model.shareDraft = null } }
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
                } else if (staged != null) {
                    model.sendStaged(ctx, chat, input.trim())
                    input = ""
                } else if (input.isNotBlank()) {
                    model.send(key, input)
                    input = ""
                }
            },
        )
    }
}

/** Лента личного чата: привязана к низу (`reverseLayout`), элемент 0 — самое свежее. */
@Composable
private fun DmFeed(model: AppModel, msgs: List<Message>, listState: LazyListState, fresh: MutableSet<MsgRef>) {
    FeedFrame(rememberDragSelect(msgs, listState)) { DmFeedList(model, msgs, listState, fresh) }
}

@Composable
private fun DmFeedList(model: AppModel, msgs: List<Message>, listState: LazyListState, fresh: MutableSet<MsgRef>) {
    LazyColumn(
        state = listState,
        reverseLayout = true,
        // Поля по бокам даёт ряд сообщения: заливка выбора идёт во всю ширину листа.
        modifier = Modifier.fillMaxSize(),
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
            FeedItem(fresh, MsgRef(m.id, m.sender)) {
                if (prev == null || dayKey(prev.time) != dayKey(m.time)) DateChip(dayLabel(m.time))
                // Склейка блоков: в DM «отправитель» — это сторона (я/собеседник).
                val side = { msg: Message -> if (msg.mine) "me" else "peer" }
                val withPrev = prev != null && continuesBlock(side(prev), prev.time, side(m), m.time)
                val withNext = next != null && continuesBlock(side(m), m.time, side(next), next.time)
                val pos = blockPos(withPrev, withNext)
                MessageRow(model, m, pos, far = seriesCut(m, prev.takeIf { withPrev }, next.takeIf { withNext }))
            }
        }
        // Запрос в контакты ещё не принят (7i): сверху ленты — что сообщения ждут.
        model.contacts.firstOrNull { it.key == model.currentChat }?.takeIf { it.awaiting }?.let { c ->
            item(key = "awaiting") { AwaitingBanner(c.displayName) }
        }
        // Последний элемент = верх ленты: баннер над самой старой перепиской. В чате с
        // собой шифровать «между» некого — там вместо него пустое состояние.
        if (!model.isSaved(model.currentChat)) {
            item { E2EBanner() }
        } else if (msgs.isEmpty()) {
            item {
                Box(Modifier.fillParentMaxSize(), contentAlignment = Alignment.Center) {
                    EmptyState(
                        TablerIcons.Bookmark,
                        stringResource(R.string.saved_empty_title),
                        stringResource(R.string.saved_empty_hint),
                    )
                }
            }
        }
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
    scope.launch(Dispatchers.IO) { sendUriNow(ctx, model, key, uri, fallbackName, caption, group) }
}

/**
 * Поле принимает картинки с клавиатуры (GIF, стикеры): их uri уходят в [onMedia], остальное
 * (текст) — дальше, обычной вставкой.
 */
private fun Modifier.keyboardMedia(onMedia: (List<android.net.Uri>) -> Unit): Modifier =
    contentReceiver { content ->
        if (!content.hasMediaType(MediaType.Image)) return@contentReceiver content
        val uris = mutableListOf<android.net.Uri>()
        val rest = content.consume { item -> item.uri?.also { uris += it } != null }
        if (uris.isNotEmpty()) onMedia(uris)
        rest
    }

/** Отправить одно вложение по content:// — блокирует поток, звать с IO. */
internal fun sendUriNow(
    ctx: android.content.Context,
    model: AppModel,
    key: String,
    uri: android.net.Uri,
    fallbackName: String,
    caption: String,
    group: Boolean,
) {
    val mime = ctx.contentResolver.getType(uri) ?: "application/octet-stream"
    val name = displayName(ctx, uri) ?: fallbackName
    // Картинки идут через память: им нужен scrub (EXIF/GPS), а он требует всего
    // файла целиком. Весят они немного, так что это допустимо.
    if (mime.startsWith("image/")) {
        val bytes = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return
        val meta = MediaMeta.extract(ctx, uri, mime, bytes)
        model.sendMediaWithMeta(key, bytes, mime, name, meta, caption, group)
        return
    }
    // Всё остальное — ПОТОКОМ: сначала переливаем content:// в кэш-файл по 64 КБ,
    // потом движок считает манифест и отдаёт чанки с диска. Видео на гигабайт не
    // должно стоить гигабайта RAM только потому, что его выбрали в пикере.
    val cached = cacheFromUri(ctx, uri, name) ?: return
    val meta = MediaMeta.extract(ctx, uri, mime, ByteArray(0))
    model.sendMediaFile(key, cached.absolutePath, mime, name, meta, caption, group)
}

/** Перелить content:// в файл кэша порциями. Возвращает файл или null. */
internal fun cacheFromUri(ctx: android.content.Context, uri: android.net.Uri, name: String): java.io.File? =
    runCatching {
        val dir = java.io.File(ctx.cacheDir, "outgoing").apply { mkdirs() }
        // Имя прислал чужой провайдер: «../» в нём вывело бы запись из кэша в файлы
        // приложения. Берём только последний сегмент.
        val safe = name.substringAfterLast('/').substringAfterLast('\\').ifBlank { "file" }
        val f = java.io.File(dir, "${System.currentTimeMillis()}-$safe")
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
    onClick: (() -> Unit)? = null,
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
        modifier.padding(bottom = Tokens.Space.xs).height(IntrinsicSize.Min).heightIn(min = Tokens.Size.rule)
            .clip(RoundedCornerShape(Tokens.Radius.chip))
            .background(Tokens.Colors.quoteOnBubble)
            .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier),
    ) {
        AccentRule(Modifier.fillMaxHeight())
        // Миниатюра слева от подписи — если она у нас есть.
        val hasThumb = src != null && src.kind != "text" &&
            QuoteThumb(model, src.blobId, src.thumbId, src.kind)
        Column(Modifier.padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs)) {
            Text(
                who,
                color = HoloAccent,
                fontSize = Tokens.Font.small,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                src?.let {
                    when {
                        it.deleted -> stringResource(R.string.chat_reply_gone)
                        it.text.isNotEmpty() -> model.shownText(it)
                        // С миниатюрой имя файла лишнее — картинка уже сказала, что это.
                        hasThumb -> quoteKind(it.kind)
                        else -> it.name.ifEmpty { quoteKind(it.kind) }
                    }
                } ?: stringResource(R.string.chat_reply_gone),
                color = Tokens.Colors.quoteTextOnBubble,
                fontSize = Tokens.Font.small,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * «Сообщение удалено» на месте отозванного «у всех» (макет `special/Deleted`): тихая пилюля
 * без времени и статуса — содержимого нет, и говорить о нём нечего. Долгое нажатие сразу
 * ведёт к «удалить у себя»: остальные действия с пустой строкой бессмысленны.
 */
@Composable
private fun DeletedRow(model: AppModel, m: Message, group: Boolean) {
    val chatId = if (group) model.currentGroup else model.currentChat
    val shape = RoundedCornerShape(Tokens.Radius.bubble)
    Row(Modifier.fillMaxWidth(), horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
        // Входящие группы стоят правее колонки аватаров — и надгробие с ними.
        if (group && !m.mine) Spacer(Modifier.width(Tokens.Size.avatarHead + Tokens.Space.s))
        Row(
            Modifier.clip(shape)
                .background(Tokens.Colors.group)
                .border(1.dp, Tokens.Colors.raised, shape)
                .combinedClickable(onClick = {}, onLongClick = {
                    if (chatId != null) {
                        model.bubbleActions = BubbleActions(m, ChatTarget(chatId, group), Rect.Zero, shape)
                    }
                })
                .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.bubbleV),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(TablerIcons.Ban, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconXs))
            Spacer(Modifier.width(Tokens.Space.xs))
            Text(
                stringResource(R.string.msg_deleted),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.preview,
                fontStyle = FontStyle.Italic,
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
    // Кому отвечаем: своё — «Вы», чужое — имя автора по его ключу. В личном чате это
    // собеседник, в группе — конкретный участник.
    InputStrip(
        TablerIcons.Reply,
        stringResource(
            R.string.chat_reply_to,
            if (m.mine) stringResource(R.string.chat_reply_you) else groupSenderName(model, m.sender),
        ),
        if (m.text.isNotEmpty()) model.shownText(m) else m.name.ifEmpty { quoteKind(m.kind) },
        onCancel = model::cancelReply,
    )
}

/** Плашка присланного через «Поделиться»: «2 фото · 4,1 МБ», имена файлов, «×» — отказаться. */
@Composable
internal fun SharedStrip(d: chat.peerage.ShareDraft, onCancel: () -> Unit) =
    InputStrip(TablerIcons.Paperclip, sharedLabel(d), d.items.joinToString(", ") { it.name }, onCancel)

/** Системная строка «E2E» в начале переписки. */
/** Плашка над перепиской с тем, кто ещё не принял запрос (макет `people/Pending`). */
@Composable
private fun AwaitingBanner(name: String) {
    Box(Modifier.fillMaxWidth().padding(top = Tokens.Space.s, bottom = Tokens.Space.m), contentAlignment = Alignment.Center) {
        Text(
            stringResource(R.string.chat_awaiting_banner, name),
            color = Tokens.Colors.snippet,
            fontSize = Tokens.Font.body,
            lineHeight = Tokens.Font.lineHeight,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = Tokens.Size.systemLine)
                .clip(RoundedCornerShape(Tokens.Radius.button))
                .background(Tokens.Colors.labelScrim)
                .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
        )
    }
}

/**
 * Статус своего сообщения для показа. Собеседник ещё не принял запрос в контакты (7i) —
 * «отправлено» и «ожидание» честнее назвать «ждёт принятия»: сообщение лежит у нас и
 * уйдёт, когда запрос примут.
 */
internal fun shownStatus(model: AppModel, chat: String?, group: Boolean, status: String): String {
    if (group || status !in setOf("sent", "pending", "queued")) return status
    val awaiting = model.contacts.firstOrNull { it.key == chat }?.awaiting == true
    return if (awaiting) "held" else status
}

/** То же для сообщения открытого чата; у чужих статуса нет. */
internal fun shownStatus(model: AppModel, m: Message, group: Boolean): String =
    if (m.mine) shownStatus(model, model.currentChat, group, m.status) else m.status

@Composable
internal fun E2EBanner() {
    Row(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.s), horizontalArrangement = Arrangement.Center) {
        Row(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloSurface)
                .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Lock, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconXs))
            Spacer(Modifier.width(Tokens.Space.xs))
            Text(stringResource(R.string.chat_e2e_banner), color = HoloSecondary, fontSize = Tokens.Font.caption)
        }
    }
}

/**
 * Шапка режима выбора (app-ui.md → «Действия», макет `chatact/Multi`): снять выбор · «Выбрано: N»
 * · Переслать · Копировать · Удалить. Выбрано одно — ещё «Ответить» и «⋮» с меню сообщения:
 * выбор теперь начинается долгим тапом, и у фото без подписи (тап по нему открывает просмотр)
 * другого пути к меню нет.
 */
@Composable
internal fun SelectionTopBar(model: AppModel, group: Boolean = false) {
    val key = (if (group) model.currentGroup else model.currentChat) ?: return
    val ctx = LocalContext.current
    var confirmDelete by remember { mutableStateOf(false) }
    // «Назад» в режиме выбора снимает выбор, а не уводит из чата.
    BackHandler { model.clearSelection() }

    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(Tokens.Size.topBar).padding(horizontal = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(TablerIcons.X, stringResource(R.string.select_clear)) { model.clearSelection() }
        Text(
            stringResource(R.string.select_count, model.selection.size),
            color = Color.White,
            fontSize = Tokens.Font.name,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f).padding(start = Tokens.Space.s).politeLive(),
        )
        val one = model.selection.singleOrNull()?.let { r -> model.chat(key, group).firstOrNull { it.id == r.id && it.sender == r.sender } }
        if (one != null) BarIcon(TablerIcons.Reply, stringResource(R.string.chat_reply)) { model.startReply(one) }
        BarIcon(TablerIcons.Forward, stringResource(R.string.forward_action)) { model.beginForward(key, group) }
        BarIcon(TablerIcons.Copy, stringResource(R.string.chat_copy)) {
            val text = model.selectedTextOf(key, group)
            if (text.isNotEmpty()) copyToClipboard(ctx, text, "message")
            model.clearSelection()
        }
        BarIcon(TablerIcons.Trash, stringResource(R.string.chat_delete)) { confirmDelete = true }
        if (one != null) {
            Box {
                var more by remember { mutableStateOf(false) }
                BarIcon(TablerIcons.DotsVertical, stringResource(R.string.common_more)) { more = true }
                PopMenu(more, { more = false }) {
                    BubbleMenuItems(
                        model,
                        BubbleActions(one, ChatTarget(key, group), Rect.Zero, RectangleShape),
                        close = {
                            more = false
                            model.clearSelection()
                        },
                        onDelete = {
                            more = false
                            confirmDelete = true
                        },
                        fromSelection = true,
                    )
                }
            }
        }
    }
    if (confirmDelete) DeleteSheet(model, key, group, model.selection.toList()) { confirmDelete = false }
}

/** Чип-разделитель даты. */
@Composable
internal fun DateChip(label: String) {
    // Тот же ярлык, что у системных событий («Артём добавил Веру»): и то и другое
    // стоит вне диалога и комментирует ленту — значит и выглядеть должно одинаково.
    Row(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.s), horizontalArrangement = Arrangement.Center) {
        Box(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.labelScrim)
                .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.xxs),
        ) { Text(label, color = HoloSecondary, fontSize = Tokens.Font.caption) }
    }
}

/** Список «кого упомянуть» над полем ввода. Не больше пяти строк: дальше человек
 *  уточняет имя, а не листает. */
@Composable
private fun MentionSuggestions(
    candidates: List<Pair<String, String>>,
    avatarOf: (String) -> ByteArray?,
    onPick: (String, String) -> Unit,
) {
    Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s).padding(top = Tokens.Space.s)) {
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.button)).background(HoloSurface),
        ) {
            candidates.take(5).forEach { (key, name) ->
                Row(
                    Modifier.fillMaxWidth().clickable { onPick(key, name) }
                        .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // Фото человека, как в ленте и у участников: по одной букве в подсказке
                    // своих не узнать (с телефона).
                    SquareAvatar(name, size = Tokens.Size.avatarMention, avatarBytes = avatarOf(key), circle = true)
                    Spacer(Modifier.width(Tokens.Space.m))
                    Text(
                        name,
                        color = Color.White,
                        fontSize = Tokens.Font.label,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

/** Текст пузыря: упоминания рисуются именами, которые знает это устройство. */
private fun bubbleText(model: AppModel, m: Message): AnnotatedString =
    // Хвост из пробелов и переводов строк (пришедший или старый) не рисуем: время иначе
    // встаёт на пустую последнюю строку.
    if (m.mentions.isEmpty()) {
        AnnotatedString(m.text.trimEnd())
    } else {
        Mentions.render(
            m.text.trimEnd(),
            m.mentions,
            model.me?.pubkey,
            model::mentionName,
            accent = if (m.mine) Tokens.Colors.mentionOnOut else HoloAccent,
            meBg = Tokens.Colors.mentionSelfBg,
        )
    }

@Composable
internal fun MessageInput(
    value: String,
    onChange: (String) -> Unit,
    onPickFile: () -> Unit = {},
    onPickAudio: () -> Unit = {},
    onCamera: () -> Unit = {},
    onSendAttachments: (List<GalleryItem>, String) -> Unit = { _, _ -> },
    /** Шторку закрыли с выбранным: оно ждёт над полем; null — выбор пропадает. */
    onStageAttachments: ((List<GalleryItem>) -> Unit)? = null,
    /** Запись голосовых; null — микрофона в этом поле нет. */
    voice: VoiceSession? = null,
    model: AppModel? = null,
    /** Подсказка пустого поля: «Сообщение», в «Сохранённом» — «Заметка себе…». */
    hint: String? = null,
    /** Над полем ждут присланные через «Поделиться» вложения: отправка без текста — можно. */
    staged: Boolean = false,
    /** Полоска «отвечаем на …» над полем: экран решает, чья цель ответа. */
    replyStrip: (@Composable () -> Unit)? = null,
    /** Кого можно упомянуть по набранному после «@»: пары (ключ, имя). Пусто — в
     *  этом чате упоминаний нет (личный чат). */
    mentionCandidates: (String) -> List<Pair<String, String>> = { emptyList() },
    /** Человека выбрали из подсказки: его ключ и вставленная метка (без «@»). */
    onMentionPicked: (String, String) -> Unit = { _, _ -> },
    /** Картинка с клавиатуры (GIF, стикер); null — поле их не принимает. Зовётся синхронно,
     *  пока клавиатура держит доступ к uri. */
    onKeyboardMedia: ((List<android.net.Uri>) -> Unit)? = null,
    onSend: () -> Unit,
) {
    // Поле — TextFieldState: панели эмодзи и упоминаниям нужна ПОЗИЦИЯ КУРСОРА (иначе символ
    // падал бы в конец), а клавиатуре — приём картинок и GIF (`contentReceiver` в Compose 1.7
    // есть только у поля на TextFieldState).
    val field = rememberTextFieldState(value)
    // Последний текст, о котором знают и поле, и экран: по нему отличаем внешнюю правку
    // (очистка после отправки, черновик) от набора, не гоняя текст по кругу.
    var synced by remember { mutableStateOf(value) }
    LaunchedEffect(value) {
        if (value != synced) {
            synced = value
            if (field.text.toString() != value) field.setTextAndPlaceCursorAtEnd(value)
        }
    }
    val currentOnChange by rememberUpdatedState(onChange)
    LaunchedEffect(field) {
        snapshotFlow { field.text.toString() }.collect { t ->
            if (t != synced) {
                synced = t
                currentOnChange(t)
            }
        }
    }
    /** Заменить [start, end) на [ins] и поставить курсор после вставки. */
    fun replaceAt(start: Int, end: Int, ins: String) = field.edit {
        replace(start, end, ins)
        selection = TextRange(start + ins.length)
    }
    var emojiOpen by remember { mutableStateOf(false) }
    var attachOpen by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf<List<GalleryItem>>(emptyList()) }
    // Закрыть «Прикрепить»: выбранное не пропадает — ложится плашкой над полем.
    val closeAttach = {
        if (picked.isNotEmpty()) onStageAttachments?.invoke(picked)
        attachOpen = false
        picked = emptyList()
    }
    // Открытую панель «назад» закрывает, а не уводит из чата.
    BackHandler(enabled = emojiOpen || attachOpen) {
        emojiOpen = false
        if (attachOpen) closeAttach()
    }
    // Клавиатура и панель «Прикрепить» вместе не помещаются: открыли панель — клавиатура
    // уходит; вернулась клавиатура (тап по полю) — панель уступает, выбранное ждёт над полем.
    val keyboard = LocalSoftwareKeyboardController.current
    val imeShown = rememberImeVisible()
    LaunchedEffect(attachOpen) { if (attachOpen) keyboard?.hide() }
    LaunchedEffect(imeShown) { if (imeShown && attachOpen) closeAttach() }

    // Правка: вложить нечего (меняются только слова), отправка становится «Сохранить».
    val editing = model?.editTarget != null
    // Плашка прилегает к полю (зазор 2 dp), без неё — обычный отступ сверху.
    val stripShown = replyStrip != null && (editing || staged || model?.replyTarget != null)
    Column(Modifier.fillMaxWidth()) {
        // Цитата видна ровно там, где человек печатает ответ: иначе легко забыть,
        // что сообщение уйдёт ответом, и получить «ответ» не на то.
        replyStrip?.invoke()
        // Подсказка «кого упомянуть» — над полем, пока у курсора набирается «@…».
        // Выбор вставляет имя целиком и запоминает ключ: упоминание — ссылка на
        // человека, а не совпадение букв.
        val query = if (field.selection.collapsed) Mentions.query(field.text.toString(), field.selection.start) else null
        val candidates = query?.let { mentionCandidates(it.second) }.orEmpty()
        if (query != null && candidates.isNotEmpty()) {
            MentionSuggestions(candidates, { model?.peerAvatar(it) }) { key, name ->
                replaceAt(query.first, field.selection.start, "@$name ")
                onMentionPicked(key, name)
            }
        }
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
        // «Прикрепить» выезжает из строки ввода, как эмодзи: поле под панелью — подпись
        // набирается сразу, отправляет кнопка поля.
        AnimatedVisibility(
            visible = attachOpen,
            enter = panelEnter,
            exit = panelExit,
        ) {
            AttachPanel(
                selected = picked,
                onToggle = { g ->
                    picked = if (picked.any { it.uri == g.uri }) picked.filterNot { it.uri == g.uri } else picked + g
                },
                onCamera = {
                    closeAttach()
                    onCamera()
                },
                onPickFile = {
                    closeAttach()
                    onPickFile()
                },
                onPickAudio = {
                    closeAttach()
                    onPickAudio()
                },
                onClose = closeAttach,
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
                    replaceAt(field.selection.start, field.selection.end, e)
                    model?.useEmoji(e)
                },
                onBackspace = {
                    val end = field.selection.end
                    if (field.selection.length > 0) {
                        replaceAt(field.selection.start, end, "")
                    } else if (end > 0) {
                        // Удаляем ГРАФЕМУ, а не char: эмодзи — суррогатная пара, часто с
                        // модификаторами; посимвольный забой рвал бы их на половинки.
                        replaceAt(graphemeStart(field.text.toString(), end), end, "")
                    }
                },
                onClose = { emojiOpen = false },
            )
        }
        // Зафиксированная замком запись занимает всю строку: корзина, волна, отправка.
        if (voice != null && (voice.phase == VoicePhase.Locked || voice.phase == VoicePhase.Stopped)) {
            VoiceLockedRow(voice)
            return@Column
        }
        val holding = voice?.phase == VoicePhase.Holding
        Row(
            Modifier.fillMaxWidth().padding(
                start = Tokens.Space.m,
                end = Tokens.Space.m,
                top = if (stripShown) Tokens.Space.xxs else Tokens.Space.s,
                bottom = Tokens.Space.l, // выросшая кнопка записи (76 dp) должна влезть
            ),
            verticalAlignment = Alignment.Bottom,
        ) {
            // Поле ввода (макет `chatact/Main`): эмодзи и скрепка ВНУТРИ, ступень group. Эмодзи
            // слева, скрепка справа — как в Telegram, к которому привыкла рука.
            Row(
                Modifier.weight(1f).heightIn(min = Tokens.Size.composer)
                    .clip(RoundedCornerShape(Tokens.Radius.field)).background(Tokens.Colors.group),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (voice != null && holding) {
                    VoiceHoldField(voice)
                    return@Row
                }
                val emojiIcon = @Composable {
                    // Скрепка и эмодзи ведут себя одинаково: обе выдвигают панель из этой же
                    // строки, поэтому активная подсвечена акцентом и закрывается повторным тапом.
                    PillIcon(
                        TablerIcons.MoodSmile,
                        stringResource(R.string.cd_emoji),
                        tint = if (emojiOpen) HoloAccent else Tokens.Colors.secondary,
                    ) {
                        emojiOpen = !emojiOpen
                        if (emojiOpen && attachOpen) closeAttach()
                    }
                }
                emojiIcon()
                val attachIcon = @Composable {
                    PillIcon(
                        TablerIcons.Paperclip,
                        stringResource(R.string.cd_attach),
                        tint = if (attachOpen) HoloAccent else Tokens.Colors.secondary,
                    ) {
                        if (attachOpen) {
                            closeAttach()
                        } else {
                            attachOpen = true
                            emojiOpen = false
                        }
                    }
                }
                BasicTextField(
                    state = field,
                    modifier = Modifier.weight(1f).padding(vertical = Tokens.Space.m)
                        .then(if (onKeyboardMedia != null && !editing) Modifier.keyboardMedia(onKeyboardMedia) else Modifier),
                    textStyle = Tokens.Text.base.copy(color = Color.White),
                    cursorBrush = SolidColor(HoloAccent),
                    keyboardOptions = TextKeyboard,
                    lineLimits = TextFieldLineLimits.MultiLine(maxHeightInLines = 4),
                    decorator = { inner ->
                        Box(contentAlignment = Alignment.CenterStart) {
                            if (value.isEmpty()) {
                                Text(
                                    hint ?: stringResource(R.string.chat_input_hint),
                                    color = Tokens.Colors.tertiary,
                                    fontSize = Tokens.Font.label,
                                )
                            }
                            inner()
                        }
                    },
                )
                if (!editing) attachIcon()
            }
            Spacer(Modifier.width(Tokens.Space.s))
            // Отправка (есть текст) / голосовое удержанием (поле пустое).
            val send = value.isNotBlank() || (staged && !editing) || picked.isNotEmpty()
            ComposerButton(
                voice,
                record = !send,
                onClick = {
                    when {
                        // Выбранное в панели — с текстом поля подписью к первому вложению. Не
                        // отдельным сообщением: фото с подписью иначе — два пакета, и
                        // наблюдатель узнаёт их по характерной паре (M5).
                        picked.isNotEmpty() -> {
                            onSendAttachments(picked, value.trim())
                            picked = emptyList()
                            attachOpen = false
                            field.clearText()
                        }
                        send -> onSend()
                    }
                },
            ) {
                val tint = HoloAccentDark
                SwapIcon(
                    send,
                    {
                        Icon(
                            TablerIcons.Microphone,
                            stringResource(R.string.chat_voice),
                            tint = tint,
                            modifier = Modifier.size(Tokens.Size.iconS),
                        )
                    },
                    {
                        // Правка уходит «Сохранить» (✓), а не новой отправкой.
                        Icon(
                            if (editing) TablerIcons.Check else TablerIcons.Send,
                            stringResource(if (editing) R.string.common_save else R.string.common_send),
                            tint = tint,
                            modifier = Modifier.size(Tokens.Size.iconS),
                        )
                    },
                )
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

/** Иконка внутри поля ввода: касание во всю высоту поля и 48 dp в ширину. */
@Composable
internal fun PillIcon(icon: ImageVector, desc: String, tint: Color = HoloSecondary, onClick: () -> Unit) {
    Box(
        Modifier.width(Tokens.Size.touch).height(Tokens.Size.composer).clickable(role = Role.Button) { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(Tokens.Size.iconS)) }
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
    far: FarCut = FarCut.None,
) {
    // Системную строку и «Сообщение удалено» не выбирают: переслать или скопировать
    // в них нечего.
    val selecting = model.selection.isNotEmpty() && m.selectable
    // Выбор — у всего ряда (app-ui.md → «Действия»): заливка во всю ширину и кружок у
    // ведущего края. Не обводка пузыря: обводкой отмечено упоминание меня.
    val ref = MsgRef(m.id, m.sender)
    val selected = selecting && ref in model.selection
    // Правимое сообщение подсвечено так же (макет `chatact/Edit`): видно, что именно меняется.
    val editingThis = model.editTarget?.let { it.id == m.id && it.sender == m.sender } == true
    // Пришли сюда капсулой или по цитате (5.7): ряд подсвечен так же и гаснет плавно.
    val flashed = model.flash?.ref?.let { it.id == m.id && it.sender == m.sender } == true
    val rowFill by animateColorAsState(
        if (selected || editingThis || flashed) Tokens.Colors.selectedRow else Color.Transparent,
        tween(Tokens.Anim.normal),
        label = "rowFill",
    )
    // Кружок выбора лежит ПОВЕРХ ряда, а входящий пузырь отъезжает слоем, без перекладки:
    // ширина пузыря не меняется, текст не переносится заново, и лента при входе в выбор не
    // прыгает. Справа у входящего и слева у своего места на кружок хватает (пузырь ≤ 60 %).
    val checkShown by animateFloatAsState(if (selecting) 1f else 0f, tween(Tokens.Anim.fast), label = "check")
    val shift = with(LocalDensity.current) { (Tokens.Size.check + Tokens.Space.m).toPx() }
    val drag = LocalDragSelect.current
    if (drag != null && m.selectable) {
        DisposableEffect(drag, ref) { onDispose { drag.rows.remove(ref) } }
    }
    val press = remember { MutableInteractionSource() }
    val gestures = bubbleGestures(model, m, pos, group, far)
    // Пузырь — не весь ряд: тап и долгий тап по пустому месту сбоку от него — те же, что по
    // пузырю (п. 18). По голому снимку тап открывает просмотрщик — меню иначе не позвать.
    val bubbleRow = !m.system && !m.isCall && !m.deleted
    Box(
        Modifier.fillMaxWidth()
            .background(rowFill)
            .then(
                if (drag != null && m.selectable) Modifier.onGloballyPositioned { drag.rows[ref] = it.boundsInRoot() } else Modifier,
            )
            .then(
                when {
                    selecting -> Modifier.toggleable(selected, press, indication = null, role = Role.Checkbox) { model.toggleSelect(m) }
                    bubbleRow -> Modifier.combinedClickable(press, indication = null, onLongClick = gestures.onLong, onClick = gestures.onTap)
                    else -> Modifier
                },
            )
            .padding(horizontal = Tokens.Space.m),
        contentAlignment = Alignment.CenterStart,
    ) {
        // Свайп-ответ — всегда в дереве (выключен на время выбора): пересоздай ряд посреди
        // долгого тапа — и жест протяжки оборвался бы вместе с пузырём.
        SwipeToReply(
            Modifier.fillMaxWidth().graphicsLayer { if (!m.mine) translationX = shift * checkShown },
            enabled = !selecting && m.selectable,
            onReply = { model.startReply(m) },
        ) {
            MessageRowContent(model, m, pos, group, far, gestures)
        }
        if (checkShown > 0f) {
            Box(Modifier.graphicsLayer { alpha = checkShown }) { SelectCheck(selected) }
        }
    }
}

/**
 * Ответ свайпом влево (app-ui.md → «Жесты», макет `chatact/Swipe`): пузырь едет за пальцем,
 * справа проявляется стрелка; за порогом — отклик, отпустил — ответ. Жест не начинается у
 * правого края: там системный «назад».
 */
@Composable
private fun SwipeToReply(modifier: Modifier, enabled: Boolean, onReply: () -> Unit, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    val haptic = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val offset = remember { Animatable(0f) }
    val threshold = with(density) { Tokens.Size.swipeReply.toPx() }
    val edge = maxOf(
        WindowInsets.systemGestures.getRight(density, LayoutDirection.Ltr),
        with(density) { Tokens.Space.xxl.roundToPx() },
    )
    val latestReply by rememberUpdatedState(onReply)
    Box(
        modifier.pointerInput(enabled) {
            if (!enabled) return@pointerInput
            var active = false
            var armed = false
            val settle = { scope.launch { offset.animateTo(0f, tween(Tokens.Anim.normal)) } }
            detectHorizontalDragGestures(
                onDragStart = { start ->
                    active = start.x < size.width - edge
                    armed = false
                },
                onDragEnd = {
                    if (armed) latestReply()
                    settle()
                },
                onDragCancel = { settle() },
            ) { change, dx ->
                if (!active) return@detectHorizontalDragGestures
                // Дальше порога тянется туже: видно, что дальше тянуть незачем.
                val raw = offset.value + if (-offset.value > threshold) dx / 3 else dx
                val v = raw.coerceIn(-threshold * 1.5f, 0f)
                change.consume()
                scope.launch { offset.snapTo(v) }
                val now = -v >= threshold
                if (now && !armed) haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                armed = now
            }
        },
    ) {
        Box(Modifier.graphicsLayer { translationX = offset.value }) { content() }
        Box(
            Modifier.align(Alignment.CenterEnd)
                .graphicsLayer {
                    val p = (-offset.value / threshold).coerceIn(0f, 1f)
                    alpha = p
                    scaleX = 0.6f + 0.4f * p
                    scaleY = 0.6f + 0.4f * p
                }
                .size(Tokens.Size.replyBadge)
                .clip(CircleShape)
                .background(Tokens.Colors.raised),
            contentAlignment = Alignment.Center,
        ) { Icon(TablerIcons.Reply, null, tint = Tokens.Colors.accentSoft, modifier = Modifier.size(Tokens.Size.iconS)) }
    }
}

@Composable
private fun MessageRowContent(model: AppModel, m: Message, pos: BlockPos, group: Boolean, far: FarCut, gestures: BubbleGestures) {
    when {
        m.system -> SystemRow(model, m)
        m.isCall -> model.currentChat?.let { CallRow(model, m, it) }
        m.deleted -> DeletedRow(model, m, group)
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
                // Низ аватара — по низу пузыря, а не ряда: чипы реакций и «Не доставлено»
                // под пузырём его не тянут вниз.
                Box(Modifier.alignBy { it.measuredHeight }) {
                    SquareAvatar(
                        who,
                        size = Tokens.Size.avatarHead,
                        presence = model.presence(m.sender),
                        avatarBytes = model.peerAvatar(m.sender),
                        circle = true,
                        ring = Tokens.Colors.panel, // лента — на листе
                    )
                }
            } else {
                Spacer(Modifier.width(Tokens.Size.avatarHead))
            }
            Spacer(Modifier.width(Tokens.Space.s))
            MessageBubble(
                m,
                model,
                gestures,
                pos,
                senderName = if (pos == BlockPos.Solo || pos == BlockPos.First) who else null,
                senderColor = nameColor(m.sender),
                group = true,
                far = far,
                modifier = Modifier.alignBy(BubbleBottom),
            )
        }
        // Своё сообщение в группе — тоже групповое: его вложение открывается из ленты
        // группы, а не личного чата.
        else -> MessageBubble(m, model, gestures, pos, group = group, far = far)
    }
}

@Composable
private fun MessageBubble(
    m: Message,
    model: AppModel,
    gestures: BubbleGestures,
    pos: BlockPos = BlockPos.Solo,
    /** Имя автора внутри пузыря — только в группе и только на первом сообщении
     *  блока: в личном чате собеседник один и подписывать его нечем. */
    senderName: String? = null,
    senderColor: Color = Color.White,
    /** Пузырь в группе. Явно, а не по `currentChat`: в группе тот пуст, и всё, что
     *  ищет «свой чат», — просмотрщик, докачка, цитата — молча искало в никуда. */
    group: Boolean = false,
    /** Дальние углы у стыков серии (5.8): решает лента по ширинам соседей. */
    far: FarCut = FarCut.None,
    modifier: Modifier = Modifier,
) {
    // Чат пузыря: gid группы или ключ собеседника.
    val chatId = if (group) model.currentGroup else model.currentChat
    val shape = bubbleShape(m.mine, pos, far)
    val widths = LocalBubbleWidths.current
    // Байты в память берём только у фото и голосового: видео рисуется миниатюрой,
    // файл — именем и размером из сообщения. Иначе прокрутка чата с видео тянула
    // бы в кучу файлы целиком ради одного кадра.
    val needsBytes = m.kind == "image" || m.kind == "voice"
    val blobBytes = if (needsBytes) model.blob(m.blobId) else null
    val ready = m.downloaded || model.savedBlobs[m.blobId] == true
    var viewer by remember { mutableStateOf(false) }
    val selecting = model.selection.isNotEmpty()
    // Не доставлено (выброшено из очереди или не легло в неё): красная обводка и строка
    // «Повторить» под пузырём. Только в личном чате — у групп статуса доставки нет.
    val failed = m.mine && m.status == "failed" && !group && chatId != null
    val place = gestures.place
    val onLong = gestures.onLong
    val onTap = gestures.onTap
    // Мета одна на все ветки пузыря: и когда она встроена в текст, и когда лежит
    // поверх вложения — это один и тот же набор значков.
    val meta: @Composable () -> Unit = {
        // В «Сохранённом» доставлять некому — галочек нет.
        val ticks = m.mine && m.status.isNotEmpty() && (group || !model.isSaved(chatId))
        MsgMeta(m.time, ticks, m.editedAt > 0, shownStatus(model, m, group))
    }
    Row(
        modifier.fillMaxWidth().padding(bottom = blockGap(pos)),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start,
    ) {
        Column(horizontalAlignment = if (m.mine) Alignment.End else Alignment.Start) {
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
            // Рамка снимка — по его пропорциям; шапка над голым снимком берёт её ширину.
            val frame = mediaFrame(m.width, m.height, if (photoBubble) PhotoBubbleSize else Tokens.Size.photoInBubble)
            // Шапка над фото: пометка пересылки и цитата ответа.
            val photoHeader = photoBubble &&
                (m.fwdFrom.isNotEmpty() || m.fwdAnon || m.replyTo != 0L || senderName != null)
            Box(
                Modifier.markBubbleBottom()
                    .widthIn(max = LocalBubbleMax.current)
                    .onGloballyPositioned {
                        place.rect = it.boundsInRoot()
                        // Ширина — для углов соседей (5.8); пишем только перемену.
                        val ref = MsgRef(m.id, m.sender)
                        if (widths != null && widths[ref] != it.size.width) widths[ref] = it.size.width
                    }
                    .clipDrawn(shape)
                    .then(
                        if (photoBubble) Modifier else Modifier.background(if (m.mine) BubbleOut else BubbleIn),
                    )
                    .then(
                        when {
                            failed -> Modifier.border(1.dp, Tokens.Colors.failEdge, shape)
                            // Фото без подписи — само себе пузырь: обводка картинки по его форме.
                            photoBubble -> Modifier.imageOutline(shape)
                            else -> Modifier
                        },
                    )
                    // Тап — меню у пузыря, долгий — выбор; в режиме выбора тап переключает,
                    // а не открывает вложение (иначе выбор был бы ловушкой).
                    .combinedClickable(onLongClick = onLong, onClick = onTap)
                    .then(
                        if (photoBubble) Modifier else Modifier.padding(horizontal = Tokens.Space.bubbleH, vertical = Tokens.Space.bubbleV),
                    ),
            ) {
                Crossfade(pending, animationSpec = tween(Tokens.Anim.normal), label = "attach") { isPending ->
                    // Не Column: цитата тянется во всю ширину пузыря, а не по своему тексту.
                    BubbleColumn {
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
                                // Поиск по всей истории чата — не на каждую перерисовку ряда (п. 19):
                                // пересчитывается, только когда меняется сама история.
                                val src = remember(chatId, m.replyTo, m.replySender) {
                                    derivedStateOf { model.chat(chatId, group).firstOrNull { it.id == m.replyTo && it.sender == m.replySender } }
                                }.value
                                QuoteBlock(
                                    model,
                                    src,
                                    m.replySender == model.me?.pubkey,
                                    // Тап — к исходному, место ответа — в стек «вниз» (5.7).
                                    onClick = if (src != null && chatId != null && !selecting) {
                                        { model.followQuote(ChatTarget(chatId, group), m, src) }
                                    } else {
                                        null
                                    },
                                    // Плашка — во всю ширину пузыря (над фото — шапки): узкая
                                    // посреди широкой полосы читается обрывком.
                                    modifier = if (photoHeader) Modifier.fillMaxWidth() else Modifier.layoutId(BubbleStretch),
                                )
                            }
                            // Имя автора — под цитатой и пометкой пересылки: сначала «на
                            // что отвечают», потом «кто говорит».
                            if (senderName != null) {
                                Text(
                                    senderName,
                                    color = senderColor,
                                    fontSize = Tokens.Font.small,
                                    fontWeight = FontWeight.Medium,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                        if (photoHeader) {
                            Column(
                                Modifier.width(frame.width)
                                    .background(if (m.mine) BubbleOut else BubbleIn)
                                    .padding(horizontal = Tokens.Space.bubbleH, vertical = Tokens.Space.bubbleV),
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
                            ) {
                                // Просим у автора: в личном чате это собеседник, в группе —
                                // тот, кто прислал (вложение лежит у него).
                                val from = if (group) m.sender else model.currentChat
                                val chat = if (group) model.currentGroup?.let { ChatTarget(it, true) } else model.currentChat?.let { ChatTarget(it, false) }
                                from?.let { model.downloadMedia(it, m.blobId, m.name, m.size, chat) }
                            }
                            else -> when (m.kind) {
                                // В режиме выделения тап по вложению переключает выбор,
                                // а не открывает файл: иначе выбор был бы ловушкой.
                                // Снимок помечен местом на экране: просмотрщик, закрываясь
                                // протяжкой, уводит его обратно сюда.
                                "image" -> Box(Modifier.mediaSpot(MsgRef(m.id, m.sender))) {
                                    ImageAttachment(
                                        blobBytes,
                                        frame,
                                        onOpen = { if (selecting) model.toggleSelect(m) else viewer = true },
                                        bare = photoBubble,
                                        onLongClick = onLong,
                                        cacheKey = m.blobId,
                                    )
                                }
                                // Видео — тот же кадр, что и фото, плюс метка
                                // длительности: тап открывает просмотрщик, а не отдаёт
                                // файл наружу системному плееру.
                                "video" -> VideoAttachment(
                                    model.blob(m.thumbId),
                                    frame,
                                    ready,
                                    m.durationMs,
                                    onOpen = { if (selecting) model.toggleSelect(m) else viewer = true },
                                    bare = photoBubble,
                                    onLongClick = onLong,
                                    cacheKey = m.thumbId,
                                )
                                "voice" -> VoiceAttachment(blobBytes, m.blobId, m.durationMs)
                                "file" -> FileAttachment(model, m.blobId, m.name, m.size, ready, onLongClick = onLong)
                                // Текст обтекает мету: она едет в конец последней строки
                                // и переносится сама, если та её не вмещает.
                                else -> {
                                    // Разбор разметки и ссылок — раз на текст, а не на каждую перерисовку
                                    // ряда (п. 19). Имена в «@» зависят от контактов — только у таких.
                                    val names = if (m.mentions.isEmpty()) null else model.contacts
                                    val shown = remember(m.text, m.mentions, m.mine, names) { bubbleText(model, m).markdown().linked(m.mine) }
                                    BubbleText(shown.marked(LocalSearchMarks.current, m)) { meta() }
                                }
                            }
                        }
                        // Подпись к вложению — в том же пузыре, под содержимым. Она пришла
                        // ОДНИМ сообщением с файлом, поэтому и показывается как его часть.
                        if (m.kind != "text" && m.text.isNotEmpty()) {
                            val caption = remember(m.text, m.mine) { AnnotatedString(m.text.trimEnd()).markdown().linked(m.mine) }
                            BubbleText(
                                caption.marked(LocalSearchMarks.current, m),
                                modifier = Modifier.padding(top = Tokens.Space.s),
                            ) { meta() }
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
            if (m.reactions.isNotEmpty() && chatId != null) ReactionChips(model, m, ChatTarget(chatId, group))
            if (failed) FailedLine { chatId?.let { model.retry(it, m) } }
        }
    }
    if (viewer && chatId != null) {
        // Листается всё медиа чата (просмотрщик сам спросит движок); лента — затравка,
        // пока список едет.
        MediaViewer(
            model,
            ChatTarget(chatId, group),
            model.chat(chatId, group),
            MsgRef(m.id, m.sender),
            onClose = { viewer = false },
            onReply = { model.startReply(it) },
            onShowInChat = { model.jumpTo = MsgRef(it.id, it.sender) },
        )
    }
}

/** «Не доставлено · Повторить» под пузырём (макет `special/Statuses`). */
@Composable
private fun FailedLine(onRetry: () -> Unit) {
    Row(
        Modifier.padding(top = Tokens.Space.xxs, end = Tokens.Space.xs)
            .clip(RoundedCornerShape(Tokens.Radius.chip))
            .clickable(role = Role.Button, onClickLabel = stringResource(R.string.act_retry)) { onRetry() }
            .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(stringResource(R.string.act_failed) + " · ", color = Tokens.Colors.failText, fontSize = Tokens.Font.small)
        Text(stringResource(R.string.act_retry), color = Tokens.Colors.accentSoft, fontSize = Tokens.Font.small, fontWeight = FontWeight.Medium)
    }
}

/** Низ самого пузыря в ряду сообщения — без того, что висит под ним (реакции, «Не доставлено»). */
private val BubbleBottom = HorizontalAlignmentLine(::maxOf)

private fun Modifier.markBubbleBottom() = layout { m, c ->
    val p = m.measure(c)
    layout(p.width, p.height, mapOf(BubbleBottom to p.height)) { p.place(0, 0) }
}

/** Место пузыря в окне; обновляется раскладкой, читается по долгому нажатию. */
/**
 * Жесты сообщения. Тап — меню у пузыря; долгое нажатие — выбор, и не отпуская пальца можно
 * выбрать подряд (протяжка, как в Telegram). В режиме выбора тап переключает. Одни на пузырь,
 * его вложения (иначе палец на картинке съедал бы жест) и пустое место ряда сбоку (п. 18).
 */
private class BubbleGestures(val place: BubblePlace, val onTap: () -> Unit, val onLong: () -> Unit)

@Composable
private fun bubbleGestures(model: AppModel, m: Message, pos: BlockPos, group: Boolean, far: FarCut): BubbleGestures {
    // Где пузырь лежит в окне — для меню у него. Не состояние: прокрутка двигает пузырь
    // каждый кадр, а читается место только в момент нажатия.
    val place = remember { BubblePlace() }
    val drag = LocalDragSelect.current
    val chatId = if (group) model.currentGroup else model.currentChat
    val shape = bubbleShape(m.mine, pos, far)
    return BubbleGestures(
        place,
        onTap = {
            if (model.selection.isNotEmpty()) {
                model.toggleSelect(m)
            } else if (chatId != null && m.id != 0L) {
                model.bubbleActions = BubbleActions(m, ChatTarget(chatId, group), place.rect, shape)
            }
        },
        onLong = {
            if (m.selectable) {
                if (drag != null) drag.start(model, m) else model.toggleSelect(m)
            }
        },
    )
}

private class BubblePlace {
    var rect = androidx.compose.ui.geometry.Rect.Zero
}

/** Наибольшая ширина снимка, который сам себе пузырь. Шапка над ним (цитата, «Переслано»)
 *  берёт ширину снимка — иначе она торчала бы за край картинки. */
internal val PhotoBubbleSize = Tokens.Size.photoBubble

/**
 * Рамка снимка в ленте по его пропорциям: ширина — до [maxW], высота — по пропорции, но не
 * выше 4/3 и не ниже 0,4 ширины; высокий кадр сужается (не уже 0,55 [maxW]). Что не влезло в
 * пределы — обрезается по краям. Размер неизвестен (старые сообщения) — квадрат.
 */
internal fun mediaFrame(w: Int, h: Int, maxW: Dp): DpSize {
    if (w <= 0 || h <= 0) return DpSize(maxW, maxW)
    val ratio = h.toFloat() / w
    val maxH = maxW * (4f / 3f)
    var width = maxW
    var height = maxW * ratio
    if (height > maxH) {
        height = maxH
        width = (maxH / ratio).coerceAtLeast(maxW * 0.55f)
    }
    return DpSize(width, height.coerceAtLeast(maxW * 0.4f))
}

/** Картинка-вложение в пузыре: миниатюра (тап → полный экран) или плейсхолдер, пока
 *  байты докачиваются. */
@Composable
internal fun ImageAttachment(
    bytes: ByteArray?,
    frame: DpSize,
    onOpen: () -> Unit,
    bare: Boolean = false,
    onLongClick: () -> Unit = {},
    cacheKey: String = "",
) {
    // Снимок декодируется под сторону пузыря, а не в оригинале: в ленте он всё
    // равно не крупнее 240dp, а полноразмерный битмап стоит десятки мегабайт.
    val side = maxOf(frame.width, frame.height)
    val bmp = rememberBitmap(bytes, cacheKey, side)
    // GIF и анимированный WebP играют прямо в пузыре (Android 9+); до разбора и ниже 9 —
    // первый кадр битмапом.
    val anim = rememberAnimatedPainter(bytes, side)
    // [bare] — картинка САМА является пузырём: своё скругление и фон ей не нужны,
    // их даёт пузырь. Иначе внутри пузыря получалась бы вторая скруглённая рамка.
    //
    // Долгое нажатие обрабатывается ЗДЕСЬ, а не только у пузыря: обычный
    // `clickable` на картинке съедал жест целиком, и выделить сообщение-фото было
    // нельзя — палец всегда попадал в картинку, а не в поля пузыря.
    Box(
        Modifier.size(frame)
            .then(if (bare) Modifier else Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloSurface).imageOutline(RoundedCornerShape(Tokens.Radius.chip)))
            .combinedClickable(
                enabled = bmp != null,
                onClick = onOpen,
                onLongClick = onLongClick,
            ),
        contentAlignment = Alignment.Center,
    ) {
        when {
            anim != null -> Image(anim, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
            bmp != null -> Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
            else -> CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
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
    frame: DpSize,
    ready: Boolean,
    durationMs: Int,
    onOpen: () -> Unit,
    bare: Boolean = false,
    onLongClick: () -> Unit = {},
    cacheKey: String = "",
) {
    // Кадр — миниатюра, приехавшая с сообщением. Сам файл ради картинки не
    // читаем: раньше видео целиком шло в память и расшифрованной копией на диск.
    val still = rememberBitmap(thumb, cacheKey, maxOf(frame.width, frame.height))
    Box(
        Modifier.size(frame)
            .then(if (bare) Modifier else Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).imageOutline(RoundedCornerShape(Tokens.Radius.chip)))
            .background(Color.Black)
            .combinedClickable(
                enabled = ready,
                onClick = onOpen,
                onLongClick = onLongClick,
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (still != null) {
            Image(still, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        }
        if (!ready) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
        } else {
            // Кнопка «play» — на затемнении: кадр бывает светлым, и белый значок
            // на нём иначе теряется.
            Box(
                Modifier.size(52.dp).clip(CircleShape).background(Tokens.Colors.playScrim),
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.PlayArrow, null, tint = Color.White, modifier = Modifier.size(Tokens.Size.iconXl))
            }
        }
        if (durationMs > 0) {
            Text(
                fmtDuration(durationMs),
                color = Color.White,
                fontSize = Tokens.Font.caption,
                modifier = Modifier.align(Alignment.BottomStart)
                    .padding(Tokens.Space.s)
                    .clip(RoundedCornerShape(Tokens.Radius.badge))
                    .background(Tokens.Colors.mediaScrim)
                    .padding(horizontal = Tokens.Space.xs, vertical = 1.dp),
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
        Modifier.padding(end = Tokens.Size.metaReserve).widthIn(min = 160.dp).combinedClickable(
            enabled = ready,
            onClick = { model.exportBlob(blobId, "shared", name) { f -> f?.let { openWithSystem(ctx, it) } } },
            onLongClick = onLongClick,
        ),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!ready) {
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
        } else {
            Icon(Icons.Filled.AttachFile, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconM))
        }
        Spacer(Modifier.width(Tokens.Space.s))
        Column {
            Text(name.ifBlank { stringResource(R.string.chat_file) }, color = Color.White, fontSize = Tokens.Font.label, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (ready) fmtSizeLong(size) else stringResource(R.string.chat_downloading),
                color = HoloSecondary,
                fontSize = Tokens.Font.caption,
            )
        }
    }
}

/**
 * Голосовое-вложение: play/pause + длительность (M2). Плеер на байтах блоба.
 *
 * Длительность берётся из сообщения: до первого проигрывания иначе стояло слово
 * «Голосовое». Байты присылает собеседник, поэтому плеер собирается под
 * `runCatching`: битый файл — это молчащая кнопка, а не падение приложения.
 */
@Composable
internal fun VoiceAttachment(bytes: ByteArray?, blobId: String, knownDurationMs: Int) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var playing by remember { mutableStateOf(false) }
    var durationMs by remember(bytes) { mutableStateOf(knownDurationMs) }
    val player = remember { mutableStateOf<android.media.MediaPlayer?>(null) }
    DisposableEffect(bytes) {
        onDispose {
            player.value?.release()
            player.value = null
        }
    }
    Row(
        Modifier.padding(end = Tokens.Size.metaReserve).widthIn(min = 150.dp),
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
                        // Файл и подготовка плеера — не на главном потоке. Имя — по
                        // блобу: по размеру два разных голосовых одной длины писали в
                        // один файл, и второе подменяло играющее первое.
                        scope.launch(Dispatchers.IO) {
                            val np = runCatching {
                                // В `viewer/`: расшифрованные копии оттуда убираются на старте.
                                val dir = java.io.File(ctx.cacheDir, "viewer").apply { mkdirs() }
                                val f = java.io.File(dir, "voice_$blobId.m4a").apply { writeBytes(bytes) }
                                android.media.MediaPlayer().apply {
                                    setDataSource(f.absolutePath)
                                    prepare()
                                }
                            }.getOrNull() ?: return@launch
                            // Пузырь ушёл с экрана, пока плеер готовился, — отпускаем
                            // его здесь: onDispose про него ещё не знает.
                            try {
                                withContext(Dispatchers.Main) {
                                    if (player.value != null) {
                                        np.release() // второй тап, пока готовился первый
                                        return@withContext
                                    }
                                    durationMs = np.duration
                                    np.setOnCompletionListener {
                                        playing = false
                                        it.seekTo(0)
                                    }
                                    np.start()
                                    player.value = np
                                    playing = true
                                }
                            } catch (e: kotlinx.coroutines.CancellationException) {
                                np.release()
                                throw e
                            }
                        }
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
                    modifier = Modifier.size(Tokens.Size.iconM),
                )
            }
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Icon(Icons.Filled.Mic, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconS))
        Spacer(Modifier.width(Tokens.Space.s))
        Text(
            if (durationMs > 0) fmtDuration(durationMs) else stringResource(R.string.chat_voice),
            color = Color.White,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(end = Tokens.Space.s),
        )
    }
}

/** Открыть скачанный блоб системным приложением через FileProvider. */
internal fun openWithSystem(ctx: android.content.Context, f: java.io.File) {
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
        Modifier.padding(end = Tokens.Size.metaReserve).widthIn(min = 180.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(48.dp).clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.thumbScrim).imageOutline(RoundedCornerShape(Tokens.Radius.chip)),
            contentAlignment = Alignment.Center,
        ) {
            val bmp = rememberBitmap(thumb, thumbId, 48.dp)
            if (bmp != null) {
                Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
            } else {
                Icon(kindIcon(kind), null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconM))
            }
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f, fill = false)) {
            Text(
                name.ifBlank { stringResource(R.string.chat_file) },
                color = Color.White,
                fontSize = Tokens.Font.label,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                buildList {
                    if (durationMs > 0) add(fmtDuration(durationMs))
                    if (size > 0) add(fmtSizeLong(size))
                }.joinToString(" · "),
                color = HoloSecondary,
                fontSize = Tokens.Font.caption,
            )
        }
        Spacer(Modifier.width(Tokens.Space.s))
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

/** Размер файла словами системы: «4,2 МБ» по-русски, «4.2 MB» по-английски. Свои
 *  «MB/KB» были зашиты латиницей и с точкой при любом языке. */
@Composable
internal fun fmtSizeLong(bytes: Long): String =
    android.text.format.Formatter.formatShortFileSize(LocalContext.current, bytes)

internal fun fmtDuration(ms: Int): String {
    val s = ms / 1000
    return "%d:%02d".format(s / 60, s % 60)
}

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
    val fg = Tokens.Colors.metaOnBubble
    // Стиль задан ЯВНО у обоих текстов: без него «изм.» брал высоту строки из темы
    // (24sp) и мета у отредактированного пузыря вырастала, оставляя пустоту снизу.
    // Моноширинные цифры: время у каждого сообщения своё, а колонка меты общая —
    // на пропорциональных цифрах она слегка гуляла бы от строки к строке.
    val style = Tokens.Text.meta.copy(color = fg)
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (edited) {
            // Текст подменился — молчать об этом нельзя, иначе «исправил»
            // неотличимо от «так и было написано».
            Text(stringResource(R.string.chat_edited), style = style)
            Spacer(Modifier.width(Tokens.Space.xs))
        }
        Text(fmtMsgTime(time), style = style)
        if (mine) {
            Spacer(Modifier.width(Tokens.Space.xxs))
            StatusTick(status)
        }
    }
}

/** Мета поверх фотографии: та же строка на затемнённой плашке. Без неё белое время
 *  пропадает на светлом снимке — а тень читается хуже плашки и на тёмном, и на светлом. */
@Composable
internal fun MetaOnPhoto(content: @Composable () -> Unit) {
    Box(
        Modifier.padding(Tokens.Space.s)
            .clip(RoundedCornerShape(Tokens.Radius.badge))
            .background(Tokens.Colors.mediaScrim)
            .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xxs),
    ) { content() }
}

/** Собеседник печатает: последнее «печатаю» ещё не истекло. Перерисовывается сам, когда
 *  срок выходит, — модели таймер не нужен. */
@Composable
internal fun isTyping(model: AppModel, peer: String): Boolean {
    val until = model.typingUntil[peer] ?: 0L
    var now by remember(until) { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(until) {
        val left = until - System.currentTimeMillis()
        if (left > 0) {
            kotlinx.coroutines.delay(left)
            now = System.currentTimeMillis()
        }
    }
    return now < until
}

/** Статус доставки исходящего сообщения — честно, без приукрашивания.
 *
 *  `queued` (получатель офлайн, сообщение ждёт в очереди) намеренно отличается от
 *  `pending` (уходит прямо сейчас) НЕ только иконкой, но и подписью: раньше оба
 *  рисовались одинаковыми часами, и «ждёт неделю» выглядело как «сейчас отправится».
 *  Галочка `sent` — доставка НЕ подтверждена; ✓✓ ставится только по Ack.
 *  `failed` — выброшено из досылки; в ленте к нему строка «Повторить». */
@Composable
internal fun StatusTick(status: String) {
    val tint = Tokens.Colors.quoteTextOnBubble
    when (status) {
        // Прочитано — те же ✓✓ цветом акцента: отличие только в цвете (как и в списке чатов).
        "read" -> Icon(Icons.Filled.DoneAll, stringResource(R.string.status_read), tint = Tokens.Colors.accent, modifier = Modifier.size(Tokens.Size.iconXs))
        "delivered" -> Icon(Icons.Filled.DoneAll, stringResource(R.string.status_delivered), tint = tint, modifier = Modifier.size(Tokens.Size.iconXs))
        "sent" -> Icon(Icons.Filled.Done, stringResource(R.string.status_sent), tint = tint, modifier = Modifier.size(Tokens.Size.iconXs))
        "pending" -> Icon(Icons.Filled.Schedule, stringResource(R.string.status_sending), tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(Tokens.Size.iconXs))
        // Ждёт принятия запроса в контакты (7i): часы, но не «отправляется».
        "held" -> Icon(TablerIcons.Clock, stringResource(R.string.status_held), tint = Tokens.Colors.heldTick, modifier = Modifier.size(Tokens.Size.iconXs))
        // Только значок, без слова: мета живёт в углу пузыря и должна оставаться
        // узкой — слово растянуло бы её на половину строки. Расшифровка статуса —
        // в описании для screen reader и на экране очереди отправки.
        "queued" -> Icon(
            Icons.Filled.CloudQueue,
            stringResource(R.string.status_queued),
            tint = tint.copy(alpha = 0.7f),
            modifier = Modifier.size(Tokens.Size.iconXs),
        )
        "failed" -> Icon(
            TablerIcons.AlertCircle,
            stringResource(R.string.act_failed),
            tint = Tokens.Colors.failText,
            modifier = Modifier.size(Tokens.Size.iconXs),
        )
    }
}

/** Срез дальних углов пузыря по ширинам соседей в серии ([farCut]); вне ленты — полные углы. */
@Composable
internal fun seriesCut(m: Message, above: Message?, below: Message?): FarCut {
    val widths = LocalBubbleWidths.current ?: return FarCut.None
    fun w(x: Message?) = x?.let { widths[MsgRef(it.id, it.sender)] }
    return farCut(w(m), w(above), w(below))
}
