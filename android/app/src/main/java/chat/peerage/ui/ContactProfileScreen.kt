package chat.peerage.ui

import android.Manifest
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/**
 * Профиль контакта — «витрина» (макет `prof/Main`, app-ui.md → «Профиль контакта и
 * группы»): на фоне аватар, имя, статус, ключ и плитки Написать · Звонок · Видео · Звук;
 * на панели — сверка ключа, медиа, общие группы, «Поделиться» и опасное внизу.
 *
 * Сверка — отдельным экраном ([KeyVerifyScreen]): развёрнутая прямо здесь, она занимала
 * пол-экрана ради действия, которое делают один раз за знакомство.
 */
@Composable
internal fun ContactProfileScreen(
    model: AppModel,
    onBack: () -> Unit,
    onMessage: () -> Unit,
    onVerify: () -> Unit,
    onOpenGroup: (String) -> Unit,
    onMedia: () -> Unit,
    onDeleted: () -> Unit,
) {
    val key = model.currentChat ?: return
    val c = model.contacts.firstOrNull { it.key == key } ?: return
    val ctx = LocalContext.current
    val presence = model.presence(key)
    var renaming by remember(key) { mutableStateOf(false) }
    var confirm by remember(key) { mutableStateOf<ProfileDanger?>(null) }
    var commonOpen by remember(key) { mutableStateOf(false) }
    // Режим чата берётся из сводки — она же кормит список на главной, так что плитка
    // здесь и значок на главной не разъедутся.
    val muted = model.chatQuiet(key, false)
    var notify by remember(key) { mutableStateOf(false) }
    if (notify) NotifySheet(model, ChatTarget(key, group = false)) { notify = false }

    // Составы групп грузятся, только когда группу открывают, — без подгрузки здесь
    // «общих групп» было 0 у любого, с кем не заходили в общий чат в этой сессии.
    LaunchedEffect(key) {
        model.refreshChatSummaries()
        model.groups.forEach { model.loadGroupMembers(it.gid) }
    }
    val common = model.groups.filter { key in model.groupMemberKeys[it.gid].orEmpty() }
    val runWithPerm = rememberPermissionRunner()

    ShowcaseScreen(
        onBack = onBack,
        onEdit = { renaming = true },
        editDesc = stringResource(R.string.profile_rename),
        header = {
            val photo = model.contactAvatars[c.avatarId]
            OpenableAvatar(photo) {
                SquareAvatar(
                    c.displayName,
                    size = Tokens.Size.avatarHero,
                    avatarBytes = photo,
                    circle = true, // собеседник — человек
                )
            }
            ShowcaseName(c.displayName)
            Text(
                presenceLabel(presence),
                color = presenceText(presence),
                fontSize = Tokens.Font.body,
            )
            // Ключ — сокращённо и моноширинно; нажатие копирует целиком. Сверяют не по
            // нему, а набором знаков, — он нужен, чтобы его передать.
            Text(
                c.key.take(10) + "…" + c.key.takeLast(6),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.small,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                modifier = Modifier.clip(RoundedCornerShape(Tokens.Radius.inner))
                    .clickable { copyToClipboard(ctx, c.key, "key") }
                    .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xxs),
            )
        },
        actions = {
            ShowAction(TablerIcons.Message, stringResource(R.string.profile_write), primary = true, onClick = onMessage)
            // Микрофон спрашиваем так же, как кнопка звонка в шапке чата: без него
            // звонок уходил немым, и человек не понимал почему.
            ShowAction(TablerIcons.Phone, stringResource(R.string.profile_call)) {
                runWithPerm(arrayOf(Manifest.permission.RECORD_AUDIO), R.string.perm_denied_mic) { model.callDm(key) }
            }
            ShowAction(TablerIcons.Video, stringResource(R.string.profile_video)) {
                runWithPerm(
                    arrayOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.CAMERA),
                    R.string.perm_denied_camera,
                ) { model.callDm(key, video = true) }
            }
            // Режим уведомлений — локальная пометка чата: собеседник о ней не узнаёт.
            ShowAction(
                if (muted) TablerIcons.BellOff else TablerIcons.Bell,
                stringResource(if (muted) R.string.profile_sound_off else R.string.profile_sound),
            ) { notify = true }
        },
    ) {
        CardSection {
            NavRow(
                if (c.verified) TablerIcons.ShieldCheck else TablerIcons.AlertTriangle,
                stringResource(if (c.verified) R.string.profile_key_ok else R.string.profile_key_todo),
                sub = if (c.verified) {
                    verifiedLine(c) ?: stringResource(R.string.profile_key_ok_sub)
                } else {
                    stringResource(R.string.profile_key_todo_sub)
                },
                iconTint = if (c.verified) Tokens.Colors.verified else Tokens.Colors.warn,
                iconBg = if (c.verified) Tokens.Colors.verifiedTile else Tokens.Colors.warnTile,
                divider = false,
                onClick = onVerify,
            )
        }
        CardSection {
            ProfileMediaEntry(model, ChatTarget(key, group = false), onMedia)
            ProfileAutodelRow(model, ChatTarget(key, group = false))
            NavRow(
                TablerIcons.Users,
                stringResource(R.string.profile_common_groups),
                value = "${common.size}",
                onClick = if (common.isNotEmpty()) ({ commonOpen = true }) else null,
            )
            NavRow(TablerIcons.Share, stringResource(R.string.profile_share)) { confirm = ProfileDanger.Share }
        }
        CardSection {
            DangerTextRow(stringResource(R.string.profile_clear_history), divider = false) { confirm = ProfileDanger.Clear }
            DangerTextRow(stringResource(R.string.profile_block)) { confirm = ProfileDanger.Block }
            DangerTextRow(stringResource(R.string.info_delete)) { confirm = ProfileDanger.Delete }
        }
    }

    if (renaming) RenameDialog(model, key, c.displayName) { renaming = false }

    if (commonOpen) {
        ActionSheet(stringResource(R.string.profile_common_groups), "", onClose = { commonOpen = false }) {
            common.forEach { g ->
                PlateButton(g.name, TablerIcons.Users, Modifier.fillMaxWidth()) {
                    commonOpen = false
                    onOpenGroup(g.gid)
                }
            }
        }
    }

    when (confirm) {
        // Делимся ЧИСТЫМ ключом, без имени и пояснений: получатель вставляет строку в
        // «Добавить контакт» как есть. Спрашиваем, потому что раздаётся чужая личность.
        ProfileDanger.Share -> ConfirmSheet(
            title = stringResource(R.string.profile_share),
            text = stringResource(R.string.profile_share_confirm, c.displayName),
            ok = stringResource(R.string.profile_share_ok),
            onOk = { shareText(ctx, c.key) },
            onClose = { confirm = null },
        )
        // Стирается только У СЕБЯ — это и сказано: у собеседника переписка останется.
        ProfileDanger.Clear -> ConfirmSheet(
            title = stringResource(R.string.chat_clear_title),
            text = stringResource(R.string.profile_clear_history_confirm),
            ok = stringResource(R.string.profile_clear_history_ok),
            onOk = { model.clearHistory(key) },
            onClose = { confirm = null },
        )
        ProfileDanger.Block -> ConfirmSheet(
            title = stringResource(R.string.chat_block_title),
            text = stringResource(R.string.profile_block_confirm),
            ok = stringResource(R.string.profile_block),
            onOk = { model.blockRequest(key) },
            onClose = { confirm = null },
        )
        ProfileDanger.Delete -> ConfirmSheet(
            title = stringResource(R.string.profile_delete_title),
            text = stringResource(R.string.profile_delete_confirm),
            ok = stringResource(R.string.folder_delete_ok),
            onOk = {
                model.remove(key)
                onDeleted()
            },
            onClose = { confirm = null },
        )
        null -> {}
    }
}

/** Что из профиля контакта требует подтверждения шторкой. */
private enum class ProfileDanger { Share, Clear, Block, Delete }

@Composable
private fun RenameDialog(model: AppModel, key: String, current: String, onClose: () -> Unit) {
    var name by remember { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(stringResource(R.string.profile_rename)) },
        text = {
            Box(
                Modifier.fillMaxWidth().height(48.dp).clip(RoundedCornerShape(Tokens.Radius.tile))
                    .background(HoloBg).padding(horizontal = Tokens.Space.m),
                contentAlignment = Alignment.CenterStart,
            ) {
                BasicTextField(
                    value = name,
                    onValueChange = { name = it },
                    singleLine = true,
                    textStyle = Tokens.Text.base.copy(color = Color.White),
                    cursorBrush = SolidColor(HoloAccent),
                    keyboardOptions = TextKeyboard,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                model.rename(key, name.trim())
                onClose()
            }) { Text(stringResource(R.string.common_save), color = HoloAccent) }
        },
        dismissButton = {
            TextButton(onClick = onClose) {
                Text(stringResource(R.string.common_cancel), color = HoloSecondary)
            }
        },
    )
}
