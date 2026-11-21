package chat.peerage.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.AppModel
import chat.peerage.BlockedPeer
import chat.peerage.R
import chat.peerage.Tokens

/**
 * «Заблокированные» (макет `misc/Blocked`, UI 5.2): кто заблокирован и с какого дня.
 * Бывший контакт подписан именем, сохранённым при блокировке; незнакомец — ключом.
 */
@Composable
internal fun SettingsBlockedScreen(model: AppModel) {
    val list = model.blocked
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.l, bottom = Tokens.Space.xxl),
    ) {
        if (list.isEmpty()) {
            Caption(stringResource(R.string.blocked_empty))
        } else {
            CardSection {
                list.forEachIndexed { i, b -> BlockedRow(b, divider = i > 0) { model.unblock(b.key) } }
            }
            Caption(stringResource(R.string.blocked_caption))
        }
    }
}

@Composable
private fun BlockedRow(b: BlockedPeer, divider: Boolean, onUnblock: () -> Unit) {
    val short = b.key.take(8)
    val since = fmtSince(b.time)
    val named = b.name.isNotBlank()
    CardRow(divider = divider, inset = RowInset, modifier = Modifier.heightIn(min = Tokens.Size.personRow)) {
        if (named) {
            SquareAvatar(b.name, size = Tokens.Size.avatarPick, circle = true)
        } else {
            SquareAvatar("", size = Tokens.Size.avatarPick, circle = true, bg = Tokens.Colors.raised) {
                Icon(TablerIcons.Key, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
            }
        }
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f)) {
            Text(
                if (named) b.name else stringResource(R.string.blocked_key, short),
                color = Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                when {
                    !b.contact -> stringResource(R.string.blocked_sub_stranger, since)
                    // Ключ уже в заголовке — второй раз не нужен.
                    !named -> stringResource(R.string.blocked_sub_since, since)
                    else -> stringResource(R.string.blocked_sub_contact, short, since)
                },
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                // Дата важнее ширины: рядом пилюля, и в одну строку подпись не влезает.
                maxLines = 2,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        PillButton(stringResource(R.string.blocked_unblock), neutral = true, onClick = onUnblock)
    }
}

/** «28 октября»; год — только если не текущий. */
private fun fmtSince(ts: Long): String {
    val d = java.util.Date(ts * 1000)
    val cal = java.util.Calendar.getInstance()
    val year = cal.get(java.util.Calendar.YEAR)
    cal.time = d
    val pattern = if (cal.get(java.util.Calendar.YEAR) == year) "d MMMM" else "d MMMM yyyy"
    // Дата не рвётся переносом: «с 21 / ноября» читается хуже, чем перенос после «·».
    return java.text.SimpleDateFormat(pattern, java.util.Locale.getDefault()).format(d).replace(' ', ' ')
}

private val RowInset = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m + Tokens.Space.xxs
