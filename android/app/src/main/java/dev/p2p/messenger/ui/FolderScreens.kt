package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.MarkEmailRead
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/**
 * Папки (макет `docs/design/mockups/folder-editor.html`).
 *
 * Папка — ВИД, а не коробка: чат попадает сразу в несколько, а удаление папки не
 * трогает переписку. Правила комбинируются неочевидно («типы + конкретные чаты −
 * исключения»), поэтому в редакторе обязателен живой счётчик: иначе результат виден
 * только после выхода.
 */

/** Значки, доступные пользователю для своей папки (из курируемого набора Tabler). */
internal val FolderIcons: List<Pair<String, ImageVector>> = listOf(
    "messages" to TablerIcons.Messages,
    "message" to TablerIcons.Message,
    "headphones" to TablerIcons.Headphones,
    "speakerphone" to TablerIcons.Speakerphone,
    "users" to TablerIcons.Users,
    "bell" to TablerIcons.Bell,
    "lock" to TablerIcons.ShieldLock,
    "user" to TablerIcons.User,
)

internal fun folderIcon(key: String): ImageVector =
    FolderIcons.firstOrNull { it.first == key }?.second ?: TablerIcons.Messages

/** Список папок: встроенные (только для сведения) + свои с переходом в редактор. */
@Composable
internal fun FoldersScreen(model: AppModel, onEdit: (String?) -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Text(
            stringResource(R.string.folders_hint),
            color = HoloSecondary,
            fontSize = 13.sp,
            modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.m),
        )

        ScreenSectionLabel(stringResource(R.string.folders_builtin))
        CardSection {
            Folders.forEachIndexed { i, (key, icon, labelRes) ->
                val count = when (key) {
                    "all" -> model.contacts.size + model.groups.size
                    "dm" -> model.contacts.size
                    "room" -> model.groups.size
                    else -> 0
                }
                CardRow(divider = i > 0) {
                    Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
                    Spacer(Modifier.width(Tokens.Space.m))
                    Text(
                        stringResource(labelRes),
                        color = Color.White,
                        fontSize = 15.sp,
                        modifier = Modifier.weight(1f),
                    )
                    Text("$count", color = Tokens.Colors.secondary, fontSize = 13.sp)
                }
            }
        }

        ScreenSectionLabel(stringResource(R.string.folders_mine))
        CardSection {
            model.folders.forEachIndexed { i, f ->
                CardRow(onClick = { onEdit(f.id) }, divider = i > 0, inset = 62.dp) {
                    Box(
                        Modifier.size(38.dp).clip(RoundedCornerShape(Tokens.Radius.button))
                            .background(FolderSelected),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(folderIcon(f.icon), null, tint = HoloAccent, modifier = Modifier.size(19.dp))
                    }
                    Spacer(Modifier.width(Tokens.Space.m))
                    Text(f.name, color = Color.White, fontSize = 15.sp, modifier = Modifier.weight(1f))
                    Icon(
                        TablerIcons.ChevronRight,
                        null,
                        tint = Tokens.Colors.tertiary,
                        modifier = Modifier.size(Tokens.Size.iconS),
                    )
                }
            }
        }

        // «Создать папку» — своей карточкой под списком: это действие, а не ещё
        // одна папка, и попасть в него по ошибке при промахе по списку не должно.
        Spacer(Modifier.height(Tokens.Space.s))
        CardSection {
            CardRow(onClick = { onEdit(null) }, divider = false) {
                Icon(TablerIcons.Plus, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconM))
                Spacer(Modifier.width(Tokens.Space.m))
                Text(stringResource(R.string.folder_create), color = HoloAccent, fontSize = 15.sp)
            }
        }
        Spacer(Modifier.height(Tokens.Space.l))
    }
}

