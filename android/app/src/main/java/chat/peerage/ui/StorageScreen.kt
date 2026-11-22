package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/**
 * Шапка «Хранилища» (макеты `screens/Storage`, `autodel/Main`): заголовок, итог и полоса
 * состава — над панелью, на фоне экрана. Вопрос человека — «что съело место», поэтому
 * состав читается раньше любого списка.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun StorageHeader(model: AppModel, onBack: () -> Unit) {
    val total = model.storageBlobBytes
    val kinds = model.storageByKind
    Column(Modifier.fillMaxWidth().background(Tokens.Colors.bg)) {
        TopBar(stringResource(R.string.menu_storage), onBack = onBack)
        Column(
            Modifier.padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.s, bottom = Tokens.Space.xl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                Text(fmtSizeLong(total), color = Color.White, fontSize = Tokens.Font.huge, fontWeight = FontWeight.Bold)
                Text(
                    stringResource(R.string.storage_total_caption),
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                    modifier = Modifier.padding(bottom = Tokens.Space.xs),
                )
            }
            if (total > 0 && kinds.isNotEmpty()) {
                Row(
                    Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(Tokens.Radius.badge)),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xxs),
                ) {
                    kinds.forEach { (kind, bytes) ->
                        Box(Modifier.fillMaxHeight().weight(bytes.toFloat().coerceAtLeast(0.001f)).background(kindColor(kind)))
                    }
                }
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
                ) {
                    kinds.forEach { (kind, bytes) ->
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs)) {
                            Box(Modifier.size(8.dp).clip(RoundedCornerShape(2.dp)).background(kindColor(kind)))
                            Text(
                                "${kindLabel(kind)} ${fmtSizeLong(bytes)}",
                                color = Tokens.Colors.kindLegend,
                                fontSize = Tokens.Font.small,
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * «Хранилище» на панели: копия данных и автоудаление, вес по чатам, очистка вложений.
 * Сама очистка — только по кнопке и с подтверждением; автоудаление — по правилу,
 * которое человек включил сам.
 */
@Composable
internal fun StorageScreen(model: AppModel, onBackup: () -> Unit, onAutodel: () -> Unit) {
    var confirm by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        model.refreshStorage()
        model.refreshAutodel()
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.panel, bottom = Tokens.Space.xxl),
    ) {
        CardSection {
            val hasBackup = model.dataBackupAt > 0
            NavRow(
                TablerIcons.Database,
                stringResource(R.string.storage_backup),
                sub = if (hasBackup) {
                    stringResource(R.string.storage_backup_last, fmtDayShort(model.dataBackupAt))
                } else {
                    stringResource(R.string.storage_backup_none)
                },
                divider = false,
                onClick = onBackup,
            )
            NavRow(
                TablerIcons.Clock,
                stringResource(R.string.autodel_title),
                sub = autodelSummary(model.autodel.rule),
                onClick = onAutodel,
            )
        }

        if (model.storageChats.isNotEmpty()) {
            SectionTitle(stringResource(R.string.storage_by_chat))
            CardSection {
                model.storageChats.forEachIndexed { i, sc ->
                    StorageChatRow(model, sc, divider = i > 0)
                }
            }
        }

        val canClear = model.storageBlobs > 0
        Spacer(Modifier.height(Tokens.Space.m + Tokens.Space.xxs))
        CardSection {
            CardRow(onClick = if (canClear) ({ confirm = true }) else null, divider = false) {
                Column(Modifier.weight(1f)) {
                    Text(
                        stringResource(R.string.storage_clear_attachments),
                        color = if (canClear) Tokens.Colors.accent else Tokens.Colors.tertiary,
                        fontSize = Tokens.Font.name,
                    )
                    Text(
                        stringResource(R.string.storage_clear_sub),
                        color = Tokens.Colors.tertiary,
                        fontSize = Tokens.Font.body,
                        modifier = Modifier.padding(top = Tokens.Space.xxs),
                    )
                }
            }
        }
    }

    if (confirm) {
        ConfirmSheet(
            title = stringResource(R.string.storage_clear_title),
            text = stringResource(R.string.storage_clear_sub),
            ok = stringResource(R.string.storage_clear_ok),
            onOk = { model.clearAttachments() },
            onClose = { confirm = false },
        )
    }
}

/** Строка «По чатам»: аватар чата (группа — квадратом), имя, сколько весят вложения. */
@Composable
private fun StorageChatRow(model: AppModel, sc: StorageChat, divider: Boolean) {
    val (name, avatar) = chatFace(model, ChatTarget(sc.id, sc.group))
    CardRow(divider = divider, inset = Tokens.Space.l + Tokens.Size.rowIcon + Tokens.Space.m) {
        SquareAvatar(
            name,
            size = Tokens.Size.rowIcon,
            avatarBytes = avatar,
            circle = !sc.group,
            bg = if (sc.group) Tokens.Colors.groupAvatar else null,
        )
        Spacer(Modifier.width(Tokens.Space.m))
        Text(name, color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f), maxLines = 1)
        Text(fmtSizeLong(sc.bytes), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.option)
    }
}

/** Имя и фото чата: контакт — его имя и аватар, группа — название и фото. */
@Composable
internal fun chatFace(model: AppModel, chat: ChatTarget): Pair<String, ByteArray?> =
    if (chat.group) {
        val g = model.groups.firstOrNull { it.gid == chat.id }
        (g?.name ?: chat.id.take(12)) to model.groupPhoto(chat.id)
    } else {
        val c = model.contacts.firstOrNull { it.key == chat.id }
        (c?.displayName ?: chat.id.take(12)) to model.contactAvatars[c?.avatarId]
    }

/** Дата «1 августа 2026» — человеку важен возраст копии, а не время суток. */
internal fun fmtDay(ts: Long): String =
    java.text.SimpleDateFormat("d MMMM yyyy", java.util.Locale.getDefault())
        .format(java.util.Date(ts * 1000))

/** «3 октября» в этом году, «3 октября 2025» — в прошлом. */
internal fun fmtDayShort(ts: Long): String {
    val cal = java.util.Calendar.getInstance()
    val year = cal.get(java.util.Calendar.YEAR)
    cal.timeInMillis = ts * 1000
    val pattern = if (cal.get(java.util.Calendar.YEAR) == year) "d MMMM" else "d MMMM yyyy"
    return java.text.SimpleDateFormat(pattern, java.util.Locale.getDefault()).format(cal.time)
}

private fun kindColor(kind: String): Color = when (kind) {
    "image" -> Tokens.Colors.accent
    "video" -> Tokens.Colors.kindVideo
    "voice" -> Tokens.Colors.online
    else -> Tokens.Colors.kindFile
}

@Composable
private fun kindLabel(kind: String): String = stringResource(
    when (kind) {
        "image" -> R.string.storage_kind_image
        "video" -> R.string.storage_kind_video
        "voice" -> R.string.storage_kind_voice
        else -> R.string.storage_kind_file
    },
)
