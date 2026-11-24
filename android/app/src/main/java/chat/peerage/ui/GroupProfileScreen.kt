package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/** Сколько участников показывать до «Все участники (N)» (app-ui.md → «Профиль группы»). */
private const val MEMBERS_SHOWN = 5

/**
 * Профиль группы — та же «витрина», что у контакта (макет `prof/GroupA`): квадратный
 * аватар, «N участников · M в сети», плитки Звонок · Добавить · Звук · Поиск; на панели —
 * медиа, участники (себя первым, админа меткой) и «Выйти из группы».
 *
 * Отступления от макета до своих шагов: строка «Уведомления» с режимом чата — вместе с
 * режимами (notifications.md), «Изменить» (название и фото) — шаг 3.5. «Добавить» видит только админ: приглашать в MLS-группу может он один.
 */
@Composable
internal fun GroupProfileScreen(
    model: AppModel,
    onBack: () -> Unit,
    onAdd: () -> Unit,
    onSearch: () -> Unit,
    onMedia: () -> Unit,
    onMessage: (String) -> Unit,
    onProfile: (String) -> Unit,
    onLeft: () -> Unit,
    onEdit: () -> Unit,
) {
    val gid = model.currentGroup ?: return
    val g = model.groups.firstOrNull { it.gid == gid } ?: return
    val me = model.me?.pubkey
    val isAdmin = g.admin
    var showAll by remember(gid) { mutableStateOf(false) }
    var sheetFor by remember(gid) { mutableStateOf<String?>(null) }
    var confirmRemove by remember(gid) { mutableStateOf<String?>(null) }
    var confirmLeave by remember(gid) { mutableStateOf(false) }
    var confirmTransfer by remember(gid) { mutableStateOf<String?>(null) }
    LaunchedEffect(gid) {
        model.loadGroupMembers(gid)
        model.refreshChatSummaries()
    }
    val muted = model.chatQuiet(gid, true)
    var notify by remember(gid) { mutableStateOf(false) }
    if (notify) NotifySheet(model, ChatTarget(gid, group = true)) { notify = false }
    val selfOnline = !model.offline
    fun online(k: String) = if (k == me) selfOnline else model.online[k] == true

    // Себя — первым, дальше админ, кто в сети, остальные по имени: в длинном списке
    // нужнее те, кому можно написать сейчас.
    val others = model.groupMemberKeys[gid].orEmpty().filter { it != me }
        .sortedWith(
            compareBy<String> { it != g.adminKey }
                .thenBy { model.online[it] != true }
                .thenBy { model.peerName(it).lowercase() },
        )
    val members = listOfNotNull(me) + others
    val onlineCount = members.count(::online)
    val shown = if (members.size > MEMBERS_SHOWN && !showAll) members.take(MEMBERS_SHOWN) else members
    val join = rememberRoomJoin(model)

    ShowcaseScreen(
        onBack = onBack,
        // Название и фото меняет только админ: остальным их не примут.
        onEdit = if (isAdmin && !g.archived) onEdit else null,
        editDesc = stringResource(R.string.group_edit_desc),
        header = {
            val photo = model.groupPhoto(gid)
            OpenableAvatar(photo, square = true) { SquareAvatar(g.name, size = Tokens.Size.avatarHero, avatarBytes = photo) }
            ShowcaseName(g.name)
            Row {
                Text(
                    pluralStringResource(R.plurals.group_members_n, members.size, members.size) + " · ",
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                )
                Text(stringResource(R.string.group_online_n, onlineCount), color = Tokens.Colors.online, fontSize = Tokens.Font.body)
            }
        },
        actions = {
            if (!g.archived) ShowAction(TablerIcons.Phone, stringResource(R.string.profile_call), primary = true) { join(gid) }
            if (isAdmin && !g.archived) ShowAction(TablerIcons.UserPlus, stringResource(R.string.group_add), onClick = onAdd)
            ShowAction(
                if (muted) TablerIcons.BellOff else TablerIcons.Bell,
                stringResource(if (muted) R.string.profile_sound_off else R.string.profile_sound),
            ) { notify = true }
            ShowAction(TablerIcons.Search, stringResource(R.string.group_search), onClick = onSearch)
        },
    ) {
        CardSection {
            ProfileNotifyRow(model, ChatTarget(gid, group = true))
            ProfileMediaEntry(model, ChatTarget(gid, group = true), onMedia)
            ProfileAutodelRow(model, ChatTarget(gid, group = true))
        }
        Column {
            // Подпись группы стоит вплотную к ней (макет): общий зазор между группами её
            // не касается, поэтому не SectionTitle с его верхним полем.
            Text(
                stringResource(R.string.group_members_title, members.size),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(start = Tokens.Space.panelText, end = Tokens.Space.panelText, bottom = Tokens.Space.s),
            )
            CardSection {
                if (isAdmin && !g.archived) {
                    NavRow(
                        TablerIcons.Plus,
                        stringResource(R.string.group_add_member),
                        iconTint = Tokens.Colors.accent,
                        iconBg = Tokens.Colors.accentMuted,
                        labelColor = Tokens.Colors.accent,
                        divider = false,
                        onClick = onAdd,
                    )
                }
                shown.forEachIndexed { i, k ->
                    MemberRow(
                        model,
                        k,
                        self = k == me,
                        admin = k == g.adminKey,
                        online = online(k),
                        divider = i > 0 || (isAdmin && !g.archived),
                    ) { if (k != me) sheetFor = k }
                }
                if (shown.size < members.size) {
                    CardRow(onClick = { showAll = true }, inset = ProfileMemberInset) {
                        Text(
                            stringResource(R.string.group_all_members, members.size),
                            color = Tokens.Colors.accent,
                            fontSize = Tokens.Font.name,
                        )
                    }
                }
            }
        }
        CardSection {
            DangerTextRow(stringResource(R.string.group_leave), divider = false) { confirmLeave = true }
        }
    }

    sheetFor?.let { k ->
        MemberSheet(
            model,
            k,
            online(k),
            admin = isAdmin,
            onClose = { sheetFor = null },
            onMessage = { onMessage(k) },
            onProfile = { onProfile(k) },
            onRemove = { confirmRemove = k },
            onTransfer = { confirmTransfer = k },
        )
    }
    confirmTransfer?.let { k ->
        ConfirmSheet(
            title = stringResource(R.string.group_transfer_title),
            text = stringResource(R.string.group_transfer_text, model.peerName(k)),
            ok = stringResource(R.string.group_transfer_ok),
            onOk = { model.transferAdmin(gid, k) },
            onClose = { confirmTransfer = null },
        )
    }
    confirmRemove?.let { k ->
        ConfirmSheet(
            title = stringResource(R.string.group_remove_title),
            text = stringResource(R.string.group_remove_confirm, model.peerName(k)),
            ok = stringResource(R.string.group_remove_member),
            onOk = { model.removeGroupMember(gid, k) },
            onClose = { confirmRemove = null },
        )
    }
    if (confirmLeave) {
        LeaveGroupSheet(model, gid, onLeft = onLeft, onClose = { confirmLeave = false })
    }
}

