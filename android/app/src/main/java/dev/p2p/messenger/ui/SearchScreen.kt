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
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.AppModel
import dev.p2p.messenger.CenterMessage
import dev.p2p.messenger.HoloOnline
import dev.p2p.messenger.HoloSecondary
import dev.p2p.messenger.ScreenPad

/** Глобальный поиск: фильтр контактов и групп по имени/ключу. Сообщения — позже
 *  (история per-peer; полнотекст по чатам подключим, когда история уйдёт в движок). */
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
        model.contacts.filter { it.name.lowercase().contains(q) || it.key.lowercase().contains(q) }
    }
    val groups = if (q.isEmpty()) emptyList() else model.groups.filter { it.name.lowercase().contains(q) }

    Column(Modifier.fillMaxSize()) {
        TextField(
            query,
            { query = it },
            Modifier.fillMaxWidth().padding(ScreenPad),
            placeholder = { Text("Имя, ключ или название группы", color = HoloSecondary) },
            colors = inputColors(),
            singleLine = true,
        )
        when {
            q.isEmpty() -> CenterMessage("Начните вводить — найдём контакты и группы")
            contacts.isEmpty() && groups.isEmpty() -> CenterMessage("Ничего не найдено")
            else -> LazyColumn(Modifier.fillMaxSize()) {
                items(groups, key = { "g_${it.gid}" }) { g ->
                    ListRow(onClick = { onOpenGroup(g.gid) }) {
                        SquareAvatar(g.name, size = 44.dp, bg = Color(0xFF0D2A0D)) {
                            Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
                        }
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(g.name, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text("${g.members} уч. · группа", color = HoloSecondary, fontSize = 11.sp)
                        }
                    }
                }
                items(contacts, key = { "c_${it.key}" }) { c ->
                    val online = model.online[c.key] == true
                    ListRow(onClick = { onOpenDm(c.key) }) {
                        SquareAvatar(c.name, size = 44.dp, online = online)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(c.name, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                if (online) "онлайн" else "не в сети",
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
