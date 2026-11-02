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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.PersonAdd
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
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

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
    // Лента привязана к низу, как в ChatScreen.
    FollowNewest(listState, msgs)
    JumpToMessage(model, listState, msgs)
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
    // Голосовые в группе: удержание, замок, отмена — VoiceSession.
    val voice = rememberVoiceSession { bytes ->
        scope.launch(Dispatchers.IO) {
            model.sendMediaWithMeta(gid, bytes, "audio/mp4", "voice.m4a", MediaMeta(), group = true)
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
                val join = rememberRoomJoin(model)
                RoomBanner(model, presenceMembers) { join(gid) }
            }

            PinnedBar(model, gid, group = true, canUnpin = isAdmin)
            LazyColumn(
                state = listState,
                reverseLayout = true,
                // Поля по бокам даёт ряд сообщения: заливка выбора идёт во всю ширину листа.
                modifier = Modifier.weight(1f).fillMaxWidth(),
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
                    fontSize = Tokens.Font.body,
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
                                picked = emptyMap()
                            }
                        } ?: model.replyTarget?.let { ReplyStrip(model, it) }
                    },
                    onPickFile = { filePicker.launch(arrayOf("*/*")) },
                    onPickAudio = { audioPicker.launch(arrayOf("audio/*")) },
                    onCamera = openCamera,
                    onSendAttachments = { list, caption -> sendGallery(ctx, model, gid, list, scope, caption, group = true) },
                    voice = voice,
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
            val audio = if (isMe) !model.muted else model.roomMembers.firstOrNull { it.peer == key }?.audio ?: true
            val video = if (isMe) model.videoActive else model.roomMembers.firstOrNull { it.peer == key }?.video ?: false
            val photo = if (isMe) model.selfAvatar else model.peerAvatar(key)
            ParticipantFullscreen(name, audio, video, isMe, photo, if (isMe) model.selfName else name) { focusedParticipant = null }
            BackHandler { focusedParticipant = null }
        }
    }
}

/** Шапка группового чата: аватар группы · имя · «N уч · M в сети» (тап → инфо) ·
 *  войти/выйти из комнаты · меню. */
@Composable
internal fun GroupChatTopBar(model: AppModel, onBack: () -> Unit, onInfo: () -> Unit) {
    var confirmLeave by remember { mutableStateOf(false) }
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
        Modifier.fillMaxWidth().background(HoloBg).height(Tokens.Size.topBar).padding(horizontal = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { onInfo() }
                .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(name, size = Tokens.Size.avatarHead, bg = Tokens.Colors.groupAvatar) {
                Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(Tokens.Size.iconM))
            }
            Spacer(Modifier.width(Tokens.Space.m))
            Column(Modifier.weight(1f)) {
                Text(
                    name,
                    color = Color.White,
                    fontSize = Tokens.Font.label,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(pluralOf(R.plurals.group_presence, total.toLong(), onlineN), color = HoloSecondary, fontSize = Tokens.Font.caption)
            }
        }
        if (gid != null) {
            if (model.currentRoom == gid) {
                // «Позвать» жил только на экране ЛИЧНОГО звонка за условием «звонок не
                // личный» — то есть не показывался никогда. Его место — здесь.
                BarIcon(Icons.Filled.PersonAdd, stringResource(R.string.call_btn_invite)) { model.showRingSheet = true }
                BarIcon(Icons.Filled.CallEnd, stringResource(R.string.cd_leave_call)) { model.leaveRoom() }
            } else {
                val join = rememberRoomJoin(model)
                BarIcon(Icons.Filled.Call, stringResource(R.string.cd_join_call)) { join(gid) }
            }
        }
        if (gid != null) {
            Box {
                var menu by remember { mutableStateOf(false) }
                BarIcon(TablerIcons.DotsVertical, stringResource(R.string.common_more), tint = Color.White) { menu = true }
                GroupMenu(model, gid, menu, onClose = { menu = false }, onInfo = onInfo) { confirmLeave = true }
            }
        }
    }
    // Выход уводит из чата сам: модель забывает группу, и навигация уходит с её экранов.
    if (confirmLeave && gid != null) {
        ConfirmSheet(
            title = stringResource(R.string.group_leave),
            text = stringResource(R.string.group_leave_confirm),
            ok = stringResource(R.string.group_leave_ok),
            onOk = { model.leaveGroup(gid) },
            onClose = { confirmLeave = false },
        )
    }
}

