package chat.peerage.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowForward
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.AppModel
import chat.peerage.HoloAccent
import chat.peerage.HoloSecondary
import chat.peerage.R
import chat.peerage.Tokens

/** Шапка выбора, куда переслать (п. 57): сколько и крестик — отменить пересылку. */
@Composable
internal fun ForwardTopBar(model: AppModel, onCancel: () -> Unit) {
    TopBar(
        stringResource(R.string.forward_title, model.forwarding?.second?.size ?: 0),
        leading = { BarIcon(TablerIcons.X, stringResource(R.string.common_cancel), onClick = onCancel) },
    )
}

/**
 * «Скрыть автора» над списком выбора. Решение — на эту пересылку; ключ автора при этом не
 * уходит вовсе: «скрыл» не должно означать «спрятал на экране, но отдал по проводу».
 */
@Composable
internal fun ForwardHideAuthorRow(model: AppModel) {
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
        Spacer(Modifier.width(Tokens.Space.xs))
        Column {
            Text(stringResource(R.string.forward_hide_author), color = Color.White, fontSize = Tokens.Font.label)
            Text(stringResource(R.string.forward_hide_author_hint), color = HoloSecondary, fontSize = Tokens.Font.small)
        }
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
        Modifier.padding(bottom = Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Иконка-стрелка: подпись читается как пометка о пересылке даже боковым
        // зрением, не сливаясь с текстом сообщения.
        //
        // Цвет — белый с прозрачностью, а НЕ серый `HoloSecondary`: на цветном пузыре
        // серый читается как пятно чужого слоя, а полупрозрачный белый остаётся в
        // цветовой семье пузыря и одинаково работает на входящем и исходящем.
        val tint = Tokens.Colors.metaOnBubble
        Icon(
            Icons.AutoMirrored.Filled.ArrowForward,
            null,
            tint = tint,
            modifier = Modifier.size(Tokens.Size.iconXs),
        )
        Spacer(Modifier.width(Tokens.Space.xs))
        Text(label, color = tint, fontSize = Tokens.Font.small, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}
