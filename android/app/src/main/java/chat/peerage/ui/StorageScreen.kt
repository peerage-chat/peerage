package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
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
    // Шторка очистки: null — закрыта; открытая с чата выбирает только его.
    var clearing by remember { mutableStateOf<ClearOpen?>(null) }
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
                    StorageChatRow(model, sc, divider = i > 0) { clearing = ClearOpen(ChatTarget(sc.id, sc.group)) }
                }
            }
        }

        val canClear = model.storageBlobs > 0
        Spacer(Modifier.height(Tokens.Space.m + Tokens.Space.xxs))
        CardSection {
            CardRow(onClick = if (canClear) ({ clearing = ClearOpen(null) }) else null, divider = false) {
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

    clearing?.let { open -> ClearSheet(model, open.only) { clearing = null } }
}

/** Как открыта шторка очистки: [only] — с чата, иначе всё. */
private class ClearOpen(val only: ChatTarget?)

/**
 * Шторка очистки вложений (п. 27): типы — чипами, чаты — кружками выбора, на кнопке — сколько
 * освободится (считает движок тем же фильтром, что потом удаляет). Всё выбрано — очистить всё,
 * в том числе байты, на которые история уже не ссылается.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ClearSheet(model: AppModel, only: ChatTarget?, onClose: () -> Unit) {
    val kinds = remember { model.storageByKind }
    val chats = remember { model.storageChats }
    val allChats = remember { chats.map { ChatTarget(it.id, it.group) }.toSet() }
    var pickedKinds by remember { mutableStateOf(kinds.map { it.first }.toSet()) }
    var pickedChats by remember { mutableStateOf(only?.let(::setOf) ?: allChats) }
    val empty = (kinds.isNotEmpty() && pickedKinds.isEmpty()) || (chats.isNotEmpty() && pickedChats.isEmpty())
    val scope = ClearScope(
        chats = if (pickedChats == allChats) emptySet() else pickedChats,
        kinds = if (pickedKinds.size == kinds.size) emptySet() else pickedKinds,
    )
    var freed by remember { mutableStateOf<Long?>(null) }
    val latest by rememberUpdatedState(scope)
    LaunchedEffect(scope, empty) {
        freed = null
        if (empty) {
            freed = 0
        } else {
            // Ответ на старый выбор не перетирает новый.
            model.previewClear(scope) { bytes -> if (latest == scope) freed = bytes }
        }
    }

    SheetFrame(onClose) {
        Text(
            stringResource(R.string.storage_clear_title),
            color = Color.White,
            fontSize = Tokens.Font.sheet,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs).semantics { heading() },
        )
        Text(
            stringResource(R.string.storage_clear_sub),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.label,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
        if (kinds.size > 1) {
            SheetCaption(stringResource(R.string.storage_clear_kinds))
            FlowRow(
                Modifier.padding(horizontal = Tokens.Space.xs),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
            ) {
                kinds.forEach { (kind, bytes) ->
                    TypeChip("${kindLabel(kind)} · ${fmtSizeLong(bytes)}", kind in pickedKinds) {
                        pickedKinds = if (kind in pickedKinds) pickedKinds - kind else pickedKinds + kind
                    }
                }
            }
        }
        if (chats.size > 1) {
            SheetCaption(stringResource(R.string.storage_clear_chats))
            Column(Modifier.heightIn(max = Tokens.Size.sheetList).verticalScroll(rememberScrollState())) {
                ClearPickRow(on = pickedChats == allChats, onToggle = {
                    pickedChats = if (pickedChats == allChats) emptySet() else allChats
                }) {
                    Text(
                        stringResource(R.string.storage_clear_all_chats),
                        color = Color.White,
                        fontSize = Tokens.Font.label,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier.weight(1f),
                    )
                }
                chats.forEach { sc ->
                    val t = ChatTarget(sc.id, sc.group)
                    val (name, avatar) = chatFace(model, t)
                    ClearPickRow(on = t in pickedChats, onToggle = {
                        pickedChats = if (t in pickedChats) pickedChats - t else pickedChats + t
                    }) {
                        SquareAvatar(
                            name,
                            size = Tokens.Size.rowIcon,
                            avatarBytes = avatar,
                            circle = !sc.group,
                            bg = if (sc.group) Tokens.Colors.groupAvatar else null,
                        )
                        Spacer(Modifier.width(Tokens.Space.m))
                        Text(name, color = Color.White, fontSize = Tokens.Font.label, maxLines = 1, modifier = Modifier.weight(1f))
                        Text(
                            fmtSizeLong(sc.bytes),
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.option,
                            modifier = Modifier.padding(horizontal = Tokens.Space.s),
                        )
                    }
                }
            }
        }
        val bytes = freed
        PlateButton(
            when {
                bytes == 0L -> stringResource(R.string.storage_clear_nothing)
                bytes == null -> stringResource(R.string.storage_clear_ok)
                else -> stringResource(R.string.storage_clear_ok_size, fmtSizeLong(bytes))
            },
            modifier = Modifier.fillMaxWidth(),
            danger = true,
            enabled = bytes != null && bytes > 0,
        ) {
            model.clearAttachments(scope)
            onClose()
        }
        PlateButton(stringResource(R.string.common_cancel), modifier = Modifier.fillMaxWidth(), onClick = onClose)
    }
}

/** Подпись группы в шторке: «Типы», «Чаты». */
@Composable
private fun SheetCaption(text: String) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(horizontal = Tokens.Space.xs),
    )
}

/** Строка выбора в шторке: содержимое и кружок выбора у заднего края; весь ряд — переключатель. */
@Composable
private fun ClearPickRow(on: Boolean, onToggle: () -> Unit, content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.touch)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .toggleable(on, role = Role.Checkbox) { onToggle() }
            .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        content()
        SelectCheck(on)
    }
}

/** Строка «По чатам»: аватар чата (группа — квадратом), имя, сколько весят вложения; нажатие —
 *  очистить вложения этого чата. */
@Composable
private fun StorageChatRow(model: AppModel, sc: StorageChat, divider: Boolean, onClick: () -> Unit) {
    val (name, avatar) = chatFace(model, ChatTarget(sc.id, sc.group))
    CardRow(onClick = onClick, divider = divider, inset = Tokens.Space.l + Tokens.Size.rowIcon + Tokens.Space.m) {
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
