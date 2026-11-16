package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Права админа группы (шаг 7j, макеты `people/GroupEdit`, `Leave`): правка названия и фото,
 * выход админа с передачей прав. Админ в группе один: без него удалять и закреплять некому.
 */

/** Правка группы (макет `people/GroupEdit`): фото, название, «Готово» в шапке. Только админ. */
@Composable
internal fun GroupEditScreen(model: AppModel, onBack: () -> Unit) {
    val gid = model.currentGroup ?: return
    val g = model.groups.firstOrNull { it.gid == gid } ?: return
    var name by remember(gid) { mutableStateOf(g.name) }
    // Новое фото ждёт «Готово»: до него группа у всех прежняя.
    var photo by remember(gid) { mutableStateOf<ByteArray?>(null) }
    val pick = rememberImagePicker { photo = it }
    val shown = photo ?: model.groupPhoto(gid)
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        TopBar(stringResource(R.string.group_edit_title), onBack = onBack) {
            PillButton(stringResource(R.string.common_done), enabled = name.isNotBlank(), modifier = Modifier.padding(end = Tokens.Space.xs)) {
                if (name.trim() != g.name) model.renameGroup(gid, name)
                photo?.let { model.setGroupPhoto(gid, it, "image/jpeg") }
                onBack()
            }
        }
        Column(
            Modifier.weight(1f).fillMaxWidth()
                .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                .background(Tokens.Colors.panel)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.panel),
        ) {
            Box(
                Modifier.align(Alignment.CenterHorizontally)
                    .padding(top = Tokens.Space.l, bottom = Tokens.Space.xxl)
                    .clickable(onClickLabel = stringResource(R.string.group_edit_photo), onClick = pick),
            ) {
                SquareAvatar(name.ifBlank { g.name }, size = Tokens.Size.groupEditAvatar, avatarBytes = shown)
                Box(
                    Modifier.align(Alignment.BottomEnd)
                        .offset(x = Tokens.Space.xs, y = Tokens.Space.xs)
                        .size(Tokens.Size.groupEditBadge)
                        .clip(CircleShape)
                        .background(Tokens.Colors.accent)
                        .border(Tokens.Size.badgeRing, Tokens.Colors.bg, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(TablerIcons.Camera, null, tint = Tokens.Colors.accentDark, modifier = Modifier.size(Tokens.Size.iconS))
                }
            }
            Text(
                stringResource(R.string.group_edit_name),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(start = Tokens.Space.s - Tokens.Space.xxs, bottom = Tokens.Space.s),
            )
            Box(
                Modifier.fillMaxWidth().heightIn(min = Tokens.Size.formField)
                    .clip(RoundedCornerShape(Tokens.Radius.field))
                    .background(Tokens.Colors.surface)
                    .padding(horizontal = Tokens.Space.l),
                contentAlignment = Alignment.CenterStart,
            ) {
                BasicTextField(
                    name,
                    { name = it },
                    singleLine = true,
                    textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.name),
                    cursorBrush = SolidColor(Tokens.Colors.accent),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            Text(
                stringResource(R.string.group_edit_caption),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(start = Tokens.Space.s - Tokens.Space.xxs, top = Tokens.Space.s),
            )
        }
    }
}

/**
 * «Выйти из группы» — одно на «⋮» чата и профиль группы. Админу при живых участниках —
 * шторка `people/Leave`: без админа группа теряет удаление и закреп, поэтому сначала
 * предлагается преемник. Остальным — обычное подтверждение.
 */
@Composable
internal fun LeaveGroupSheet(model: AppModel, gid: String, onLeft: () -> Unit, onClose: () -> Unit) {
    val g = model.groups.firstOrNull { it.gid == gid }
    val me = model.me?.pubkey
    val others = model.groupMemberKeys[gid].orEmpty().filter { it != me }
    if (g == null || !g.admin || others.isEmpty()) {
        ConfirmSheet(
            title = stringResource(R.string.group_leave),
            text = stringResource(R.string.group_leave_confirm),
            ok = stringResource(R.string.group_leave_ok),
            onOk = {
                model.leaveGroup(gid)
                onLeft()
            },
            onClose = onClose,
        )
        return
    }
    // Преемник по умолчанию — первый, кто в сети: передача дойдёт сразу.
    var heir by remember(gid) { mutableStateOf(others.firstOrNull { model.online[it] == true } ?: others.first()) }
    SheetFrame(onClose) {
        Text(
            stringResource(R.string.group_leave_admin_title, g.name),
            color = Color.White,
            fontSize = Tokens.Font.sheet,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
        Text(
            stringResource(R.string.group_leave_admin_text),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.preview,
            lineHeight = Tokens.Font.lineHeight,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.raised)) {
            others.forEachIndexed { i, k ->
                if (i > 0) HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.group)
                HeirRow(model, k, on = k == heir) { heir = k }
            }
        }
        PlateButton(stringResource(R.string.group_leave_transfer), modifier = Modifier.fillMaxWidth(), danger = true) {
            model.leaveGroup(gid, heir)
            onClose()
            onLeft()
        }
        PlateButton(stringResource(R.string.group_leave_no_transfer), modifier = Modifier.fillMaxWidth()) {
            model.leaveGroup(gid)
            onClose()
            onLeft()
        }
    }
}

/** Кандидат в преемники: аватар, имя, присутствие, кружок выбора. */
@Composable
private fun HeirRow(model: AppModel, key: String, on: Boolean, onClick: () -> Unit) {
    val name = model.peerName(key)
    val online = model.online[key] == true
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.personRow).clickable(onClick = onClick)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SquareAvatar(name, size = Tokens.Size.avatarPick, online = online, avatarBytes = model.peerAvatar(key), circle = true)
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f)) {
            Text(name, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                stringResource(if (online) R.string.status_in_network else R.string.status_offline),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
            )
        }
        SelectCheck(on)
    }
}
