package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.CallMade
import androidx.compose.material.icons.automirrored.filled.CallReceived
import androidx.compose.material.icons.automirrored.filled.Login
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/**
 * История звонков (макет `docs/design/mockups/calls.html`).
 *
 * Строка = ОДИН звонок, а не «последний по контакту»: схлопывание серии прячет
 * пропущенные, а именно они здесь важнее всего. Комнаты живут в том же списке, но
 * правая кнопка у них не «перезвонить», а «Войти» — комната это постоянный звонок,
 * и он может идти прямо сейчас.
 */
@Composable
internal fun CallsScreen(model: AppModel, onCallPeer: (String) -> Unit, onJoinRoom: (String) -> Unit) {
    var missedOnly by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { model.refreshCallLog() }

    val rows = if (missedOnly) model.callLog.filter { it.outcome == "missed" } else model.callLog.toList()

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad, vertical = Tokens.Space.m),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            FilterChip(stringResource(R.string.calls_all), !missedOnly) { missedOnly = false }
            FilterChip(stringResource(R.string.calls_missed), missedOnly) { missedOnly = true }
        }

        if (rows.isEmpty()) {
            // Пустой экран говорит о приватности, а не «здесь ничего нет»: журнал
            // локальный, и это то, чем мы отличаемся от обычного мессенджера.
            EmptyState(
                Icons.Filled.Call,
                stringResource(R.string.calls_empty_title),
                stringResource(R.string.calls_empty_hint),
            )
            return
        }

        // Журнал — один контейнер-карточка, как список чатов: это такой же
        // перечень строк, и различаться формой они не должны.
        LazyColumn(Modifier.fillMaxSize()) {
            itemsIndexed(rows, key = { _, e -> "${e.started}_${e.peer}_${e.gid}" }) { i, e ->
                CardBlock(first = i == 0, last = i == rows.lastIndex) {
                    CallRow(model, e, onCallPeer, onJoinRoom, divider = i > 0)
                }
            }
            item { Spacer(Modifier.height(Tokens.Space.l)) }
        }
    }
}

@Composable
private fun CallRow(
    model: AppModel,
    e: CallEntry,
    onCallPeer: (String) -> Unit,
    onJoinRoom: (String) -> Unit,
    divider: Boolean = false,
) {
    val isRoom = e.gid.isNotEmpty() && model.groups.any { it.gid == e.gid }
    val group = model.groups.firstOrNull { it.gid == e.gid }
    val contact = model.contacts.firstOrNull { it.key == e.peer }
    val name = if (isRoom) group?.name.orEmpty() else contact?.displayName ?: e.peer.take(12)
    val missed = e.outcome == "missed"

    CardRow(divider = divider, inset = 70.dp) {
        if (isRoom) {
            // Комната — сущность: скруглённый квадрат. Человек — круг.
            SquareAvatar(name, size = Tokens.Size.avatarRow, bg = Color(0xFF0D2A0D)) {
                Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
            }
        } else {
            SquareAvatar(
                name,
                size = Tokens.Size.avatarRow,
                avatarBytes = model.contactAvatars[contact?.avatarId],
                circle = true,
                ring = Tokens.Colors.surface,
            )
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Text(
                name,
                color = if (missed) HoloError else Color.White,
                fontSize = 15.sp,
                fontWeight = FontWeight.Medium,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                val tint = when {
                    missed -> HoloError
                    e.incoming -> HoloOnline
                    else -> HoloSecondary
                }
                Icon(
                    if (e.incoming) Icons.AutoMirrored.Filled.CallReceived else Icons.AutoMirrored.Filled.CallMade,
                    null,
                    tint = tint,
                    modifier = Modifier.size(15.dp),
                )
                Spacer(Modifier.width(5.dp))
                Text(outcomeLabel(e, isRoom), color = tint, fontSize = 13.sp)
            }
        }
        Text(fmtCallTime(e.started), color = Tokens.Colors.tertiary, fontSize = 12.sp)
        Spacer(Modifier.width(Tokens.Space.m))
        if (isRoom) {
            // Комната может звучать прямо сейчас — предлагаем войти, а не перезвонить.
            Row(
                Modifier.clip(RoundedCornerShape(Tokens.Radius.tile))
                    .background(HoloOnline.copy(alpha = 0.14f))
                    .clickable { onJoinRoom(e.gid) }
                    .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.AutoMirrored.Filled.Login, null, tint = HoloOnline, modifier = Modifier.size(17.dp))
                Spacer(Modifier.width(5.dp))
                Text(stringResource(R.string.calls_join), color = HoloOnline, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            }
        } else {
            Box(
                Modifier.size(38.dp).clip(RoundedCornerShape(Tokens.Radius.button))
                    .background(Tokens.Colors.surfaceHi)
                    .clickable { onCallPeer(e.peer) },
                contentAlignment = Alignment.Center,
            ) {
                Icon(Icons.Filled.Call, stringResource(R.string.profile_call), tint = HoloAccent, modifier = Modifier.size(19.dp))
            }
        }
    }
}

/** «Входящий · 12:04» / «Пропущенный» / «Не дозвонился» / «Звонок в комнате · 48:12». */
@Composable
private fun outcomeLabel(e: CallEntry, isRoom: Boolean): String {
    val dur = fmtCallDuration(e.duration)
    return when {
        isRoom -> stringResource(R.string.calls_in_room, dur)
        e.outcome == "missed" -> stringResource(R.string.calls_missed_one)
        e.outcome == "declined" -> stringResource(R.string.calls_declined)
        e.outcome == "unanswered" -> stringResource(R.string.calls_unanswered)
        e.incoming -> stringResource(R.string.calls_incoming, dur)
        else -> stringResource(R.string.calls_outgoing, dur)
    }
}

@Composable
private fun FilterChip(label: String, on: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.pill))
            .background(if (on) Tokens.Colors.accentBadge else HoloSurface)
            .clickable { onClick() }
            .padding(horizontal = Tokens.Space.m, vertical = 6.dp),
    ) {
        Text(label, color = if (on) HoloAccent else HoloSecondary, fontSize = 13.sp)
    }
}

private fun fmtCallDuration(sec: Long): String = "%d:%02d".format(sec / 60, sec % 60)

private fun fmtCallTime(ts: Long): String {
    val now = System.currentTimeMillis() / 1000
    val fmt = if (dayKey(ts) == dayKey(now)) "HH:mm" else "d MMM"
    return java.text.SimpleDateFormat(fmt, java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))
}
