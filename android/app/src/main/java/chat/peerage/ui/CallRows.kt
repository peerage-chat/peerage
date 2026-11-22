package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import chat.peerage.AppModel
import chat.peerage.BubbleIn
import chat.peerage.BubbleOut
import chat.peerage.Message
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Звонок в ленте личного чата (макет `special/Calls`): строка, которую движок ведёт по
 * ходу звонка (журнал движка, 2.8б) — исход в `text`, длительность в `durationMs`. Стоит
 * на стороне звонившего, как сообщение; справа — «Позвонить» тем же видом звонка.
 */
@Composable
internal fun CallRow(model: AppModel, m: Message, peer: String) {
    val look = callLook(m)
    Row(
        Modifier.fillMaxWidth().padding(bottom = Tokens.Space.xs),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start,
    ) {
        Row(
            Modifier.widthIn(min = Tokens.Size.callRowMin, max = LocalBubbleMax.current)
                .clip(RoundedCornerShape(Tokens.Radius.bubble))
                .background(if (m.mine) BubbleOut else BubbleIn)
                .padding(start = Tokens.Space.m, end = Tokens.Space.panel, top = Tokens.Space.panel, bottom = Tokens.Space.panel),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            Box(
                Modifier.size(Tokens.Size.callBadge).clip(CircleShape).background(look.bg),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    if (m.kind == "video_call") TablerIcons.Video else TablerIcons.Phone,
                    null,
                    tint = look.fg,
                    modifier = Modifier.size(Tokens.Size.iconS),
                )
            }
            Column(Modifier.weight(1f)) {
                Text(callTitle(m), color = Tokens.Colors.onDark, fontSize = Tokens.Font.label)
                val time = fmtMsgTime(m.time)
                Text(
                    if (m.text == "answered" && m.durationMs > 0) {
                        stringResource(R.string.chat_call_meta, fmtCallDuration(m.durationMs / 1000L), time)
                    } else {
                        time
                    },
                    color = Tokens.Colors.secondary,
                    fontSize = Tokens.Font.small,
                    style = Tokens.Text.tabular,
                )
            }
            Box(
                Modifier.size(Tokens.Size.callBadge).clip(CircleShape)
                    .background(Tokens.Colors.onBubbleButton)
                    .clickable(role = Role.Button) { model.callDm(peer, video = m.kind == "video_call") },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    TablerIcons.Phone,
                    stringResource(R.string.chat_call_back),
                    tint = Tokens.Colors.iconChrome,
                    modifier = Modifier.size(Tokens.Size.iconS),
                )
            }
        }
    }
}

/** Цвет значка по исходу: пропущенный — красный, отвеченный входящий — зелёный, свой — акцент. */
private class CallLook(val fg: Color, val bg: Color)

private fun callLook(m: Message): CallLook = when {
    !m.mine && m.text != "answered" -> CallLook(Tokens.Colors.failText, Tokens.Colors.dangerPlate)
    !m.mine -> CallLook(Tokens.Colors.callAnswered, Tokens.Colors.callAnsweredBg)
    m.text == "answered" -> CallLook(Tokens.Colors.accentSoft, Tokens.Colors.accentMuted)
    else -> CallLook(Tokens.Colors.secondary, Tokens.Colors.raised)
}

/** «Пропущенный видеозвонок», «Исходящий · без ответа» — и в ленте, и в превью списка чатов. */
@Composable
internal fun callTitle(m: Message): String = callTitle(m.kind == "video_call", m.mine, m.text)

@Composable
internal fun callTitle(video: Boolean, mine: Boolean, outcome: String): String = stringResource(
    when {
        mine && outcome != "answered" -> R.string.chat_call_unanswered
        mine -> if (video) R.string.chat_call_out_video else R.string.chat_call_out
        outcome == "missed" -> if (video) R.string.chat_call_missed_video else R.string.chat_call_missed
        outcome == "declined" -> if (video) R.string.chat_call_declined_video else R.string.chat_call_declined
        else -> if (video) R.string.chat_call_in_video else R.string.chat_call_in
    },
)
