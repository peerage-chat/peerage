@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package dev.p2p.messenger.ui

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Reply
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlin.math.abs

/** Прочитать uri (байты+mime+имя) в IO и отправить как вложение в группу (M2). */
private fun sendGroupUriAttachment(
    ctx: android.content.Context,
    model: AppModel,
    gid: String,
    uri: android.net.Uri,
    scope: kotlinx.coroutines.CoroutineScope,
    fallbackName: String,
    caption: String = "",
) {
    scope.launch(Dispatchers.IO) {
        val mime = ctx.contentResolver.getType(uri) ?: "application/octet-stream"
        val name = displayName(ctx, uri) ?: fallbackName
        // Картинки — через память: им нужен scrub (EXIF/GPS), а он требует файла
        // целиком. Всё остальное — потоком, как в личке.
        if (mime.startsWith("image/")) {
            val bytes = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
            model.sendGroupMedia(gid, bytes, mime, name, MediaMeta.extract(ctx, uri, mime, bytes), caption)
            return@launch
        }
        // Метаданные (длительность/кадр/превью) достаём здесь: без них у участников
        // будет карточка «скачать» без единой подсказки, что именно им предлагают.
        val cached = cacheFromUri(ctx, uri, name) ?: return@launch
        val meta = MediaMeta.extract(ctx, uri, mime, ByteArray(0))
        model.sendGroupMediaFile(gid, cached.absolutePath, mime, name, meta, caption)
    }
}

