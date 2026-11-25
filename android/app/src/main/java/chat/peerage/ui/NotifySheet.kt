package chat.peerage.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.NotifyMode
import chat.peerage.R
import chat.peerage.Tokens
import java.text.DateFormat
import java.util.Date

/** Название режима для строк и подписей. */
@Composable
internal fun notifyLabel(mode: NotifyMode): String = stringResource(
    when (mode) {
        NotifyMode.All -> R.string.notif_mode_all
        NotifyMode.Mentions -> R.string.notif_mode_mentions
        NotifyMode.Nothing -> R.string.notif_mode_none
    },
)

/** «до 14:30» / «до 24 нояб., 14:30» — когда кончится свой выбор; 0 — пока не включат. */
@Composable
private fun untilLabel(untilSec: Long): String {
    if (untilSec == 0L) return stringResource(R.string.notif_for_ever)
    val at = Date(untilSec * 1000)
    val sameDay = DateFormat.getDateInstance().format(at) == DateFormat.getDateInstance().format(Date())
    val text = if (sameDay) {
        clockFormat().format(at)
    } else {
        DateFormat.getDateInstance(DateFormat.MEDIUM).format(at) + ", " + clockFormat().format(at)
    }
    return stringResource(R.string.notif_until, text)
}

@Composable
private fun SheetTitle(text: String) {
    Text(
        text,
        color = Color.White,
        fontSize = Tokens.Font.sheet,
        fontWeight = FontWeight.Bold,
        modifier = Modifier.padding(horizontal = Tokens.Space.xs),
    )
}

/**
 * Режим уведомлений чата (notifications.md): «все / упоминания / ничего». Всё, кроме «все»,
 * — со сроком: вторым шагом 1 ч / 8 ч / сутки / пока не включу. В личном чате упоминаний
 * нет. В группе первой строкой — умолчание группы (его ставит админ).
 */
@Composable
internal fun NotifySheet(model: AppModel, chat: ChatTarget, onClose: () -> Unit) {
    val own = model.ownNotify(chat.id, chat.group)
    val default = model.notifyDefault(chat.id, chat.group)
    var picking by remember(chat) { mutableStateOf<NotifyMode?>(null) }
    val mode = picking
    if (mode != null) {
        SheetFrame(onClose) {
            SheetTitle(stringResource(R.string.notif_for_title))
            Column(Modifier.selectableGroup()) {
                val labels = listOf(R.string.notif_for_1h, R.string.notif_for_8h, R.string.notif_for_day, R.string.notif_for_ever)
                NotifyMode.DURATIONS.zip(labels).forEach { (sec, label) ->
                    OptionRow(stringResource(label), on = false) {
                        model.setChatNotify(chat.id, chat.group, mode, sec)
                        onClose()
                    }
                }
            }
        }
        return
    }
    val pick: (NotifyMode?) -> Unit = { m ->
        when {
            // Свой «все» там, где умолчание и так «все», — это и есть умолчание.
            m == null || (m == NotifyMode.All && default == NotifyMode.All) -> {
                model.setChatNotify(chat.id, chat.group, null)
                onClose()
            }
            m == NotifyMode.All -> {
                model.setChatNotify(chat.id, chat.group, NotifyMode.All)
                onClose()
            }
            else -> picking = m
        }
    }
    SheetFrame(onClose) {
        SheetTitle(stringResource(R.string.notif_mode_title))
        Column(Modifier.selectableGroup()) {
            if (chat.group) {
                OptionRow(stringResource(R.string.notif_mode_inherit), sub = notifyLabel(default), on = own == null) { pick(null) }
            }
            val modes = if (chat.group) NotifyMode.entries else listOf(NotifyMode.All, NotifyMode.Nothing)
            for (m in modes) {
                val on = own?.first == m || (!chat.group && own == null && m == NotifyMode.All)
                val sub = if (on && m != NotifyMode.All && own != null) untilLabel(own.second) else null
                OptionRow(notifyLabel(m), sub = sub, on = on) { pick(m) }
            }
        }
        Text(
            stringResource(R.string.notif_mode_cap),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
    }
}

/** Режим по умолчанию для участников группы (только админ): «ничего» навязать нельзя. */
@Composable
internal fun GroupNotifyDefaultSheet(model: AppModel, gid: String, onClose: () -> Unit) {
    val current = model.notifyDefault(gid, group = true)
    SheetFrame(onClose) {
        SheetTitle(stringResource(R.string.notif_default_title))
        Column(Modifier.selectableGroup()) {
            for (m in listOf(NotifyMode.All, NotifyMode.Mentions)) {
                OptionRow(notifyLabel(m), on = current == m) {
                    if (m != current) model.setGroupNotifyDefault(gid, m)
                    onClose()
                }
            }
        }
        Text(
            stringResource(R.string.notif_default_cap),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
    }
}

/**
 * Пункт «Уведомления» в меню чата: режим виден по колокольчику (перечёркнут — не «все»),
 * выбор — в шторке. Подписи режима справа нет: в узком меню она ломала название на две строки.
 */
@Composable
internal fun NotifyMenuItem(model: AppModel, chat: ChatTarget, onPick: () -> Unit) {
    val mode = model.notifyMode(chat.id, chat.group)
    PopMenuItem(
        if (mode == NotifyMode.All) TablerIcons.Bell else TablerIcons.BellOff,
        stringResource(R.string.notif_mode_title),
        onClick = onPick,
    )
}

/** Строка «Уведомления · режим» в профиле группы (макет `prof/GroupA`) со своей шторкой. */
@Composable
internal fun ProfileNotifyRow(model: AppModel, chat: ChatTarget) {
    var open by remember(chat) { mutableStateOf(false) }
    val mode = model.notifyMode(chat.id, chat.group)
    NavRow(
        if (mode == NotifyMode.All) TablerIcons.Bell else TablerIcons.BellOff,
        stringResource(R.string.notif_mode_title),
        value = notifyLabel(mode),
        onClick = { open = true },
    )
    if (open) NotifySheet(model, chat) { open = false }
}
