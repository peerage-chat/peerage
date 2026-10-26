@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package chat.peerage.ui

import android.Manifest
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
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
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

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

/** Лента группы. `onOpenPeer` — тап по «@имени»: открыть того, кого упомянули. */
@Composable
internal fun GroupChatScreen(model: AppModel, onOpenPeer: (String) -> Unit = {}) {
    // В режиме выделения тап по пузырю переключает выбор — и по «@» тоже, иначе
    // упоминание было бы дырой, через которую выделение внезапно уводит из чата.
    val onMention = { key: String -> if (model.selection.isEmpty()) onOpenPeer(key) }
    CompositionLocalProvider(LocalMentionClick provides onMention) { GroupChatContent(model) }
}

@Composable
private fun GroupChatContent(model: AppModel) {
    val gid = model.currentGroup ?: return
    val msgs = model.chat(gid, group = true)
    val archived = model.groups.firstOrNull { it.gid == gid }?.archived == true
    val isAdmin = model.groups.firstOrNull { it.gid == gid }?.admin == true
    LaunchedEffect(gid) { model.refreshPins(gid, group = true) }
    var input by remember(gid) { mutableStateOf("") }
    // Кого человек выбрал из подсказки «@»: ключ → вставленное имя. При отправке
    // каждое «@имя», ещё живое в тексте, становится ссылкой на этого человека.
    var picked by remember(gid) { mutableStateOf(mapOf<String, String>()) }
    // Правка начинается с текущего текста: человек исправляет, а не набирает заново.
    // Упоминания своего сообщения поднимаются вместе с ним — иначе правка опечатки
    // молча снимала бы их.
    LaunchedEffect(model.editTarget) {
        model.editTarget?.let {
            input = it.text
            picked = Mentions.picked(it.text, it.mentions)
        }
    }
    val members = model.groupMemberKeys[gid].orEmpty()
    val mentionCandidates = { q: String ->
        members.filter { it != model.me?.pubkey }
            .map { it to model.peerName(it) }
            // Имя ищем с начала любого слова: «@пет» находит «Иван Петров».
            .filter { (_, name) -> q.isEmpty() || name.split(' ').any { w -> w.startsWith(q, ignoreCase = true) } }
            .sortedBy { it.second.lowercase() }
    }
    var focusedParticipant by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    // Лента привязана к низу, как в ChatScreen: вниз — только на НОВОЕ сообщение.
    val lastKey = msgs.lastOrNull()?.let { it.sender to it.id }
    LaunchedEffect(lastKey) { if (msgs.isNotEmpty()) listState.animateScrollToItem(0) }
    LaunchedEffect(model.jumpTo) {
        val target = model.jumpTo
        if (target != 0L) {
            val i = msgs.indexOfFirst { it.id == target }
            if (i >= 0) listState.animateScrollToItem(msgs.size - 1 - i)
            model.jumpTo = 0L
        }
    }
    PagingOlder(listState, gid) { model.loadOlder(gid, {}, group = true) }

    // Вложения в группе — тот же лист, что в DM (движок пушит блоб всем участникам).
    val filePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendUriAttachment(ctx, model, gid, it, scope, "file", group = true) }
    }
    val audioPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri?.let { sendUriAttachment(ctx, model, gid, it, scope, "audio", group = true) }
    }
    val camUri = remember { mutableStateOf<android.net.Uri?>(null) }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        if (ok) camUri.value?.let { sendUriAttachment(ctx, model, gid, it, scope, "photo.jpg", group = true) }
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
            recorder.stop()?.let { bytes ->
                scope.launch(Dispatchers.IO) {
                    model.sendMediaWithMeta(gid, bytes, "audio/mp4", "voice.m4a", MediaMeta(), group = true)
                }
            }
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
                reverseLayout = true,
                modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
            ) {
                // От свежего к старому; i — индекс в хронологии (см. ChatScreen).
                // Ключ — пара «автор + id»: состояние пузыря (просмотрщик, голосовое) и
                // позиция прокрутки держатся за сообщение, а не за место в ленте.
                items(msgs.size, key = { j -> msgs[msgs.size - 1 - j].let { "${it.sender}:${it.id}" } }) { j ->
                    val i = msgs.size - 1 - j
                    val m = msgs[i]
                    val prev = msgs.getOrNull(i - 1)
                    val next = msgs.getOrNull(i + 1)
                    Column {
                        if (prev == null || dayKey(prev.time) != dayKey(m.time)) DateChip(dayLabel(m.time))
                        // Склейка блоков. Системная строка — разделитель: у неё нет
                        // отправителя, поэтому соседние сообщения в один блок не сходятся.
                        val who = { g: Message -> if (g.system) null else g.sender }
                        val pos = blockPos(
                            withPrev = prev != null && !m.system &&
                                continuesBlock(who(prev), prev.time, m.sender, m.time),
                            withNext = next != null && !m.system && !next.system &&
                                continuesBlock(m.sender, m.time, next.sender, next.time),
                        )
                        MessageRow(model, m, pos, group = true)
                    }
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
                        model.editTarget?.let { t ->
                            EditStrip(t.text) {
                                model.editTarget = null
                                input = ""
                            }
                        } ?: model.replyTarget?.let { ReplyStrip(model, it) }
                    },
                    onPickFile = { filePicker.launch(arrayOf("*/*")) },
                    onPickAudio = { audioPicker.launch(arrayOf("audio/*")) },
                    onCamera = openCamera,
                    onSendAttachments = { list, caption ->
                        // Подпись — только на первом вложении, как в DM: серия из пяти
                        // фото с одной подписью на каждом читалась бы как спам.
                        list.forEachIndexed { i, g ->
                            sendUriAttachment(
                                ctx,
                                model,
                                gid,
                                g.uri,
                                scope,
                                if (g.isVideo) "video.mp4" else "photo.jpg",
                                caption = if (i == 0) caption else "",
                                group = true,
                            )
                        }
                    },
                    recording = recording,
                    onVoiceStart = startVoice,
                    onVoiceStop = stopVoice,
                    model = model,
                    mentionCandidates = mentionCandidates,
                    onMentionPicked = { key, name -> picked = picked + (key to name) },
                    onSend = {
                        val editing = model.editTarget
                        val mentions = Mentions.collect(input, picked)
                        if (editing != null) {
                            if (input.isNotBlank()) model.applyEdit(gid, true, editing.id, input, mentions)
                            model.editTarget = null
                            input = ""
                        } else if (input.isNotBlank()) {
                            model.send(gid, input, group = true, mentions = mentions)
                            input = ""
                        }
                        picked = emptyMap()
                    },
                )
            }
        }
        focusedParticipant?.let { key ->
            val isMe = key == "me"
            val name = if (isMe) {
                stringResource(R.string.common_you)
            } else {
                model.peerName(key)
            }
            val audio = if (isMe) true else model.roomMembers.firstOrNull { it.peer == key }?.audio ?: true
            val video = if (isMe) false else model.roomMembers.firstOrNull { it.peer == key }?.video ?: false
            val photo = if (isMe) model.selfAvatar else model.peerAvatar(key)
            ParticipantFullscreen(name, audio, video, isMe, photo) { focusedParticipant = null }
            BackHandler { focusedParticipant = null }
        }
    }
}

