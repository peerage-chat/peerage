@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

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
import androidx.compose.material.icons.filled.Headset
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.AppModel
import dev.p2p.messenger.CenterMessage
import dev.p2p.messenger.HoloOnline
import dev.p2p.messenger.HoloSecondary
import dev.p2p.messenger.R
import dev.p2p.messenger.ScreenPad

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
            it.displayName.lowercase().contains(q) || it.name.lowercase().contains(q) || it.key.lowercase().contains(q)
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
                        SquareAvatar(g.name, size = 44.dp, bg = Tokens.Colors.groupAvatar) {
                            Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
                        }
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(g.name, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(stringResource(R.string.search_group_subtitle, g.members), color = HoloSecondary, fontSize = 11.sp)
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
                    ListRow(onClick = { if (hit.group) onOpenGroup(hit.chat) else onOpenDm(hit.chat) }) {
                        Icon(
                            Icons.AutoMirrored.Filled.Message,
                            null,
                            tint = HoloSecondary,
                            modifier = Modifier.size(22.dp),
                        )
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(
                                hit.text,
                                color = Color.White,
                                fontSize = 14.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(title, color = HoloSecondary, fontSize = 11.sp, maxLines = 1)
                        }
                    }
                }
                items(contacts, key = { "c_${it.key}" }) { c ->
                    val online = model.online[c.key] == true
                    ListRow(onClick = { onOpenDm(c.key) }) {
                        SquareAvatar(c.displayName, size = 44.dp, online = online, avatarBytes = model.contactAvatars[c.avatarId])
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(c.displayName, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                stringResource(if (online) R.string.status_online else R.string.status_offline),
                                color = if (online) HoloOnline else HoloSecondary,
                                fontSize = 11.sp,
                            )
                        }
                    }
                }
            }
        }
    }
}