/**
 * «⋮» группы (app-ui.md → «Чат группы», макет `shared/GroupMenu`). «Пригласить» и «О группе»
 * пока ведут на один экран участников — разойдутся в шагах 3.4 (шторка приглашения) и
 * 3.1 (витрина группы). Поиск и «Медиа и файлы» — вместе с их экранами (2.9, 3.2).
 */
@Composable
private fun GroupMenu(model: AppModel, gid: String, open: Boolean, onClose: () -> Unit, onInfo: () -> Unit, onLeave: () -> Unit) {
    val muted = model.chatSummaries[model.chatKey(gid, group = true)]?.muted == true
    PopMenu(open, onClose) {
        PopMenuItem(
            TablerIcons.BellOff,
            stringResource(R.string.chat_menu_mute),
            trailing = { Switch(checked = muted, onCheckedChange = null) },
        ) { model.muteChat(gid, group = true, muted = !muted) }
        PopMenuItem(TablerIcons.UserPlus, stringResource(R.string.chat_menu_invite)) {
            onClose()
            onInfo()
        }
        PopMenuItem(TablerIcons.InfoCircle, stringResource(R.string.chat_menu_group_info)) {
            onClose()
            onInfo()
        }
        PopMenuDivider()
        PopMenuItem(TablerIcons.Logout, stringResource(R.string.group_leave), danger = true) {
            onClose()
            onLeave()
        }
    }
}

/** Вход в комнату с запросом микрофона — как звонок из личного чата. Без него
 *  человек входил немым и не понимал, почему его не слышат. */
@Composable
internal fun rememberRoomJoin(model: AppModel): (String) -> Unit {
    val runWithPerm = rememberPermissionRunner()
    return { gid -> runWithPerm(arrayOf(Manifest.permission.RECORD_AUDIO), R.string.perm_denied_mic) { model.joinRoom(gid) } }
}