@Composable
internal fun GroupsScreen(model: AppModel, onOpen: (String) -> Unit) {
    if (model.groups.isEmpty()) {
        CenterMessage(stringResource(R.string.groups_empty))
    } else {
        LazyColumn(Modifier.fillMaxSize()) {
            items(model.groups, key = { it.gid }) { g ->
                ListRow(onClick = { onOpen(g.gid) }) {
                    Icon(Icons.Filled.Group, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
                    Spacer(Modifier.width(12.dp))
                    Text(g.name, color = Color.White, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    Text(
                        if (g.archived) {
                            stringResource(R.string.group_archived)
                        } else {
                            stringResource(R.string.members_count, g.members)
                        },
                        color = HoloSecondary,
                        fontSize = 12.sp,
                    )
                }
            }
        }
    }
}

@Composable
internal fun GroupChatScreen(model: AppModel) {
    val gid = model.currentGroup ?: return
    val msgs = model.groupMessages[gid] ?: emptyList()
    val archived = model.groups.firstOrNull { it.gid == gid }?.archived == true
    val isAdmin = model.groups.firstOrNull { it.gid == gid }?.admin == true
    LaunchedEffect(gid) { model.refreshPins(gid, group = true) }
    var input by remember(gid) { mutableStateOf("") }
    // Правка начинается с текущего текста: человек исправляет, а не набирает заново.
    LaunchedEffect(model.editTargetGroup) { model.editTargetGroup?.let { input = it.text } }
    var focusedParticipant by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    // Прокрутка вниз — только на НОВОЕ сообщение (см. ChatScreen: по размеру списка
    // лента прыгала бы при догрузке старых).
    val lastKey = msgs.lastOrNull()?.let { it.sender to it.id }
    LaunchedEffect(lastKey) { if (msgs.isNotEmpty()) listState.animateScrollToItem(msgs.size) }
    PagingUp(listState, gid) { done -> model.loadOlderGroup(gid, done) }

    // Вложения в группе — тот же лист, что в DM (движок пушит блоб всем участникам).
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendGroupUriAttachment(ctx, model, gid, it, scope, "file") }
    }
    val audioPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendGroupUriAttachment(ctx, model, gid, it, scope, "audio") }
    }
    val camUri = remember { mutableStateOf<android.net.Uri?>(null) }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        if (ok) camUri.value?.let { sendGroupUriAttachment(ctx, model, gid, it, scope, "photo.jpg") }
    }
    val camPerm = rememberPermissionRunner()
    val openCamera = {
        camPerm(arrayOf(Manifest.permission.CAMERA), R.string.perm_denied_camera) {
            camUri.value = newCameraUri(ctx)
            camUri.value?.let { camera.launch(it) }
        }
    }
    // Голосовые в группе: held-to-record.
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
            recorder.stop()?.let { bytes -> scope.launch(Dispatchers.IO) { model.sendGroupMedia(gid, bytes, "audio/mp4", "voice.m4a") } }
        }
    }

    val inRoom = model.currentRoom == gid
    val presenceMembers = if (inRoom) emptyList() else (model.groupPresence[gid] ?: emptyList())

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            // В комнате — сетка звонка; иначе если есть присутствие — зелёный баннер.
            if (inRoom) {
                RoomGrid(model, true, emptyList(), onFocus = { focusedParticipant = it })
            } else if (presenceMembers.isNotEmpty()) {
                RoomBanner(model, presenceMembers) { model.joinRoom(gid) }
            }

            PinnedBar(model, gid, group = true, canUnpin = isAdmin)
            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
            ) {
                itemsIndexed(msgs) { i, m ->
                    val prev = msgs.getOrNull(i - 1)
                    val next = msgs.getOrNull(i + 1)
                    if (prev == null || dayKey(prev.time) != dayKey(m.time)) DateChip(dayLabel(m.time))
                    // Склейка блоков. Системная строка — разделитель: у неё нет
                    // отправителя, поэтому соседние сообщения в один блок не сходятся.
                    val who = { g: GroupMsg -> if (g.system) null else g.sender }
                    val pos = blockPos(
                        withPrev = prev != null && !m.system &&
                            continuesBlock(who(prev), prev.time, m.sender, m.time),
                        withNext = next != null && !m.system && !next.system &&
                            continuesBlock(m.sender, m.time, next.sender, next.time),
                    )
                    GroupMessageRow(model, m, pos)
                }
            }
            ForwardSheet(model)
            if (archived) {
                // Группа из резервной копии: история есть, MLS-состояния нет (ADR-004).
                // Показать поле ввода значило бы обещать отправку, которой не будет.
                Text(
                    stringResource(R.string.group_archived),
                    color = HoloSecondary,
                    fontSize = 13.sp,
                    modifier = Modifier.fillMaxWidth().padding(Tokens.Space.m),
                    textAlign = TextAlign.Center,
                )
            } else {
                MessageInput(
                    value = input,
                    onChange = { input = it },
                    replyStrip = {
                        model.editTargetGroup?.let { t ->
                            EditStrip(t.text) {
                                model.editTargetGroup = null
                                input = ""
                            }
                        } ?: model.groupReplyTarget?.let { GroupReplyStrip(model, it) }
                    },
                    onPickFile = { filePicker.launch(arrayOf("*/*")) },
                    onPickAudio = { audioPicker.launch(arrayOf("audio/*")) },
                    onCamera = openCamera,
                    onSendAttachments = { list, caption ->
                        // Подпись — только на первом вложении, как в DM: серия из пяти
                        // фото с одной подписью на каждом читалась бы как спам.
                        list.forEachIndexed { i, g ->
                            sendGroupUriAttachment(
                                ctx,
                                model,
                                gid,
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
                    onSend = {
                        val editing = model.editTargetGroup
                        if (editing != null) {
                            if (input.isNotBlank()) model.applyEdit(gid, true, editing.id, input)
                            model.editTargetGroup = null
                            input = ""
                        } else if (input.isNotBlank()) {
                            model.sendGroupText(gid, input)
                            input = ""
                        }
                    },
                )
            }
        }
        focusedParticipant?.let { key ->
            val isMe = key == "me"
            val name = if (isMe) {
                stringResource(R.string.common_you)
            } else {
                model.contacts.firstOrNull { it.key == key }?.name ?: (key.take(6) + "…")
            }
            val audio = if (isMe) true else model.roomMembers.firstOrNull { it.peer == key }?.audio ?: true
            val video = if (isMe) false else model.roomMembers.firstOrNull { it.peer == key }?.video ?: false
            ParticipantFullscreen(name, audio, video, isMe) { focusedParticipant = null }
        }
    }
}

