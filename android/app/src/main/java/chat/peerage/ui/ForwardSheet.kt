@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Person
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.HoloAccent
import chat.peerage.HoloBg
import chat.peerage.HoloSecondary
import chat.peerage.R
import chat.peerage.Tokens

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
        // Скрыть автора — решение на эту пересылку. Ключ автора при этом не уходит
        // вовсе: «скрыл» не должно означать «спрятал на экране, но отдал по проводу».
        Row(
            Modifier.fillMaxWidth()
                .clickable { model.forwardHideAuthor = !model.forwardHideAuthor }
                .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Checkbox(
                checked = model.forwardHideAuthor,
                onCheckedChange = { model.forwardHideAuthor = it },
                colors = CheckboxDefaults.colors(checkedColor = HoloAccent),
            )
            Spacer(Modifier.width(4.dp))
            Column {
                Text(stringResource(R.string.forward_hide_author), color = Color.White, fontSize = 15.sp)
                Text(
                    stringResource(R.string.forward_hide_author_hint),
                    color = HoloSecondary,
                    fontSize = 12.sp,
                )
            }
        }

        LazyColumn(Modifier.fillMaxWidth().height(360.dp)) {
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
internal fun ForwardMark(model: AppModel, fwdFrom: String, anon: Boolean = false) {
    if (fwdFrom.isEmpty() && !anon) return
    // Скрытый автор — именно «переслано», без имени: сообщение не выдаётся за своё,
    // но и источник не называется.
    val label = if (anon || fwdFrom.isEmpty()) {
        stringResource(R.string.forward_from_hidden)
    } else {
        val who = when {
            fwdFrom == model.me?.pubkey -> stringResource(R.string.chat_reply_you)
            else -> model.contacts.firstOrNull { it.key == fwdFrom }?.displayName
                ?: (fwdFrom.take(8) + "…")
        }
        stringResource(R.string.forward_from, who)
    }
    // БЕЗ fillMaxWidth: подпись растягивала пузырь до предельной ширины, и короткое
    // пересланное сообщение превращалось в плиту с дырой перед временем. Ширину
    // пузыря должно задавать содержимое, а не служебная пометка.
    Row(
        Modifier.padding(bottom = 3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Иконка-стрелка: подпись читается как пометка о пересылке даже боковым
        // зрением, не сливаясь с текстом сообщения.
        //
        // Цвет — белый с прозрачностью, а НЕ серый `HoloSecondary`: на цветном пузыре
        // серый читается как пятно чужого слоя, а полупрозрачный белый остаётся в
        // цветовой семье пузыря и одинаково работает на входящем и исходящем.
        val tint = Color.White.copy(alpha = 0.55f)
        Icon(
            Icons.AutoMirrored.Filled.ArrowForward,
            null,
            tint = tint,
            modifier = Modifier.size(12.dp),
        )
        Spacer(Modifier.width(4.dp))
        Text(label, color = tint, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}