/** Редактор папки. [folderId] == null — создание новой. */
@Composable
internal fun FolderEditorScreen(model: AppModel, folderId: String?, onDone: () -> Unit) {
    val existing = folderId?.let { model.folderById(it) }
    var name by remember(folderId) { mutableStateOf(existing?.name ?: "") }
    var icon by remember(folderId) { mutableStateOf(existing?.icon ?: FolderIcons.first().first) }
    var types by remember(folderId) { mutableStateOf(existing?.types ?: setOf("dm")) }
    var chats by remember(folderId) { mutableStateOf(existing?.chats ?: emptySet()) }
    var excludeRead by remember(folderId) { mutableStateOf(existing?.excludeRead ?: false) }
    var confirmDelete by remember { mutableStateOf(false) }

    // Живой счётчик: без него правила «типы + чаты − исключения» проверяются только
    // выходом из редактора.
    val preview = Folder("preview", name, icon, types, chats, excludeRead)
    val hits = model.contacts.count { model.folderHasDm(preview, it.key) } +
        model.groups.count { model.folderHasGroup(preview, it.gid) }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        ScreenSectionLabel(stringResource(R.string.folder_name_and_icon))
        Row(
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(48.dp).clip(RoundedCornerShape(Tokens.Radius.button)).background(FolderSelected),
                contentAlignment = Alignment.Center,
            ) {
                Icon(folderIcon(icon), null, tint = HoloAccent, modifier = Modifier.size(24.dp))
            }
            Spacer(Modifier.width(Tokens.Space.m))
            Box(
                Modifier.weight(1f).height(48.dp).clip(RoundedCornerShape(Tokens.Radius.tile))
                    .background(HoloSurface).padding(horizontal = Tokens.Space.l),
                contentAlignment = Alignment.CenterStart,
            ) {
                BasicTextField(
                    value = name,
                    onValueChange = { name = it },
                    textStyle = TextStyle(color = Color.White, fontSize = 15.sp),
                    cursorBrush = SolidColor(HoloAccent),
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                    decorationBox = { inner ->
                        if (name.isEmpty()) {
                            Text(stringResource(R.string.folder_name_hint), color = HoloSecondary, fontSize = 15.sp)
                        }
                        inner()
                    },
                )
            }
        }
        // Выбор значка — сразу рядом, без отдельного экрана.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad, vertical = Tokens.Space.m),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            FolderIcons.forEach { (key, iv) ->
                val on = key == icon
                Box(
                    Modifier.weight(1f).height(40.dp).clip(RoundedCornerShape(Tokens.Radius.chip))
                        .background(if (on) FolderSelected else HoloSurface)
                        .clickable { icon = key },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(iv, null, tint = if (on) HoloAccent else HoloSecondary, modifier = Modifier.size(19.dp))
                }
            }
        }

        ScreenSectionLabel(stringResource(R.string.folder_include))
        Row(
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            TypeChip(TablerIcons.Message, R.string.folder_type_dm, "dm" in types) {
                types = if ("dm" in types) types - "dm" else types + "dm"
            }
            TypeChip(TablerIcons.Headphones, R.string.folder_type_room, "room" in types) {
                types = if ("room" in types) types - "room" else types + "room"
            }
        }

        Text(
            stringResource(R.string.folder_pick_chats),
            color = HoloSecondary,
            fontSize = 12.sp,
            modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.s),
        )
        model.groups.forEach { g ->
            PickRow(g.name, g.gid in chats, null) {
                chats = if (g.gid in chats) chats - g.gid else chats + g.gid
            }
        }
        model.contacts.forEach { c ->
            PickRow(c.displayName, c.key in chats, model.contactAvatars[c.avatarId]) {
                chats = if (c.key in chats) chats - c.key else chats + c.key
            }
        }

        ScreenSectionLabel(stringResource(R.string.folder_exclude))
        Row(Modifier.fillMaxWidth().padding(horizontal = ScreenPad)) {
            TypeChip(Icons.Filled.MarkEmailRead, R.string.folder_exclude_read, excludeRead) {
                excludeRead = !excludeRead
            }
        }

        // Итог правил — одной строкой.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad, vertical = Tokens.Space.m)
                .clip(RoundedCornerShape(Tokens.Radius.tile)).background(HoloSurface)
                .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.folder_counter, hits),
                color = HoloSecondary,
                fontSize = 13.sp,
            )
        }

        AccentButton(
            stringResource(R.string.common_save),
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad),
        ) {
            if (name.isNotBlank()) {
                model.saveFolder(
                    Folder(
                        id = folderId ?: "f${System.currentTimeMillis()}",
                        name = name.trim(),
                        icon = icon,
                        types = types,
                        chats = chats,
                        excludeRead = excludeRead,
                    ),
                )
                onDone()
            }
        }

        if (existing != null) {
            Row(
                Modifier.fillMaxWidth().clickable { confirmDelete = true }
                    .padding(horizontal = ScreenPad, vertical = RowPadV),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.Filled.Delete, null, tint = HoloError, modifier = Modifier.size(21.dp))
                Spacer(Modifier.width(Tokens.Space.m))
                Text(stringResource(R.string.folder_delete), color = HoloError, fontSize = 15.sp)
            }
        }
        Spacer(Modifier.height(Tokens.Space.l))
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(R.string.folder_delete)) },
            // Папка выглядит контейнером; без этой строки удаление читается как
            // удаление переписок.
            text = { Text(stringResource(R.string.folder_delete_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    folderId?.let { model.deleteFolder(it) }
                    confirmDelete = false
                    onDone()
                }) { Text(stringResource(R.string.folder_delete_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirmDelete = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}

@Composable
private fun RowScope.TypeChip(icon: ImageVector, labelRes: Int, on: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.pill))
            .background(if (on) Tokens.Colors.accentBadge else HoloSurface)
            .clickable { onClick() }
            .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = if (on) HoloAccent else HoloSecondary, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(6.dp))
        Text(
            stringResource(labelRes),
            color = if (on) HoloAccent else HoloSecondary,
            fontSize = 13.sp,
        )
    }
}

@Composable
private fun PickRow(name: String, checked: Boolean, avatar: ByteArray?, onToggle: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onToggle() }
            .padding(horizontal = ScreenPad, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SquareAvatar(name, size = 38.dp, avatarBytes = avatar)
        Spacer(Modifier.width(Tokens.Space.m))
        Text(name, color = Color.White, fontSize = 15.sp, modifier = Modifier.weight(1f))
        Box(
            Modifier.size(22.dp).clip(RoundedCornerShape(percent = 50))
                .background(if (checked) HoloAccent else Color.Transparent),
            contentAlignment = Alignment.Center,
        ) {
            if (checked) {
                Icon(TablerIcons.Check, null, tint = HoloAccentDark, modifier = Modifier.size(14.dp))
            } else {
                Box(
                    Modifier.size(22.dp).clip(RoundedCornerShape(percent = 50))
                        .background(HoloSurface),
                )
            }
        }
    }
}