/** Зелёный баннер активной комнаты: «N в звонке · имена» + Войти. */
@Composable
internal fun RoomBanner(model: AppModel, members: List<RoomMember>, onJoin: () -> Unit) {
    val names = members.joinToString(", ") { rm ->
        model.peerName(rm.peer)
    }
    Row(
        Modifier.fillMaxWidth().background(Tokens.Colors.roomBanner).padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Text(stringResource(R.string.room_in_call, members.size), color = HoloOnline, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium)
            if (names.isNotEmpty()) {
                Text(
                    names,
                    color = HoloSecondary,
                    fontSize = Tokens.Font.caption,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Box(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloOnline)
                .clickable { onJoin() }.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
        ) { Text(stringResource(R.string.room_join), color = Tokens.Colors.onOnline, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium) }
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
        // Без имени группа в списке и в приглашении была бы безымянной строкой —
        // кнопка ждёт хотя бы одну букву.
        AccentButton(stringResource(R.string.create_btn), enabled = name.isNotBlank()) {
            model.createGroup(name.trim())
            onDone()
        }
        Text(
            stringResource(R.string.create_hint),
            color = HoloSecondary,
            fontSize = Tokens.Font.caption,
        )
    }
}

/**
 * Управление группой: участники (с отзывом для админа, ADR-004), приглашение и выход.
 *
 * Отзыв и выход — с подтверждением: оба необратимы (вернуть можно только новым
 * приглашением), а строки стоят вплотную к безобидным. Себя админ отозвать не может —
 * для этого есть «Выйти». В приглашение не попадают те, кто уже в группе.
 */
@Composable
internal fun AddMemberScreen(model: AppModel, onDone: () -> Unit) {
    val gid = model.currentGroup ?: return
    val isAdmin = model.isGroupAdmin(gid)
    val me = model.me?.pubkey
    val memberKeys = model.groupMemberKeys[gid] ?: emptyList()
    val invitable = model.contacts.filter { it.key !in memberKeys }
    var confirmRemove by remember { mutableStateOf<String?>(null) }
    var confirmLeave by remember { mutableStateOf(false) }
    LazyColumn(Modifier.fillMaxSize()) {
        if (memberKeys.isNotEmpty()) {
            item { SectionTitle(stringResource(R.string.group_members_header)) }
            itemsIndexed(memberKeys, key = { _, k -> "m_$k" }) { i, key ->
                CardBlock(first = i == 0, last = i == memberKeys.lastIndex) {
                    CardRow(divider = i > 0, inset = MemberRowInset) {
                        MemberAvatar(model, key)
                        Spacer(Modifier.width(Tokens.Space.m))
                        Text(
                            groupSenderName(model, key),
                            color = Color.White,
                            fontSize = Tokens.Font.label,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        if (isAdmin && key != me) {
                            TextButton(onClick = { confirmRemove = key }) {
                                Text(stringResource(R.string.group_remove_member), color = Tokens.Colors.destructive)
                            }
                        }
                    }
                }
            }
        }
        item { SectionTitle(stringResource(R.string.group_add_header)) }
        if (invitable.isEmpty()) {
            item {
                Text(
                    stringResource(R.string.addmember_empty),
                    color = HoloSecondary,
                    fontSize = Tokens.Font.body,
                    modifier = Modifier.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
                )
            }
        } else {
            itemsIndexed(invitable, key = { _, c -> "c_${c.key}" }) { i, c ->
                CardBlock(first = i == 0, last = i == invitable.lastIndex) {
                    CardRow(
                        onClick = {
                            model.inviteToGroup(gid, c.key)
                            onDone()
                        },
                        divider = i > 0,
                        inset = MemberRowInset,
                    ) {
                        MemberAvatar(model, c.key)
                        Spacer(Modifier.width(Tokens.Space.m))
                        Text(
                            c.displayName,
                            color = Color.White,
                            fontSize = Tokens.Font.label,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        Icon(TablerIcons.Plus, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconS))
                    }
                }
            }
        }
        item {
            // Выход — своей карточкой и с разрывом: он не должен стоять вплотную к
            // списку, где каждый тап — приглашение.
            Spacer(Modifier.height(Tokens.Space.l))
            CardSection {
                CardRow(onClick = { confirmLeave = true }, divider = false) {
                    Text(stringResource(R.string.group_leave), color = Tokens.Colors.destructive, fontSize = Tokens.Font.label)
                }
            }
            Spacer(Modifier.height(Tokens.Space.l))
        }
    }

    confirmRemove?.let { key ->
        ConfirmSheet(
            title = stringResource(R.string.group_remove_title),
            text = stringResource(R.string.group_remove_confirm, model.peerName(key)),
            ok = stringResource(R.string.group_remove_member),
            onOk = { model.removeGroupMember(gid, key) },
            onClose = { confirmRemove = null },
        )
    }
    if (confirmLeave) {
        ConfirmSheet(
            title = stringResource(R.string.group_leave),
            text = stringResource(R.string.group_leave_confirm),
            ok = stringResource(R.string.group_leave_ok),
            onOk = { model.leaveGroup(gid) },
            onClose = { confirmLeave = false },
        )
    }
}

/** Отступ разделителя у строки участника: поле карточки + аватар + зазор. */
private val MemberRowInset = Tokens.Space.m + Tokens.Size.avatarMember + Tokens.Space.m

/** Аватар участника с точкой присутствия — тот же круг, что у человека везде. */
@Composable
private fun MemberAvatar(model: AppModel, key: String) {
    SquareAvatar(
        groupSenderName(model, key),
        size = Tokens.Size.avatarMember,
        online = model.online[key] == true,
        avatarBytes = model.peerAvatar(key),
        circle = true,
    )
}

// ── Входящий звонок (полный экран, Telegram-style) ──────────────────────────
// Активный звонок — см. RoomCallScreen.kt (сетка участников).