/** Шапка группового чата: аватар группы · имя · «N уч · M в сети» (тап → инфо) ·
 *  войти/выйти из комнаты · меню. */
@Composable
internal fun GroupChatTopBar(model: AppModel, onBack: () -> Unit, onInfo: () -> Unit) {
    if (model.groupSelection.isNotEmpty()) {
        GroupSelectionTopBar(model)
        return
    }
    val gid = model.currentGroup
    val g = gid?.let { model.groups.firstOrNull { it.gid == gid } }
    val name = g?.name ?: stringResource(R.string.group_default_name)
    // Агрегат «всего/в сети» считает движок (groupPresence); до первого тика —
    // фолбэк на число участников из списка групп.
    val roster = model.currentGroupRoster
    val total = roster?.total ?: g?.members ?: 1
    val onlineN = roster?.online ?: 1
    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(56.dp).padding(horizontal = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { onInfo() }
                .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(name, size = Tokens.Size.avatarHead, bg = Color(0xFF0D2A0D)) {
                Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(20.dp))
            }
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    name,
                    color = Color.White,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(stringResource(R.string.group_presence, total, onlineN), color = HoloSecondary, fontSize = 11.sp)
            }
        }
        if (gid != null) {
            if (model.currentRoom == gid) {
                BarIcon(Icons.Filled.CallEnd, stringResource(R.string.cd_leave_call)) { model.leaveRoom() }
            } else {
                BarIcon(Icons.Filled.Call, stringResource(R.string.cd_join_call)) { model.joinRoom(gid) }
            }
        }
        BarIcon(Icons.Filled.MoreVert, stringResource(R.string.common_more), tint = Color.White, onClick = onInfo)
    }
}

