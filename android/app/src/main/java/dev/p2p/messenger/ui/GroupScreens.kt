@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

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
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import kotlin.math.abs

@Composable
internal fun GroupsScreen(model: AppModel, onOpen: (String) -> Unit) {
    if (model.groups.isEmpty()) {
        CenterMessage("Нет групп. Создайте новую.")
    } else {
        LazyColumn(Modifier.fillMaxSize()) {
            items(model.groups, key = { it.gid }) { g ->
                ListRow(onClick = { onOpen(g.gid) }) {
                    Icon(Icons.Filled.Group, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
                    Spacer(Modifier.width(12.dp))
                    Text(g.name, color = Color.White, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    Text("${g.members} уч.", color = HoloSecondary, fontSize = 12.sp)
                }
            }
        }
    }
}

@Composable
internal fun GroupChatScreen(model: AppModel) {
    val gid = model.currentGroup ?: return
    val msgs = model.groupMessages[gid] ?: emptyList()
    var input by remember(gid) { mutableStateOf("") }
    var focusedParticipant by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    LaunchedEffect(msgs.size) { if (msgs.isNotEmpty()) listState.animateScrollToItem(msgs.size) }

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

            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
            ) {
                itemsIndexed(msgs) { i, m ->
                    val prev = msgs.getOrNull(i - 1)
                    if (prev == null || dayLabel(prev.time) != dayLabel(m.time)) DateChip(dayLabel(m.time))
                    GroupMessageRow(model, m)
                }
            }
            MessageInput(input, { input = it }) {
                if (input.isNotBlank()) {
                    model.sendGroupText(gid, input)
                    input = ""
                }
            }
        }
        focusedParticipant?.let { key ->
            val isMe = key == "me"
            val name = if (isMe) {
                "Вы"
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
    val gid = model.currentGroup
    val g = gid?.let { model.groups.firstOrNull { it.gid == gid } }
    val name = g?.name ?: "Группа"
    // Агрегат «всего/в сети» считает движок (groupPresence); до первого тика —
    // фолбэк на число участников из списка групп.
    val roster = model.currentGroupRoster
    val total = roster?.total ?: g?.members ?: 1
    val onlineN = roster?.online ?: 1
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, "Назад", onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).clickable { onInfo() }
                .padding(horizontal = 4.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(name, size = 38.dp, bg = Color(0xFF0D2A0D)) {
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
                Text("$total уч. · $onlineN в сети", color = HoloSecondary, fontSize = 11.sp)
            }
        }
        if (gid != null) {
            if (model.currentRoom == gid) {
                BarIcon(Icons.Filled.CallEnd, "Выйти из звонка") { model.leaveRoom() }
            } else {
                BarIcon(Icons.Filled.Call, "Войти в звонок") { model.joinRoom(gid) }
            }
        }
        BarIcon(Icons.Filled.MoreVert, "Ещё", tint = Color.White, onClick = onInfo)
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
            Text("${members.size} в звонке", color = HoloOnline, fontSize = 13.sp, fontWeight = FontWeight.Medium)
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
            Modifier.clip(RoundedCornerShape(10.dp)).background(HoloOnline)
                .clickable { onJoin() }.padding(horizontal = 18.dp, vertical = 8.dp),
        ) { Text("Войти", color = Color(0xFF0A1F0A), fontSize = 14.sp, fontWeight = FontWeight.Medium) }
    }
}

/** Строка группового сообщения: системная / своя / входящая (имя цветом + аватар). */
@Composable
internal fun GroupMessageRow(model: AppModel, m: GroupMsg) {
    when {
        m.system -> {
            val who = groupSenderName(model, m.sender)
            val text = if (m.text.isBlank()) "$who обновил(а) состав группы" else "$who ${m.text}"
            Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
                Box(
                    Modifier.clip(RoundedCornerShape(10.dp)).background(HoloSurface)
                        .padding(horizontal = 10.dp, vertical = 4.dp),
                ) {
                    Text(text, color = HoloSecondary, fontSize = 11.sp)
                }
            }
        }
        m.mine -> Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), horizontalArrangement = Arrangement.End) {
            GroupBubble(m.text, true, m.time)
        }
        // Входящее: аватар выровнен по НИЗУ пузыря (Telegram-style), имя — ВНУТРИ пузыря.
        else -> Row(
            Modifier.fillMaxWidth().padding(vertical = 3.dp),
            horizontalArrangement = Arrangement.Start,
            verticalAlignment = Alignment.Bottom,
        ) {
            val who = groupSenderName(model, m.sender)
            val online = model.online[m.sender] == true
            SquareAvatar(who, size = 32.dp, online = online)
            Spacer(Modifier.width(8.dp))
            GroupBubble(m.text, false, m.time, senderName = who, senderColor = nameColor(m.sender))
        }
    }
}

/** Пузырь группового сообщения. Имя отправителя (цветом) — встроено сверху внутрь
 *  пузыря; время — в правом нижнем углу (как DM); без галочек. */
@Composable
internal fun GroupBubble(
    text: String,
    mine: Boolean,
    time: Long,
    senderName: String? = null,
    senderColor: Color = Color.White,
) {
    Box(
        Modifier.widthIn(max = 280.dp).clip(RoundedCornerShape(14.dp))
            .background(if (mine) BubbleOut else BubbleIn).padding(horizontal = 10.dp, vertical = 6.dp),
    ) {
        Column {
            if (senderName != null) {
                Text(
                    senderName,
                    color = senderColor,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(end = 40.dp),
                )
            }
            // Текст резервирует справа место под время (реальный отступ) → время
            // оверлеем точно в правом-нижнем углу, без наложения.
            Text(text, color = Color.White, fontSize = 15.sp, modifier = Modifier.padding(end = 40.dp))
        }
        Text(
            fmtMsgTime(time),
            color = Color.White.copy(alpha = 0.5f),
            fontSize = 10.sp,
            modifier = Modifier.align(Alignment.BottomEnd),
        )
    }
}

/** Имя отправителя группового сообщения по hex-ключу. */
internal fun groupSenderName(model: AppModel, hex: String): String = when {
    hex.isEmpty() -> "Участник"
    hex == model.me?.pubkey -> "Вы"
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
        FieldLabel("НАЗВАНИЕ")
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
        AccentButton("Создать") {
            model.createGroup(name)
            onDone()
        }
        Text(
            "Вы станете администратором. Добавляйте участников из контактов внутри группы.",
            color = HoloSecondary,
            fontSize = 11.sp,
        )
    }
}

@Composable
internal fun AddMemberScreen(model: AppModel, onDone: () -> Unit) {
    val gid = model.currentGroup ?: return
    if (model.contacts.isEmpty()) {
        CenterMessage("Нет контактов для приглашения.")
    } else {
        LazyColumn(Modifier.fillMaxSize()) {
            items(model.contacts, key = { it.key }) { c ->
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
    }
}

// ── Входящий звонок (полный экран, Telegram-style) ──────────────────────────
// Активный звонок — см. RoomCallScreen.kt (сетка участников).
