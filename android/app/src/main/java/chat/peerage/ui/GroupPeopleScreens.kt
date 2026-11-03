package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Group
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.AppModel
import chat.peerage.Contact
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Люди в группе (app-ui.md → «Группы», макеты `people/GroupPick`, `GroupName`, `Invite`): мастер
 * новой группы — сначала участники, потом название — и «Пригласить» в готовую группу.
 * Приглашение — карточка «Вступить / Отклонить»: в группу человек входит, когда примет.
 */

/** Черновик новой группы между шагами мастера: выбранные люди и название. */
internal class GroupDraft {
    val picked = mutableStateListOf<String>()
    var name by mutableStateOf("")
}

/** «N выбрано» справа в шапке выбора; ничего не выбрано — пусто. */
@Composable
internal fun PickedCount(n: Int) {
    if (n > 0) {
        Text(
            pluralStringResource(R.plurals.people_picked, n, n),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            style = Tokens.Text.tabular,
            modifier = Modifier.padding(end = Tokens.Space.m),
        )
    }
}

/** Шаг 1 (макет `people/GroupPick`): выбранные — чипами сверху, поиск, контакты с отметками. */
@Composable
internal fun GroupPickScreen(model: AppModel, draft: GroupDraft, onNext: () -> Unit) {
    var query by remember { mutableStateOf("") }
    val shown = matching(model.contacts, query)
    Column(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(top = Tokens.Space.l)) {
            if (draft.picked.isNotEmpty()) {
                item(key = "chips") { PickedChips(model, draft.picked) }
            }
            item(key = "search") { ContactSearch(query) { query = it } }
            itemsIndexed(shown, key = { _, c -> c.key }) { i, c ->
                val on = c.key in draft.picked
                PersonRow(model, c, first = i == 0, last = i == shown.lastIndex, onClick = { toggle(draft.picked, c.key) }) {
                    SelectCheck(on)
                }
            }
        }
        ScreenFooter(stringResource(R.string.common_next), onClick = onNext)
    }
}

/** Шаг 2 (макет `people/GroupName`): название, кто войдёт и кто получит приглашение, права. */
@Composable
internal fun GroupNameScreen(model: AppModel, draft: GroupDraft, onCreated: () -> Unit) {
    val people = draft.picked.mapNotNull { k -> model.contacts.firstOrNull { it.key == k } }
    val me = model.me?.pubkey.orEmpty()
    Column(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(top = Tokens.Space.m)) {
            item(key = "name") {
                Row(
                    Modifier.fillMaxWidth().padding(start = Tokens.Space.panel, end = Tokens.Space.panel, bottom = Tokens.Space.xl),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.l),
                ) {
                    // С названием — та же буква, что встанет в списке чатов; без него —
                    // нейтральная плитка. Своё фото группы — шаг 3.5.
                    if (draft.name.isBlank()) {
                        SquareAvatar(" ", size = Tokens.Size.groupDraftAvatar, bg = Tokens.Colors.raised) {
                            Icon(Icons.Filled.Group, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconXl))
                        }
                    } else {
                        SquareAvatar(draft.name.trim(), size = Tokens.Size.groupDraftAvatar)
                    }
                    FormInput(draft.name, stringResource(R.string.group_name_hint), Modifier.weight(1f)) { draft.name = it }
                }
            }
            item(key = "sec") { SectionTitle(stringResource(R.string.group_members_count, people.size + 1)) }
            item(key = "me") {
                CardBlock(first = true, last = people.isEmpty()) {
                    PersonLine(
                        model,
                        me,
                        model.selfName.ifBlank { model.peerName(me) },
                        stringResource(R.string.group_creator_admin),
                        divider = false,
                        online = !model.offline,
                    ) { RowTag(stringResource(R.string.group_admin_mark), accent = true) }
                }
            }
            itemsIndexed(people, key = { _, c -> c.key }) { i, c ->
                CardBlock(first = false, last = i == people.lastIndex) {
                    PersonLine(model, c.key, c.displayName, stringResource(R.string.group_gets_invite), divider = true)
                }
            }
            item(key = "cap") { Caption(stringResource(R.string.group_create_caption)) }
        }
        // Без названия группа в списке и в приглашении была бы безымянной строкой.
        ScreenFooter(stringResource(R.string.create_btn), enabled = draft.name.isNotBlank()) {
            model.createGroup(draft.name.trim(), draft.picked.toList())
            onCreated()
        }
    }
}

