@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.AppModel
import dev.p2p.messenger.ChatTarget
import dev.p2p.messenger.HoloAccent
import dev.p2p.messenger.HoloBg
import dev.p2p.messenger.HoloSecondary
import dev.p2p.messenger.R
import dev.p2p.messenger.Tokens

/**
 * Куда переслать: контакты, группы и «Сохранённое» одним списком.
 *
 * Показывается, когда в модели уже отложены сообщения ([AppModel.forwarding]).
 * Архивные группы в список не попадают: писать в них нельзя, и предлагать их —
 * значит обещать отправку, которой не будет.
 */
@Composable
internal fun ForwardSheet(model: AppModel) {
    val pending = model.forwarding ?: return
    val state = rememberModalBottomSheetState()
    val me = model.me?.pubkey.orEmpty()

    ModalBottomSheet(
        onDismissRequest = { model.forwarding = null },
        sheetState = state,
        containerColor = HoloBg,
    ) {
        Text(
            stringResource(R.string.forward_title, pending.second.size),
            color = Color.White,
            fontSize = 17.sp,
            modifier = Modifier.padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
        )
        LazyColumn(Modifier.fillMaxWidth().height(420.dp)) {
            if (me.isNotEmpty()) {
                item {
                    TargetRow(Icons.Filled.Bookmark, stringResource(R.string.menu_saved)) {
                        model.forwardTo(ChatTarget(me, group = false))
                    }
                }
            }
            items(model.groups.filterNot { it.archived }, key = { "g:" + it.gid }) { g ->
                TargetRow(Icons.Filled.Group, g.name) {
                    model.forwardTo(ChatTarget(g.gid, group = true))
                }
            }
            items(model.contacts, key = { "c:" + it.key }) { c ->
                TargetRow(Icons.Filled.Person, c.displayName) {
                    model.forwardTo(ChatTarget(c.key, group = false))
                }
            }
        }
        Spacer(Modifier.height(Tokens.Space.m))
    }
}

@Composable
private fun TargetRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, onClick: () -> Unit) {
    ListRow(onClick = onClick) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(12.dp))
        Text(title, color = Color.White, fontSize = 16.sp)
    }
}

/**
 * Подпись «Переслано от N» над содержимым сообщения. Имя рисуется по СВОИМ
 * контактам: на проводе едет ключ автора, а не готовая строка — иначе подписью
 * можно было бы соврать.
 */
@Composable
internal fun ForwardMark(model: AppModel, fwdFrom: String) {
    if (fwdFrom.isEmpty()) return
    val who = when {
        fwdFrom == model.me?.pubkey -> stringResource(R.string.chat_reply_you)
        else -> model.contacts.firstOrNull { it.key == fwdFrom }?.displayName
            ?: (fwdFrom.take(8) + "…")
    }
    Row(
        Modifier.fillMaxWidth().padding(bottom = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            stringResource(R.string.forward_from, who),
            color = HoloSecondary,
            fontSize = 12.sp,
            modifier = Modifier.background(Color.Transparent),
        )
    }
}