/** Шапка группового чата: аватар группы · имя · «N уч · M в сети» (тап → инфо) ·
 *  войти/выйти из комнаты · меню. */
@Composable
internal fun GroupChatTopBar(model: AppModel, onBack: () -> Unit, onInfo: () -> Unit) {
    if (model.selection.isNotEmpty()) {
        SelectionTopBar(model, group = true)
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
            SquareAvatar(name, size = Tokens.Size.avatarHead, bg = Tokens.Colors.groupAvatar) {
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
        model.peerName(rm.peer)
    }
    Row(
        Modifier.fillMaxWidth().background(Tokens.Colors.roomBanner).padding(horizontal = 14.dp, vertical = 10.dp),
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
        ) { Text(stringResource(R.string.room_join), color = Tokens.Colors.onOnline, fontSize = 14.sp, fontWeight = FontWeight.Medium) }
    }
}

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
                                color = Tokens.Colors.destructive,
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
                    Text(c.displayName, color = Color.White, fontSize = 16.sp)
                }
            }
        }
        item {
            // Выход из группы (ADR-004): currentGroup=null → MainActivity уводит на список.
            ListRow(onClick = { model.leaveGroup(gid) }) {
                Text(
                    stringResource(R.string.group_leave),
                    color = Tokens.Colors.destructive,
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
