package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/**
 * Журнал звонков (макет `screens/Calls`): вкладки «Все · Пропущенные», плашка комнаты, где
 * звонок идёт прямо сейчас, строки по дням. Строка — один звонок: схлопывание прятало бы
 * пропущенные. Исключение — подряд идущие звонки одной комнаты: их движок сворачивает в
 * «Группа (N)», иначе один вечер в комнате занимал бы экран.
 */
@Composable
internal fun CallsScreen(model: AppModel, onCallPeer: (String) -> Unit, onJoinRoom: (String) -> Unit) {
    var missedOnly by remember { mutableStateOf(false) }
    // Без разрешения на микрофон звонок стартовал немым: собеседник слышал тишину,
    // а мы не знали почему. Как в профиле и чате — сначала спросить.
    val runWithPerm = rememberPermissionRunner()
    val callPeer: (String) -> Unit = { peer ->
        runWithPerm(arrayOf(android.Manifest.permission.RECORD_AUDIO), R.string.perm_denied_mic) { onCallPeer(peer) }
    }
    val joinRoom: (String) -> Unit = { gid ->
        runWithPerm(arrayOf(android.Manifest.permission.RECORD_AUDIO), R.string.perm_denied_mic) { onJoinRoom(gid) }
    }
    LaunchedEffect(Unit) { model.refreshCallLog() }

    val rows = if (missedOnly) model.callLog.filter { it.outcome == "missed" } else model.callLog.toList()
    // Комнаты, где звонок идёт сейчас (по маячкам), кроме той, где мы сами.
    val live = model.groups.mapNotNull { g ->
        val here = model.groupPresence[g.gid].orEmpty()
        if (here.isEmpty() || g.gid == model.currentRoom) null else g to here.size
    }

    Column(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxWidth()) {
            Row(
                Modifier.fillMaxWidth().padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.xs),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xl),
            ) {
                FolderTab(stringResource(R.string.calls_all), !missedOnly, 0) { missedOnly = false }
                FolderTab(stringResource(R.string.calls_missed), missedOnly, 0) { missedOnly = true }
            }
            HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider)
        }

        LazyColumn(Modifier.fillMaxSize()) {
            items(live, key = { "live_${it.first.gid}" }) { (g, n) ->
                LiveCallCard(model, g, n) { joinRoom(g.gid) }
            }
            if (rows.isEmpty()) {
                item {
                    Box(Modifier.fillParentMaxHeight(if (live.isEmpty()) 1f else 0.7f)) {
                        EmptyState(
                            TablerIcons.Phone,
                            stringResource(R.string.calls_empty_title),
                            stringResource(R.string.calls_empty_hint),
                        )
                    }
                }
            }
            rows.forEachIndexed { i, e ->
                val day = dayKey(e.started)
                val newDay = i == 0 || dayKey(rows[i - 1].started) != day
                if (newDay) {
                    item(key = "day_$day") { DayHeader(dayLabel(e.started)) }
                }
                item(key = "${e.started}_${e.peer}_${e.gid}") {
                    CallRow(model, e, callPeer, joinRoom, divider = !newDay)
                }
            }
            item { Spacer(Modifier.height(Tokens.Space.l)) }
        }
    }
}

/** Плашка «Идёт звонок в «Походы» · 3 участника · Войти» (`screens/Calls`). */
@Composable
private fun LiveCallCard(model: AppModel, g: Group, members: Int, onJoin: () -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.m, bottom = Tokens.Space.xxs)
            .clip(RoundedCornerShape(Tokens.Radius.liveCall))
            .background(Tokens.Colors.liveCall)
            .padding(start = Tokens.Space.m, end = Tokens.Space.s + Tokens.Space.xxs, top = Tokens.Space.s + Tokens.Space.xxs, bottom = Tokens.Space.s + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        SquareAvatar(
            g.name,
            size = Tokens.Size.liveCallAvatar,
            avatarBytes = model.groupPhoto(g.gid),
            bg = Tokens.Colors.groupAvatar,
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs)) {
            Text(
                stringResource(R.string.calls_live_title, g.name),
                color = Tokens.Colors.liveCallTitle,
                fontSize = Tokens.Font.preview,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
            )
            Text(
                pluralStringResource(R.plurals.calls_live_members, members, members),
                color = Tokens.Colors.liveCallSub,
                fontSize = Tokens.Font.small,
            )
        }
        Text(
            stringResource(R.string.calls_join),
            color = Tokens.Colors.onOnline,
            fontSize = Tokens.Font.preview,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.heightIn(min = Tokens.Size.ib)
                .clip(RoundedCornerShape(percent = 50))
                .background(Tokens.Colors.online)
                .clickable(role = Role.Button, onClick = onJoin)
                .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s + Tokens.Space.xxs),
        )
    }
}

/** Заголовок дня: «Сегодня», «Вчера», «3 октября». */
@Composable
private fun DayHeader(text: String) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m + Tokens.Space.xxs, bottom = Tokens.Space.xxs),
    )
}