/**
 * «Пригласить» в готовую группу (макет `people/Invite`): несколько сразу. Участники — «уже в
 * группе», позванные и не ответившие — «ждёт ответа»; их не выбрать.
 */
@Composable
internal fun GroupInviteScreen(model: AppModel, picked: MutableList<String>, onDone: () -> Unit) {
    val gid = model.currentGroup ?: return
    val members = model.groupMemberKeys[gid].orEmpty().toSet()
    var invited by remember(gid) { mutableStateOf(emptySet<String>()) }
    LaunchedEffect(gid) { model.groupInvited(gid) { invited = it } }
    var query by remember { mutableStateOf("") }
    // Порядок — как в контактах (макет): занятые стоят на своих местах серыми.
    val shown = matching(model.contacts, query)
    Column(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(top = Tokens.Space.l)) {
            item(key = "search") { ContactSearch(query) { query = it } }
            if (model.contacts.isEmpty()) {
                item(key = "empty") { Caption(stringResource(R.string.addmember_empty)) }
            }
            itemsIndexed(shown, key = { _, c -> c.key }) { i, c ->
                val first = i == 0
                val last = i == shown.lastIndex
                when (c.key) {
                    in members -> PersonRow(model, c, first, last, status = stringResource(R.string.group_in_group), dim = true)
                    in invited -> PersonRow(model, c, first, last, status = stringResource(R.string.group_invite_sent), dim = true) {
                        RowTag(stringResource(R.string.group_invite_waiting))
                    }
                    else -> PersonRow(model, c, first, last, onClick = { toggle(picked, c.key) }) { SelectCheck(c.key in picked) }
                }
            }
            item(key = "cap") { Caption(stringResource(R.string.group_invite_caption)) }
        }
        ScreenFooter(
            if (picked.isEmpty()) stringResource(R.string.group_invite_btn) else stringResource(R.string.group_invite_btn_n, picked.size),
            enabled = picked.isNotEmpty(),
        ) {
            picked.forEach { model.inviteToGroup(gid, it) }
            picked.clear()
            onDone()
        }
    }
}

private fun toggle(list: MutableList<String>, key: String) {
    if (!list.remove(key)) list.add(key)
}

/** Контакты по строке поиска: имя, своё имя контакта или начало ключа. */
private fun matching(all: List<Contact>, query: String): List<Contact> {
    val q = query.trim().lowercase()
    if (q.isEmpty()) return all
    return all.filter { it.displayName.lowercase().contains(q) || it.profileName.lowercase().contains(q) || it.key.startsWith(q) }
}

/** Выбранные чипами: аватар, имя, «×» снимает выбор. */
@Composable
private fun PickedChips(model: AppModel, keys: MutableList<String>) {
    LazyRow(
        contentPadding = PaddingValues(horizontal = Tokens.Space.panel),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s - Tokens.Space.xxs),
        modifier = Modifier.padding(bottom = Tokens.Space.m),
    ) {
        items(keys.toList(), key = { it }) { k ->
            val name = model.peerName(k)
            Row(
                Modifier.heightIn(min = Tokens.Size.pill)
                    .clip(RoundedCornerShape(Tokens.Radius.pill))
                    .background(Tokens.Colors.raised)
                    .clickable(onClickLabel = stringResource(R.string.common_remove)) { keys.remove(k) }
                    .padding(start = Tokens.Space.xs, end = Tokens.Space.s - Tokens.Space.xxs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s - Tokens.Space.xxs),
            ) {
                SquareAvatar(name, size = Tokens.Size.avatarChip, avatarBytes = model.peerAvatar(k), circle = true)
                Text(name, color = Color.White, fontSize = Tokens.Font.preview, maxLines = 1)
                Icon(TablerIcons.X, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconXs))
            }
        }
    }
}