/** Отступ разделителя у строки участника: поле карточки + аватар + зазор. */
private val ProfileMemberInset = Tokens.Space.l + Tokens.Size.avatarMember + Tokens.Space.m

/** Участник группы: аватар с точкой присутствия, имя («(вы)» у себя), статус, метка «админ». */
@Composable
private fun MemberRow(
    model: AppModel,
    key: String,
    self: Boolean,
    admin: Boolean,
    online: Boolean,
    divider: Boolean,
    onClick: () -> Unit,
) {
    // У себя — своё имя, а не «Вы»: «(вы)» уже стоит рядом, а буква аватара — от имени.
    val name = if (self) model.selfName.ifBlank { model.peerName(key) } else model.peerName(key)
    CardRow(onClick = onClick, divider = divider, inset = ProfileMemberInset) {
        SquareAvatar(
            name,
            size = Tokens.Size.avatarMember,
            online = online,
            avatarBytes = model.peerAvatar(key),
            circle = true,
        )
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    name,
                    color = Color.White,
                    fontSize = Tokens.Font.name,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (self) Text(" " + stringResource(R.string.group_you_mark), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name)
            }
            Text(
                stringResource(if (online) R.string.status_in_network else R.string.status_offline),
                color = if (online) Tokens.Colors.online else Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
            )
        }
        if (admin) {
            Text(
                stringResource(R.string.group_admin_mark),
                color = Tokens.Colors.accentSoft,
                fontSize = Tokens.Font.caption,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(start = Tokens.Space.s)
                    .clip(RoundedCornerShape(Tokens.Radius.inner))
                    .background(Tokens.Colors.accentMuted)
                    .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xxs),
            )
        }
    }
}

/**
 * Шторка участника (макет `people/Member`): аватар, имя, «в сети · в контактах»; меню —
 * Написать · Профиль, админу ещё «Удалить из группы» с пояснением про смену ключей.
 * «Передать права админа» — шаг 3.5.
 */
@Composable
private fun MemberSheet(
    model: AppModel,
    key: String,
    online: Boolean,
    admin: Boolean,
    onClose: () -> Unit,
    onMessage: () -> Unit,
    onProfile: () -> Unit,
    onRemove: () -> Unit,
    onTransfer: () -> Unit,
) {
    val name = model.peerName(key)
    // Незнакомцу из группы написать нельзя: гейт контактов пропустит только запрос.
    val contact = model.contacts.any { it.key == key }
    SheetFrame(onClose) {
        Row(
            Modifier.padding(horizontal = Tokens.Space.xs).padding(top = Tokens.Space.s - Tokens.Space.xxs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            SquareAvatar(name, size = Tokens.Size.avatarSheet, avatarBytes = model.peerAvatar(key), circle = true)
            Column {
                Text(name, color = Color.White, fontSize = Tokens.Font.sheetName, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    stringResource(if (online) R.string.status_in_network else R.string.status_offline) + " · " +
                        stringResource(if (contact) R.string.group_member_in_contacts else R.string.group_member_not_contact),
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                )
            }
        }
        if (contact || admin) {
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.raised)) {
                if (contact) {
                    PopMenuItem(TablerIcons.Message, stringResource(R.string.profile_write)) {
                        onClose()
                        onMessage()
                    }
                    PopMenuItem(TablerIcons.User, stringResource(R.string.group_member_profile)) {
                        onClose()
                        onProfile()
                    }
                }
                if (admin) {
                    PopMenuItem(TablerIcons.Crown, stringResource(R.string.group_transfer)) {
                        onClose()
                        onTransfer()
                    }
                    HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.group)
                    PopMenuItem(TablerIcons.Trash, stringResource(R.string.group_remove_from), danger = true) {
                        onClose()
                        onRemove()
                    }
                }
            }
        }
        if (admin) {
            Text(
                stringResource(R.string.group_remove_hint),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs),
            )
        }
    }
}
