package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/**
 * Папки (макеты `misc/Folders`, `misc/FolderEdit`).
 *
 * Папка — ВИД, а не коробка: чат попадает сразу в несколько, а удаление папки не
 * трогает переписку. Правила комбинируются неочевидно («типы + конкретные чаты −
 * исключения»), поэтому у редактора живой счётчик в шапке: результат виден, пока правишь.
 */

/** Значки своей папки (`misc/FolderEdit`). Ключ хранится в папке; незнакомый — «папка». */
internal val FolderIcons: List<Pair<String, ImageVector>> = listOf(
    "briefcase" to TablerIcons.Briefcase,
    "home" to TablerIcons.Home,
    "folder" to TablerIcons.Folder,
    "headphones" to TablerIcons.Headphones,
    "speakerphone" to TablerIcons.Speakerphone,
    "message" to TablerIcons.Message,
)

internal fun folderIcon(key: String): ImageVector =
    FolderIcons.firstOrNull { it.first == key }?.second ?: TablerIcons.Folder

/** Список папок: встроенные — для сведения; свои — с переходом в редактор и «Создать папку». */
@Composable
internal fun FoldersScreen(model: AppModel, onEdit: (String?) -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = Tokens.Space.l, bottom = Tokens.Space.xxl)) {
        Text(
            stringResource(R.string.folders_hint),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            lineHeight = Tokens.Font.lineHeight,
            modifier = Modifier.padding(horizontal = Tokens.Space.panelText),
        )

        SectionTitle(stringResource(R.string.folders_builtin))
        CardSection {
            Folders.forEachIndexed { i, (_, icon, labelRes) ->
                FolderRow(icon, stringResource(labelRes), divider = i > 0)
            }
        }

        SectionTitle(stringResource(R.string.folders_mine))
        CardSection {
            model.folders.forEachIndexed { i, f ->
                val n = model.folderCount(f)
                FolderRow(
                    folderIcon(f.icon),
                    f.name,
                    divider = i > 0,
                    sub = pluralStringResource(R.plurals.folder_chats, n, n),
                    chevron = true,
                ) { onEdit(f.id) }
            }
            FolderRow(
                TablerIcons.Plus,
                stringResource(R.string.folder_create),
                divider = model.folders.isNotEmpty(),
                accent = true,
            ) { onEdit(null) }
        }
    }
}

/** Строка группы папок (`.gr`): значок, название и подпись, шеврон у своей. */
@Composable
private fun FolderRow(
    icon: ImageVector,
    label: String,
    divider: Boolean,
    sub: String? = null,
    chevron: Boolean = false,
    accent: Boolean = false,
    onClick: (() -> Unit)? = null,
) {
    CardRow(onClick = onClick, divider = divider, inset = 0.dp) {
        Icon(icon, null, tint = if (accent) Tokens.Colors.accent else Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xxs)) {
            Text(
                label,
                color = if (accent) Tokens.Colors.accent else Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
            )
            if (sub != null) {
                Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, modifier = Modifier.padding(top = Tokens.Space.xxs))
            }
        }
        if (chevron) Icon(TablerIcons.ChevronRight, null, tint = Tokens.Colors.chevron, modifier = Modifier.size(Tokens.Size.iconS))
    }
}

/** Шапка редактора: «Папка» и живой счётчик «7 чатов». */
@Composable
internal fun FolderEditTopBar(model: AppModel, onBack: () -> Unit) {
    TopBar(stringResource(R.string.folder_edit_title), onBack = onBack) {
        model.folderDraft?.let { d ->
            val n = model.folderCount(d)
            Text(
                pluralStringResource(R.plurals.folder_chats, n, n),
                color = Tokens.Colors.accentSoft,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(end = Tokens.Space.m),
            )
        }
    }
}

/**
 * Редактор папки. [folderId] == null — новая. Правка идёт в черновик модели и пишется при
 * выходе ([AppModel.commitFolderDraft]); без названия новая папка не появляется.
 */