/** Зелёный баннер активной комнаты: «N в звонке · имена» + Войти. */
@Composable
internal fun RoomBanner(model: AppModel, members: List<RoomMember>, onJoin: () -> Unit) {
    val names = members.joinToString(", ") { rm ->
        model.contacts.firstOrNull { it.key == rm.peer }?.name ?: (rm.peer.take(6) + "…")
    }
    Row(
        Modifier.fillMaxWidth().background(Color(0xFF12331A)).padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(stringResource(R.string.room_in_call, members.size), color = HoloOnline, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            if (names.isNotEmpty()) {
                Text(
                    names,
                    color = HoloSecondary,
                    fontSize = 11.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Spacer(Modifier.width(12.dp))
        Box(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloOnline)
                .clickable { onJoin() }.padding(horizontal = 18.dp, vertical = 8.dp),
        ) { Text(stringResource(R.string.room_join), color = Color(0xFF0A1F0A), fontSize = 14.sp, fontWeight = FontWeight.Medium) }
    }
}

/** Строка группового сообщения: системная / своя / входящая (имя цветом + аватар). */
@Composable
internal fun GroupMessageRow(model: AppModel, m: GroupMsg, pos: BlockPos = BlockPos.Solo) {
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
        m.mine -> Row(Modifier.fillMaxWidth().padding(bottom = blockGap(pos)), horizontalArrangement = Arrangement.End) {
            GroupBubble(m, model, true, m.time, pos = pos)
        }
        // Входящее: аватар — ТОЛЬКО у нижнего сообщения блока (Telegram-style), у
        // остальных на его месте пустая распорка, чтобы левый край блока не гулял.
        // Имя отправителя — только на ПЕРВОМ сообщении блока, внутри пузыря.
        else -> Row(
            Modifier.fillMaxWidth().padding(bottom = blockGap(pos)),
            horizontalArrangement = Arrangement.Start,
            verticalAlignment = Alignment.Bottom,
        ) {
            val who = groupSenderName(model, m.sender)
            val online = model.online[m.sender] == true
            val avatarBottom = pos == BlockPos.Solo || pos == BlockPos.Last
            if (avatarBottom) {
                SquareAvatar(who, size = Tokens.Size.avatarSeries, online = online, circle = true, ring = HoloBg)
            } else {
                Spacer(Modifier.width(Tokens.Size.avatarSeries))
            }
            Spacer(Modifier.width(8.dp))
            GroupBubble(
                m,
                model,
                false,
                m.time,
                senderName = if (pos == BlockPos.Solo || pos == BlockPos.First) who else null,
                senderColor = nameColor(m.sender),
                pos = pos,
            )
        }
    }
}

/**
 * Действия над выделенными сообщениями группы. Отличие от DM: «удалить у всех»
 * доступно, только когда все выбранные — НАШИ (автор берётся из MLS, чужое отозвать
 * нельзя), а ответ — только при одном выбранном.
 */
@Composable
private fun GroupSelectionTopBar(model: AppModel) {
    val gid = model.currentGroup ?: return
    val ctx = LocalContext.current
    val sel = model.groupSelection
    val mineKey = model.me?.pubkey.orEmpty()
    var confirmDelete by remember { mutableStateOf(false) }

    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.Filled.Close, stringResource(R.string.common_cancel)) { model.clearGroupSelection() }
        Text(
            sel.size.toString(),
            color = Color.White,
            fontSize = 18.sp,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f).padding(start = 8.dp),
        )
        BarIcon(Icons.AutoMirrored.Filled.Send, stringResource(R.string.forward_action)) {
            model.beginForwardGroup(gid)
        }
        // Править можно только своё: подменять чужие слова — не редактирование.
        if (sel.size == 1 && sel.first().sender == mineKey) {
            val ref = sel.first()
            BarIcon(Icons.Filled.Edit, stringResource(R.string.chat_edit)) {
                model.groupMessages[gid]?.firstOrNull { it.id == ref.id && it.sender == ref.sender }
                    ?.let { model.editTargetGroup = it }
                model.clearGroupSelection()
            }
        }
        // Закрепляет только админ — у остальных кнопки нет, чтобы она не обещала
        // того, что движок всё равно отклонит.
        if (sel.size == 1 && model.groups.firstOrNull { it.gid == gid }?.admin == true) {
            val ref = sel.first()
            val msg = model.groupMessages[gid]
                ?.firstOrNull { it.id == ref.id && it.sender == ref.sender }
            val already = model.pinned[gid].orEmpty()
                .any { it.id == ref.id && it.sender == ref.sender }
            BarIcon(TablerIcons.Pin, stringResource(R.string.pin_action)) {
                msg?.let { model.setPinnedGroup(gid, it, !already) }
                model.clearGroupSelection()
            }
        }
        if (sel.size == 1) {
            BarIcon(Icons.AutoMirrored.Filled.Reply, stringResource(R.string.chat_reply)) {
                val ref = sel.first()
                model.groupMessages[gid]?.firstOrNull { it.id == ref.id && it.sender == ref.sender }
                    ?.let { model.startGroupReply(it) }
            }
        }
        BarIcon(Icons.Filled.ContentCopy, stringResource(R.string.chat_copy)) {
            val text = model.groupSelectedText(gid)
            if (text.isNotEmpty()) {
                val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                    as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("p2p", text))
                android.widget.Toast
                    .makeText(ctx, R.string.chat_copied, android.widget.Toast.LENGTH_SHORT).show()
            }
            model.clearGroupSelection()
        }
        BarIcon(Icons.Filled.Delete, stringResource(R.string.chat_delete)) { confirmDelete = true }
    }

    if (confirmDelete) {
        val allMine = sel.all { it.sender == mineKey }
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(R.string.chat_delete_title, sel.size)) },
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
                            model.retractGroupSelected(gid)
                            confirmDelete = false
                        }) { Text(stringResource(R.string.chat_delete_for_all), color = HoloError) }
                    }
                    TextButton(onClick = {
                        model.deleteGroupSelected(gid)
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

/** Полоска «отвечаем на …» над полем ввода группы. */
@Composable
private fun GroupReplyStrip(model: AppModel, m: GroupMsg) {
    val who = if (m.sender == model.me?.pubkey) {
        stringResource(R.string.chat_reply_you)
    } else {
        groupSenderName(model, m.sender)
    }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloSurface).padding(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(2.dp).height(30.dp).background(HoloAccent))
        Column(Modifier.weight(1f).padding(horizontal = 8.dp)) {
            Text(who, color = HoloAccent, fontSize = 12.sp, fontWeight = FontWeight.Medium)
            Text(
                m.text.ifEmpty { m.name.ifEmpty { quoteKind(m.kind) } },
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
            modifier = Modifier.size(20.dp).clickable { model.cancelGroupReply() },
        )
    }
}

/** Цитата в групповом пузыре: имя автора оригинала + начало текста. */
@Composable
private fun GroupQuote(model: AppModel, m: GroupMsg, modifier: Modifier = Modifier) {
    val src = model.groupMessages[model.currentGroup]
        ?.firstOrNull { it.id == m.replyTo && it.sender == m.replySender }
    val who = if (m.replySender == model.me?.pubkey) {
        stringResource(R.string.chat_reply_you)
    } else {
        groupSenderName(model, m.replySender)
    }
    // Те же правила, что у цитаты в личном чате: подложка и текст — полупрозрачный
    // белый, чтобы плашка читалась на цветном пузыре.
    Row(
        modifier.padding(bottom = 4.dp).height(IntrinsicSize.Min).heightIn(min = 30.dp)
            .clip(RoundedCornerShape(Tokens.Radius.chip))
            .background(Color.White.copy(alpha = 0.10f)),
    ) {
        Box(Modifier.width(2.5.dp).fillMaxHeight().background(HoloAccent))
        val hasThumb = src != null && src.kind != "text" &&
            QuoteThumb(model, src.blobId, src.thumbId)
        Column(Modifier.padding(horizontal = 9.dp, vertical = 4.dp)) {
            Text(who, color = HoloAccent, fontSize = 12.sp, fontWeight = FontWeight.Medium)
            Text(
                src?.let {
                    when {
                        it.text.isNotEmpty() -> it.text
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

/** Пузырь группового сообщения. Имя отправителя (цветом) — встроено сверху внутрь
 *  пузыря; время — в правом нижнем углу (как DM); без галочек. */
@Composable
internal fun GroupBubble(
    m: GroupMsg,
    model: AppModel,
    mine: Boolean,
    time: Long,
    senderName: String? = null,
    senderColor: Color = Color.White,
    pos: BlockPos = BlockPos.Solo,
) {
    // M2: байты вложения из реактивного кэша (появятся по докачке блоба без рефреша).
    val blobBytes = if (m.blobId.isNotEmpty()) model.mediaBlobs[m.blobId] else null
    var viewer by remember { mutableStateOf(false) }
    val selecting = model.groupSelection.isNotEmpty()
    val selected = GroupMsgRef(m.id, m.sender) in model.groupSelection
    // В группе галочек нет: доставку каждому из участников мы не отслеживаем, и
    // одна галочка означала бы больше, чем знает движок.
    val meta: @Composable () -> Unit = { MsgMeta(time, mine = false, edited = m.editedAt > 0) }
    val pending = !m.downloaded && blobBytes == null && m.blobId.isNotEmpty() && m.kind != "text"
    // Фото без подписи — само себе пузырь (как в личном чате). Имя автора, цитата
    // и пометка пересылки уходят шапкой НАД снимком, на своей подложке, — картинка
    // при этом остаётся голой.
    val photoBubble = m.kind == "image" && !pending && m.text.isEmpty()
    val photoHeader = photoBubble &&
        (m.fwdFrom.isNotEmpty() || m.fwdAnon || m.replyTo != 0L || senderName != null)
    Box(
        Modifier.widthIn(max = 280.dp).clip(bubbleShape(mine, pos))
            .then(if (photoBubble) Modifier else Modifier.background(if (mine) BubbleOut else BubbleIn))
            .then(
                if (selected) Modifier.border(1.dp, HoloAccent, bubbleShape(mine, pos)) else Modifier,
            )
            .combinedClickable(
                onLongClick = { model.toggleGroupSelect(m) },
                onClick = { if (selecting) model.toggleGroupSelect(m) },
            )
            .then(if (photoBubble) Modifier else Modifier.padding(horizontal = 10.dp, vertical = 6.dp)),
    ) {
        Column {
            val head: @Composable () -> Unit = {
                ForwardMark(model, m.fwdFrom, m.fwdAnon)
                if (m.replyTo != 0L) {
                    GroupQuote(model, m, if (photoHeader) Modifier.fillMaxWidth() else Modifier)
                }
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
                // Подложка — только под шапкой, шириной со снимок.
                Column(
                    Modifier.width(PhotoBubbleSize)
                        .background(if (mine) BubbleOut else BubbleIn)
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                ) { head() }
            } else if (!photoBubble) {
                head()
            }
            // Крупное вложение в группе не качается само: карточка с именем/размером/
            // превью и кнопкой — решение принимает получатель, а не тот, кто отправил
            // видео сразу пятерым (то же правило, что в DM).
            Crossfade(pending, animationSpec = tween(Tokens.Anim.normal), label = "attach") { isPending ->
                when {
                    isPending -> PendingAttachmentCard(
                        model,
                        m.kind,
                        m.name,
                        m.size,
                        m.durationMs,
                        m.thumbId,
                        m.blobId,
                    ) { model.downloadMedia(m.sender, m.blobId) }
                    else -> when (m.kind) {
                        // В режиме выделения тап по вложению переключает выбор,
                        // а не открывает файл.
                        "image" -> ImageAttachment(
                            blobBytes,
                            onOpen = { if (selecting) model.toggleGroupSelect(m) else viewer = true },
                            bare = photoBubble,
                            onLongClick = { model.toggleGroupSelect(m) },
                        )
                        "voice" -> VoiceAttachment(blobBytes)
                        "file" -> FileAttachment(m.name, blobBytes) { model.toggleGroupSelect(m) }
                        // Текст обтекает мету — перенос считает сам движок вёрстки.
                        else -> BubbleText(m.text) { meta() }
                    }
                }
            }
            // Подпись к вложению — в том же пузыре под содержимым: она пришла ОДНИМ
            // сообщением с файлом, значит и показывается как его часть.
            if (m.kind != "text" && m.text.isNotEmpty()) {
                BubbleText(m.text, modifier = Modifier.padding(top = 6.dp)) { meta() }
            }
        }
        if (viewer && blobBytes != null) ImageViewer(blobBytes) { viewer = false }
        // Мета — в правом нижнем углу ПУЗЫРЯ (место в тексте под неё уже оставлено):
        // имя автора и метка «Переслано» делают пузырь шире текста, и прижатая к
        // концу строки мета висела бы посередине. На голом фото — на затемнении.
        Box(Modifier.align(Alignment.BottomEnd)) {
            if (photoBubble) MetaOnPhoto { meta() } else meta()
        }
    }
}

/** Имя отправителя группового сообщения по hex-ключу. */
@Composable
internal fun groupSenderName(model: AppModel, hex: String): String = when {
    hex.isEmpty() -> stringResource(R.string.group_member_fallback)
    hex == model.me?.pubkey -> stringResource(R.string.common_you)
    else -> model.contacts.firstOrNull { it.key == hex }?.name ?: (hex.take(6) + "…")
}

/** Яркий детерминированный цвет имени отправителя (Discord-style). */
internal val NameColors = listOf(
    Color(0xFF5BC0EB),
    Color(0xFF9BC53D),
    Color(0xFFE5793A),
    Color(0xFFEF6FB3),
    Color(0xFF7C5CFC),
    Color(0xFF20C997),
    Color(0xFFF6C453),
)
internal fun nameColor(hex: String): Color =
    if (hex.isEmpty()) HoloSecondary else NameColors[abs(hex.hashCode()) % NameColors.size]

/** Сетка участников комнаты (2 колонки). Тап по тайлу → onFocus(key).
 *  [inRoom] — мы сейчас в комнате (показываем себя + model.roomMembers).
 *  [presenceMembers] — участники по маячкам когда мы ещё не зашли. */

@Composable
internal fun CreateGroupScreen(model: AppModel, onDone: () -> Unit) {
    var name by remember { mutableStateOf("") }
    Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
        FieldLabel(stringResource(R.string.create_label_name))
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
        AccentButton(stringResource(R.string.create_btn)) {
            model.createGroup(name)
            onDone()
        }
        Text(
            stringResource(R.string.create_hint),
            color = HoloSecondary,
            fontSize = 11.sp,
        )
    }
}

/** Управление группой: текущие участники (с отзывом для админа, ADR-004) + добавление. */
@Composable
internal fun AddMemberScreen(model: AppModel, onDone: () -> Unit) {
    val gid = model.currentGroup ?: return
    val isAdmin = model.isGroupAdmin(gid)
    val memberKeys = model.groupMemberKeys[gid] ?: emptyList()
    LazyColumn(Modifier.fillMaxSize()) {
        if (memberKeys.isNotEmpty()) {
            item { SectionHeader(stringResource(R.string.group_members_header)) }
            items(memberKeys, key = { "m_$it" }) { key ->
                ListRow(onClick = {}) {
                    StatusDot(model.online[key] == true)
                    Spacer(Modifier.width(12.dp))
                    Text(model.peerName(key), color = Color.White, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    if (isAdmin) {
                        TextButton(onClick = { model.removeGroupMember(gid, key) }) {
                            Text(
                                stringResource(R.string.group_remove_member),
                                color = Color(0xFFE0563B),
                                fontSize = 14.sp,
                            )
                        }
                    }
                }
            }
        }
        item { SectionHeader(stringResource(R.string.group_add_header)) }
        if (model.contacts.isEmpty()) {
            item {
                Text(
                    stringResource(R.string.addmember_empty),
                    color = HoloSecondary,
                    fontSize = 14.sp,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
        } else {
            items(model.contacts, key = { "c_${it.key}" }) { c ->
                ListRow(onClick = {
                    model.inviteToGroup(gid, c.key)
                    onDone()
                }) {
                    StatusDot(model.online[c.key] == true)
                    Spacer(Modifier.width(12.dp))
                    Text(c.name, color = Color.White, fontSize = 16.sp)
                }
            }
        }
        item {
            // Выход из группы (ADR-004): currentGroup=null → MainActivity уводит на список.
            ListRow(onClick = { model.leaveGroup(gid) }) {
                Text(
                    stringResource(R.string.group_leave),
                    color = Color(0xFFE0563B),
                    fontSize = 16.sp,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun SectionHeader(text: String) {
    Text(
        text,
        color = HoloSecondary,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
    )
}

// ── Входящий звонок (полный экран, Telegram-style) ──────────────────────────
// Активный звонок — см. RoomCallScreen.kt (сетка участников).
