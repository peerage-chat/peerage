package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Карточка запроса в контакты: кто пишет, когда и с чем. Решение — здесь, а не кнопками в
 * строке главной: принять незнакомца стоит, только увидев всё, что он прислал.
 * Запрос исчез (принят, отклонён, заблокирован) — [onDone] возвращает на главную.
 */
@Composable
internal fun RequestScreen(model: AppModel, key: String, onDone: () -> Unit) {
    val r = model.contactRequests.firstOrNull { it.key == key }
    LaunchedEffect(r == null) { if (r == null) onDone() }
    if (r == null) return
    var confirmBlock by remember { mutableStateOf(false) }
    DecisionLayout(
        footer = {
            GhostButton(stringResource(R.string.req_block), Tokens.Colors.error, Modifier.fillMaxWidth()) { confirmBlock = true }
            PrimaryButton(stringResource(R.string.req_accept)) { model.acceptRequest(r.key) }
            GhostButton(stringResource(R.string.req_decline), modifier = Modifier.fillMaxWidth()) { model.declineRequest(r.key) }
        },
    ) {
        DecisionHead(r.shownName, fmtListTime(r.time), group = false)
        SectionTitle(stringResource(R.string.req_greeting))
        Box(
            Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth()
                .clip(GreetingShape)
                .background(Tokens.Colors.raised)
                .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs, vertical = Tokens.Space.bubbleH),
        ) {
            Text(
                r.greeting.ifBlank { stringResource(R.string.req_no_message) },
                color = if (r.greeting.isBlank()) Tokens.Colors.tertiary else Color.White,
                fontSize = Tokens.Font.label,
            )
        }
    }
    if (confirmBlock) {
        ConfirmSheet(
            title = stringResource(R.string.req_block_title, r.shownName),
            text = stringResource(R.string.req_block_text),
            ok = stringResource(R.string.req_block),
            onOk = { model.blockRequest(r.key) },
            onClose = { confirmBlock = false },
        )
    }
}

/**
 * Карточка приглашения в группу: название и кто позвал. После «Вступить» карточка ждёт ответа
 * пригласившего (он может быть не в сети) и закрывается сама, когда вступление завершилось.
 */
@Composable
internal fun InviteScreen(model: AppModel, gid: String, onDone: () -> Unit) {
    val inv = model.groupInvites.firstOrNull { it.gid == gid }
    LaunchedEffect(inv == null) { if (inv == null) onDone() }
    if (inv == null) return
    DecisionLayout(
        footer = {
            if (!inv.waiting) {
                PrimaryButton(stringResource(R.string.group_invite_join)) { model.acceptGroupInvite(inv.gid) }
                GhostButton(stringResource(R.string.req_decline), modifier = Modifier.fillMaxWidth()) {
                    model.declineGroupInvite(inv.gid)
                }
            }
        },
    ) {
        DecisionHead(
            inv.name.ifBlank { stringResource(R.string.group_default_name) },
            stringResource(R.string.group_invite_from, model.peerName(inv.inviter)),
            group = true,
        )
        if (inv.waiting) {
            Text(
                stringResource(R.string.group_invite_waiting),
                color = Tokens.Colors.secondary,
                fontSize = Tokens.Font.preview,
                modifier = Modifier.padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.xl)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(Tokens.Radius.group))
                    .background(Tokens.Colors.group)
                    .padding(Tokens.Space.l),
            )
        }
    }
}

/** Раскладка карточки-решения: прокручиваемое содержимое и кнопки, прибитые к низу. */
@Composable
private fun DecisionLayout(footer: @Composable ColumnScope.() -> Unit, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(top = Tokens.Space.xl),
            content = content,
        )
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(horizontal = Tokens.Space.xl, vertical = Tokens.Space.m),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
            content = footer,
        )
    }
}

/** Шапка карточки: крупный аватар, имя и строка под ним. */
@Composable
private fun DecisionHead(title: String, caption: String, group: Boolean) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panelText),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        SquareAvatar(title, size = Tokens.Size.avatarProfile, circle = !group)
        Column(Modifier.weight(1f)) {
            Text(
                title,
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = FontWeight.Bold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (caption.isNotEmpty()) {
                Text(caption, style = Tokens.Text.meta, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
            }
        }
    }
}

/** Главная кнопка экрана во всю ширину. */
@Composable
internal fun PrimaryButton(text: String, onClick: () -> Unit) {
    AccentButton(text, Modifier.fillMaxWidth().heightIn(min = Tokens.Size.primaryButton), onClick = onClick)
}

/** Приветствие — пузырём, как оно ляжет в чат после принятия. */
private val GreetingShape = RoundedCornerShape(
    topStart = Tokens.Radius.bubble,
    topEnd = Tokens.Radius.bubble,
    bottomEnd = Tokens.Radius.bubble,
    bottomStart = Tokens.Radius.bubbleTail,
)