@Composable
private fun CallRow(
    model: AppModel,
    e: CallEntry,
    onCallPeer: (String) -> Unit,
    onJoinRoom: (String) -> Unit,
    divider: Boolean,
) {
    // Комната — по самой записи (движок отдаёт gid только у комнаты группы), а не по
    // тому, жива ли группа: звонки удалённой группы движок убирает из журнала сам.
    val isRoom = e.gid.isNotEmpty()
    val group = model.groups.firstOrNull { it.gid == e.gid }
    val contact = model.contacts.firstOrNull { it.key == e.peer }
    val name = when {
        isRoom -> group?.name ?: stringResource(R.string.calls_room_gone)
        else -> contact?.displayName ?: shortHex(e.peer)
    }
    val missed = e.outcome == "missed"

    if (divider) {
        HorizontalDivider(
            thickness = 1.dp,
            color = Tokens.Colors.divider,
            modifier = Modifier.padding(start = Tokens.Space.xl + Tokens.Size.callAvatar + Tokens.Space.m + Tokens.Space.xxs),
        )
    }
    Row(
        Modifier.fillMaxWidth().height(Tokens.Size.callRow)
            .padding(start = Tokens.Space.xl, end = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        if (isRoom) {
            // Комната — сущность: скруглённый квадрат. Человек — круг.
            SquareAvatar(name, size = Tokens.Size.callAvatar, avatarBytes = model.groupPhoto(e.gid), bg = Tokens.Colors.groupAvatar)
        } else {
            SquareAvatar(name, size = Tokens.Size.callAvatar, avatarBytes = model.contactAvatars[contact?.avatarId], circle = true)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs)) {
            Text(
                buildAnnotatedString {
                    append(name)
                    if (e.count > 1) {
                        withStyle(SpanStyle(color = Tokens.Colors.tertiary, fontWeight = FontWeight.Normal)) { append(" (${e.count})") }
                    }
                },
                color = if (missed) Tokens.Colors.missed else Color.White,
                fontSize = Tokens.Font.name,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + 1.dp)) {
                val (icon, tint) = when {
                    isRoom -> TablerIcons.Users to Tokens.Colors.tertiary
                    missed -> TablerIcons.ArrowDownLeft to Tokens.Colors.missed
                    e.incoming -> TablerIcons.ArrowDownLeft to Tokens.Colors.online
                    else -> TablerIcons.ArrowUpRight to Tokens.Colors.tertiary
                }
                Icon(icon, null, tint = tint, modifier = Modifier.size(Tokens.Size.iconXs))
                Text(outcomeLabel(e, isRoom), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, maxLines = 1)
            }
        }
        Text(fmtClock(e.started), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.small)
        // Группы больше нет — войти некуда; не-контакту звонок не дойдёт (гейт запросов).
        // В обоих случаях кнопки нет, но место под неё держим — время стоит по одной вертикали.
        val action: (() -> Unit)? = when {
            isRoom && group != null -> ({ onJoinRoom(e.gid) })
            !isRoom && contact != null -> ({ onCallPeer(e.peer) })
            else -> null
        }
        Box(
            Modifier.size(Tokens.Size.callAvatar).clip(RoundedCornerShape(Tokens.Radius.button))
                .then(if (action != null) Modifier.clickable(role = Role.Button, onClick = action) else Modifier),
            contentAlignment = Alignment.Center,
        ) {
            if (action != null) {
                Icon(
                    TablerIcons.Phone,
                    stringResource(if (isRoom) R.string.calls_join_room else R.string.calls_call_back),
                    tint = Tokens.Colors.accent,
                    modifier = Modifier.size(Tokens.Size.iconRow),
                )
            }
        }
    }
}

/** «Входящий · 4:12» / «Пропущенный» / «Без ответа» / «Звонок в комнате · 38:10». */
@Composable
private fun outcomeLabel(e: CallEntry, isRoom: Boolean): String {
    val dur = fmtCallDuration(e.duration)
    return when {
        isRoom -> stringResource(R.string.calls_in_room, dur)
        e.outcome == "missed" -> stringResource(R.string.calls_missed_one)
        e.outcome == "declined" -> stringResource(R.string.calls_declined)
        e.outcome == "unanswered" -> stringResource(R.string.calls_unanswered)
        e.incoming && e.video -> stringResource(R.string.calls_incoming_video, dur)
        e.incoming -> stringResource(R.string.calls_incoming, dur)
        e.video -> stringResource(R.string.calls_outgoing_video, dur)
        else -> stringResource(R.string.calls_outgoing, dur)
    }
}

/** Время звонка «14:20»: день уже сказан заголовком над строками. */
private fun fmtClock(ts: Long): String =
    java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))

/** «4:07» / «1:02:15» — час с лишним в комнате не должен читаться как «62:15». */
internal fun fmtCallDuration(sec: Long): String =
    if (sec >= 3600) "%d:%02d:%02d".format(sec / 3600, sec % 3600 / 60, sec % 60) else "%d:%02d".format(sec / 60, sec % 60)