/** Поиск по контактам (макет: плашка во всю ширину над группой). */
@Composable
private fun ContactSearch(query: String, onQuery: (String) -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.panel, end = Tokens.Space.panel, bottom = Tokens.Space.m)
            .heightIn(min = Tokens.Size.composer)
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(Tokens.Colors.surface)
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s + Tokens.Space.xxs),
    ) {
        Icon(TablerIcons.Search, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconRow))
        Box(Modifier.weight(1f)) {
            if (query.isEmpty()) Text(stringResource(R.string.contacts_search_hint), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
            BasicTextField(
                query,
                onQuery,
                singleLine = true,
                textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.label),
                cursorBrush = SolidColor(Tokens.Colors.accent),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/** Поле формы на панели (макет: `.inp`): плашка, крупный текст, подсказка на месте пустого. */
@Composable
private fun FormInput(value: String, hint: String, modifier: Modifier = Modifier, onValue: (String) -> Unit) {
    Box(
        modifier.heightIn(min = Tokens.Size.formField)
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(Tokens.Colors.surface)
            .padding(horizontal = Tokens.Space.l),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (value.isEmpty()) Text(hint, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name)
        BasicTextField(
            value,
            onValue,
            singleLine = true,
            textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.name),
            cursorBrush = SolidColor(Tokens.Colors.accent),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/** Строка контакта в группе-списке: статус по умолчанию — в сети или нет. */
@Composable
private fun LazyItemScope.PersonRow(
    model: AppModel,
    c: Contact,
    first: Boolean,
    last: Boolean,
    status: String? = null,
    dim: Boolean = false,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    CardBlock(first, last) {
        PersonLine(model, c.key, c.displayName, status, divider = !first, dim = dim, onClick = onClick, trailing = trailing)
    }
}

/** Человек строкой: аватар 40, имя, статус, справа — отметка или метка. */
@Composable
private fun PersonLine(
    model: AppModel,
    key: String,
    name: String,
    status: String?,
    divider: Boolean,
    dim: Boolean = false,
    /** Своё присутствие — не из списка пиров; null — смотреть по ключу. */
    online: Boolean? = null,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val online = online ?: (model.online[key] == true)
    CardRow(onClick = onClick, divider = divider, inset = PersonInset, modifier = Modifier.heightIn(min = Tokens.Size.personRow)) {
        SquareAvatar(name, size = Tokens.Size.avatarPick, online = online, avatarBytes = model.peerAvatar(key), circle = true)
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f)) {
            Text(
                name,
                color = if (dim) Tokens.Colors.disabledText else Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                status ?: stringResource(if (online) R.string.status_in_network else R.string.status_offline),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        trailing?.let {
            Spacer(Modifier.width(Tokens.Space.s))
            it()
        }
    }
}

/** Метка у правого края строки: «админ» — акцентом, состояние — серым. */
@Composable
private fun RowTag(text: String, accent: Boolean = false) {
    Text(
        text,
        color = if (accent) Tokens.Colors.accentSoft else Tokens.Colors.tertiary,
        fontSize = Tokens.Font.small,
        fontWeight = if (accent) FontWeight.Medium else FontWeight.Normal,
        maxLines = 1,
        modifier = Modifier.clip(RoundedCornerShape(Tokens.Radius.chip))
            .background(if (accent) Tokens.Colors.accentMuted else Tokens.Colors.raised)
            .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xxs),
    )
}

/** Пояснение под группой: последствие одной-двумя фразами. */
@Composable
private fun Caption(text: String) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        lineHeight = Tokens.Font.lineHeight,
        modifier = Modifier.padding(start = Tokens.Space.panel + Tokens.Space.s - Tokens.Space.xxs, end = Tokens.Space.panel, top = Tokens.Space.s),
    )
}

/** Главная кнопка экрана внизу, над системной полосой (макет: `foot`). */
@Composable
private fun ScreenFooter(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    Box(
        Modifier.fillMaxWidth().navigationBarsPadding()
            .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m, bottom = Tokens.Space.xxl),
    ) {
        AccentButton(text, Modifier.fillMaxWidth().heightIn(min = Tokens.Size.primaryButton), enabled = enabled, onClick = onClick)
    }
}

/** Отступ разделителя у строки человека: поле строки + аватар + зазор. */
private val PersonInset = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m + Tokens.Space.xxs
