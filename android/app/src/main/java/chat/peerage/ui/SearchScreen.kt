@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Message
import androidx.compose.material.icons.filled.Group
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.*
import chat.peerage.AppModel
import chat.peerage.CenterMessage
import chat.peerage.HoloOnline
import chat.peerage.HoloSecondary
import chat.peerage.R
import chat.peerage.ScreenPad

/**
 * Глобальный поиск: контакты и группы по имени/ключу — и САМИ СООБЩЕНИЯ.
 *
 * Поиск по переписке делает движок (полнотекстовый индекс в общей базе): клиент не
 * держит историю целиком и искать в ней не может. Находка помнит свой чат, поэтому
 * по нажатию открывается нужный чат — личный или групповой.
 */
@Composable
internal fun SearchScreen(
    model: AppModel,
    onOpenDm: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    val q = query.trim().lowercase()
    val contacts = if (q.isEmpty()) {
        emptyList()
    } else {
        model.contacts.filter {
            it.displayName.lowercase().contains(q) || it.profileName.lowercase().contains(q) || it.key.lowercase().contains(q)
        }
    }
    val groups = if (q.isEmpty()) emptyList() else model.groups.filter { it.name.lowercase().contains(q) }
    // Запрос уходит в движок на каждое изменение строки: поиск идёт по индексу и
    // возвращает не больше сотни строк, отдельная кнопка «искать» тут лишняя.
    LaunchedEffect(q) { model.searchMessages(q) }
    val hits = if (q.isEmpty()) emptyList() else model.searchHits

    Column(Modifier.fillMaxSize()) {
        TextField(
            query,
            { query = it },
            Modifier.fillMaxWidth().padding(ScreenPad),
            placeholder = { Text(stringResource(R.string.search_hint), color = HoloSecondary) },
            colors = inputColors(),
            singleLine = true,
        )
        when {
            q.isEmpty() -> CenterMessage(stringResource(R.string.search_prompt))
            contacts.isEmpty() && groups.isEmpty() && hits.isEmpty() ->
                CenterMessage(stringResource(R.string.search_empty))
            else -> LazyColumn(Modifier.fillMaxSize()) {
                items(groups, key = { "g_${it.gid}" }) { g ->
                    ListRow(onClick = { onOpenGroup(g.gid) }) {
                        SquareAvatar(g.name, size = Tokens.Size.avatarRow, bg = Tokens.Colors.groupAvatar, avatarBytes = model.groupPhoto(g.gid)) {
                            Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(Tokens.Size.iconM))
                        }
                        Spacer(Modifier.width(Tokens.Space.m))
                        Column(Modifier.weight(1f)) {
                            Text(g.name, color = Color.White, fontSize = Tokens.Font.label, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(pluralOf(R.plurals.search_group_subtitle, g.members.toLong()), color = HoloSecondary, fontSize = Tokens.Font.caption)
                        }
                    }
                }
                items(hits, key = { "m_${it.chat}_${it.sender}_${it.id}" }) { hit ->
                    val title = if (hit.group) {
                        model.groups.firstOrNull { it.gid == hit.chat }?.name.orEmpty()
                    } else if (hit.chat == model.me?.pubkey) {
                        stringResource(R.string.menu_saved)
                    } else {
                        model.contacts.firstOrNull { it.key == hit.chat }?.displayName
                            ?: hit.chat.take(8)
                    }
                    // Находка ведёт к САМОМУ сообщению, а не в конец чата: иначе поиск
                    // говорил только «где-то тут было».
                    ListRow(onClick = {
                        if (hit.group) onOpenGroup(hit.chat) else onOpenDm(hit.chat)
                        // После открытия: открытие сбрасывает цель и читает хвост, а окно
                        // вокруг находки встаёт следом в той же очереди.
                        model.showAt(ChatTarget(hit.chat, hit.group), hit.time, hit.row, MsgRef(hit.id, hit.sender))
                    }) {
                        Icon(
                            Icons.AutoMirrored.Filled.Message,
                            null,
                            tint = HoloSecondary,
                            modifier = Modifier.size(Tokens.Size.iconM),
                        )
                        Spacer(Modifier.width(Tokens.Space.m))
                        Column(Modifier.weight(1f)) {
                            Text(
                                hit.text,
                                color = Color.White,
                                fontSize = Tokens.Font.label,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(title, color = HoloSecondary, fontSize = Tokens.Font.caption, maxLines = 1)
                        }
                    }
                }
                items(contacts, key = { "c_${it.key}" }) { c ->
                    val online = model.online[c.key] == true
                    ListRow(onClick = { onOpenDm(c.key) }) {
                        SquareAvatar(
                            c.displayName,
                            size = Tokens.Size.avatarRow,
                            online = online,
                            avatarBytes = model.contactAvatars[c.avatarId],
                            circle = true, // человек — круг, как в списке чатов
                            ring = HoloBg,
                        )
                        Spacer(Modifier.width(Tokens.Space.m))
                        Column(Modifier.weight(1f)) {
                            Text(c.displayName, color = Color.White, fontSize = Tokens.Font.label, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                stringResource(if (online) R.string.status_online else R.string.status_offline),
                                color = if (online) HoloOnline else HoloSecondary,
                                fontSize = Tokens.Font.caption,
                            )
                        }
                    }
                }
            }
        }
    }
}