@Composable
internal fun FolderEditorScreen(model: AppModel, folderId: String?, onDone: () -> Unit) {
    DisposableEffect(folderId) {
        model.folderDraft = folderId?.let { model.folderById(it) }
            ?: Folder("f${System.currentTimeMillis()}", "", FolderIcons.first().first, types = setOf("dm"))
        onDispose { model.commitFolderDraft() }
    }
    val d = model.folderDraft ?: return
    val edit = { f: Folder -> model.folderDraft = f }
    var confirmDelete by remember { mutableStateOf(false) }
    val existing = folderId != null

    Column(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(top = Tokens.Space.l)) {
            NameField(d.name) { edit(d.copy(name = it)) }
            Row(
                Modifier.fillMaxWidth().padding(
                    start = Tokens.Space.panel,
                    end = Tokens.Space.panel,
                    top = Tokens.Space.s + Tokens.Space.xxs,
                ),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
            ) {
                FolderIcons.forEach { (key, iv) -> IconTile(iv, key == d.icon) { edit(d.copy(icon = key)) } }
            }

            SectionTitle(stringResource(R.string.folder_include))
            Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panel),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
            ) {
                TypeChip(stringResource(R.string.folder_type_dm), "dm" in d.types) { edit(d.copy(types = d.types.toggle("dm"))) }
                TypeChip(stringResource(R.string.folder_type_room), "room" in d.types) { edit(d.copy(types = d.types.toggle("room"))) }
            }

            val picks = pickable(model)
            if (picks.isNotEmpty()) {
                SectionTitle(stringResource(R.string.folder_pick_chats))
                CardSection {
                    picks.forEachIndexed { i, t ->
                        PickRow(model, t, t.id in d.chats, divider = i > 0) { edit(d.copy(chats = d.chats.toggle(t.id))) }
                    }
                }
            }

            SectionTitle(stringResource(R.string.folder_exclude))
            CardSection {
                CardRow(
                    divider = false,
                    modifier = Modifier.switchRow(d.excludeRead) { edit(d.copy(excludeRead = it)) },
                ) {
                    Icon(TablerIcons.Check, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
                    Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
                    Text(
                        stringResource(R.string.folder_exclude_read),
                        color = Color.White,
                        fontSize = Tokens.Font.label,
                        fontWeight = FontWeight.Medium,
                        modifier = Modifier.weight(1f),
                    )
                    Switch(checked = d.excludeRead, onCheckedChange = null)
                }
            }
            Spacer(Modifier.height(Tokens.Space.l))
        }
        if (existing) {
            GhostButton(
                stringResource(R.string.folder_delete),
                color = Tokens.Colors.iconDangerMuted,
                modifier = Modifier.fillMaxWidth().padding(
                    start = Tokens.Space.xl,
                    end = Tokens.Space.xl,
                    top = Tokens.Space.m,
                    bottom = Tokens.Space.xxl,
                ),
            ) { confirmDelete = true }
        }
    }

    if (confirmDelete) {
        // Папка выглядит контейнером; без этой строки удаление читается как удаление переписок.
        ConfirmSheet(
            title = stringResource(R.string.folder_delete),
            text = stringResource(R.string.folder_delete_confirm),
            ok = stringResource(R.string.folder_delete_ok),
            onOk = {
                folderId?.let { model.deleteFolder(it) }
                model.folderDraft = null // выход не должен записать удалённую обратно
                onDone()
            },
            onClose = { confirmDelete = false },
        )
    }
}

/** Чаты для «И конкретные чаты»: сначала те, где писали недавно. «Сохранённое» — не чат для папки. */
private fun pickable(model: AppModel): List<ChatTarget> {
    val all = model.contacts.map { ChatTarget(it.key, group = false) } + model.groups.map { ChatTarget(it.gid, group = true) }
    return all.sortedByDescending { model.chatSummaries[model.chatKey(it.id, it.group)]?.lastTime ?: 0L }
}

private fun Set<String>.toggle(v: String): Set<String> = if (v in this) this - v else this + v

@Composable
private fun NameField(value: String, onChange: (String) -> Unit) {
    Box(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panel).heightIn(min = Tokens.Size.formField)
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(Tokens.Colors.group)
            .padding(horizontal = Tokens.Space.l),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (value.isEmpty()) Text(stringResource(R.string.folder_name_hint), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name)
        BasicTextField(
            value,
            onChange,
            singleLine = true,
            textStyle = Tokens.Text.base.copy(color = Color.White, fontSize = Tokens.Font.name),
            cursorBrush = SolidColor(Tokens.Colors.accent),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/** Плитка значка: выбранная — приглушённым акцентом с обводкой. */
@Composable
private fun IconTile(icon: ImageVector, on: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(Tokens.Radius.button)
    Box(
        Modifier.size(Tokens.Size.folderIcon).clip(shape)
            .background(if (on) Tokens.Colors.accentMuted else Tokens.Colors.raised)
            .then(if (on) Modifier.border(Tokens.Size.checkStroke, Tokens.Colors.accent, shape) else Modifier)
            .clickable(role = Role.RadioButton) { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = if (on) Tokens.Colors.accentSoft else Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
    }
}

/** Чип «Личные» / «Группы» (`.tb`). */
@Composable
private fun TypeChip(label: String, on: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.heightIn(min = Tokens.Size.typeChip).clip(RoundedCornerShape(Tokens.Radius.pill))
            .background(if (on) Tokens.Colors.accentMuted else Tokens.Colors.raised)
            .clickable(role = Role.Checkbox) { onClick() }
            .padding(horizontal = Tokens.Space.m),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (on) Tokens.Colors.accentSoft else Tokens.Colors.chipText, fontSize = Tokens.Font.option)
    }
}

/** Чат в «И конкретные чаты»: аватар, имя и кружок выбора. */
@Composable
private fun PickRow(model: AppModel, t: ChatTarget, checked: Boolean, divider: Boolean, onToggle: () -> Unit) {
    val (name, avatar) = chatFace(model, t)
    CardRow(onClick = onToggle, divider = divider, inset = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m + Tokens.Space.xxs) {
        SquareAvatar(
            name,
            size = Tokens.Size.avatarPick,
            avatarBytes = avatar,
            circle = !t.group,
            bg = if (t.group) Tokens.Colors.groupAvatar else null,
        )
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Text(
            name,
            color = Color.White,
            fontSize = Tokens.Font.label,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            modifier = Modifier.weight(1f).heightIn(min = Tokens.Size.personRow - Tokens.Space.l).wrapContentHeight(),
        )
        SelectCheck(checked)
    }
}
