package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.launch

/**
 * «Исходящие» (макеты `misc/Outbox`, `misc/OutboxHold`): плитки «сообщений · объём · самое
 * старое», предупреждение от тысячи, очереди по получателям, «Очистить всё» — удержанием.
 *
 * Здесь лежат СВОИ сообщения, которые ещё не дошли. Сервера, который подержал бы их, нет —
 * поэтому очередь сама не чистится ни по сроку, ни по объёму, а разрастание только
 * предупреждает. Сброс — всегда явный. Сброшенное остаётся в чате «Не доставлено» с
 * «Повторить».
 */
@Composable
internal fun OutboxScreen(model: AppModel, onOpenChat: (String) -> Unit) {
    var peerSheet by remember { mutableStateOf<OutboxPeer?>(null) }
    var clearAll by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { model.refreshOutbox() }

    val rows = model.outboxByPeer
    if (rows.isEmpty()) {
        EmptyState(
            TablerIcons.Check,
            stringResource(R.string.outbox_empty_title),
            stringResource(R.string.outbox_empty_hint),
        )
        return
    }
    val total = rows.sumOf { it.count }

    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState())
                .padding(top = Tokens.Space.l, bottom = Tokens.Space.l),
        ) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panel),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
            ) {
                StatTile(
                    fmtCount(total),
                    pluralStringResource(R.plurals.outbox_tile_msgs, total),
                    Modifier.weight(1f),
                    bg = Tokens.Colors.group,
                    size = Tokens.Font.sheet,
                )
                StatTile(
                    fmtSizeLong(rows.sumOf { it.bytes }),
                    stringResource(R.string.outbox_tile_bytes),
                    Modifier.weight(1f),
                    bg = Tokens.Colors.group,
                    size = Tokens.Font.sheet,
                )
                StatTile(
                    agoLabel(rows.minOf { it.oldest }),
                    stringResource(R.string.outbox_tile_oldest),
                    Modifier.weight(1f),
                    bg = Tokens.Colors.group,
                    size = Tokens.Font.sheet,
                )
            }
            Text(
                stringResource(R.string.outbox_lead),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(
                    start = Tokens.Space.panelText,
                    end = Tokens.Space.panelText,
                    top = Tokens.Space.m,
                ),
            )
            // Предупреждение, а не лимит: молча выкинуть СВОИ сообщения нельзя.
            if (model.outboxWarn && model.warnOutboxGrowth) GrowthWarning()

            SectionTitle(stringResource(R.string.outbox_by_peer))
            CardSection {
                rows.forEachIndexed { i, r -> OutboxRow(model, r, divider = i > 0) { peerSheet = r } }
            }
        }
        Box(
            Modifier.fillMaxWidth().padding(
                start = Tokens.Space.xl,
                end = Tokens.Space.xl,
                top = Tokens.Space.m,
                bottom = Tokens.Space.xxl,
            ),
        ) {
            PlateButton(stringResource(R.string.outbox_clear_all), modifier = Modifier.fillMaxWidth(), danger = true) {
                clearAll = true
            }
        }
    }

    peerSheet?.let { r ->
        val name = peerName(model, r.peer)
        ActionSheet(name, stringResource(R.string.outbox_clear_peer_body), onClose = { peerSheet = null }) {
            PlateButton(stringResource(R.string.outbox_clear_peer), modifier = Modifier.fillMaxWidth(), danger = true) {
                model.clearOutboxFor(r.peer)
                peerSheet = null
            }
            PlateButton(stringResource(R.string.outbox_open_chat), modifier = Modifier.fillMaxWidth()) {
                peerSheet = null
                onOpenChat(r.peer)
            }
        }
    }

    // «Очистить всё» — удержанием: промахнуться по кнопке можно, случайно держать её
    // две секунды — нет.
    if (clearAll) {
        ActionSheet(
            stringResource(R.string.outbox_clear_all_title),
            pluralStringResource(R.plurals.outbox_clear_all_body, total, fmtCount(total)),
            onClose = { clearAll = false },
        ) {
            HoldButton(stringResource(R.string.outbox_hold)) {
                model.clearOutboxAll()
                clearAll = false
            }
            PlateButton(stringResource(R.string.outbox_keep), modifier = Modifier.fillMaxWidth()) { clearAll = false }
        }
    }
}

/** Плашка «Очередь разрослась» (`misc/Outbox`): тот же тон, что «Ключ не сверен». */
@Composable
private fun GrowthWarning() {
    Row(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.l)
            .clip(RoundedCornerShape(Tokens.Radius.group))
            .background(Tokens.Colors.stateWarn)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m + Tokens.Space.xxs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(TablerIcons.AlertTriangle, null, tint = Tokens.Colors.stateWarnText, modifier = Modifier.size(Tokens.Size.iconRow))
        Column {
            Text(
                stringResource(R.string.outbox_warn_title),
                color = Tokens.Colors.stateWarnText,
                fontSize = Tokens.Font.preview,
                fontWeight = FontWeight.Medium,
            )
            Text(
                stringResource(R.string.outbox_warn_hint),
                color = Tokens.Colors.stateWarnText,
                fontSize = Tokens.Font.preview,
                lineHeight = Tokens.Font.lineHeight,
            )
        }
    }
}

/** Получатель: «1 204 · 3,2 МБ · не в сети 24 дня» или «… · отправляется…». */
@Composable
private fun OutboxRow(model: AppModel, r: OutboxPeer, divider: Boolean, onTap: () -> Unit) {
    val c = model.contacts.firstOrNull { it.key == r.peer }
    val name = peerName(model, r.peer)
    val online = model.online[r.peer] == true
    // Причина, а не только факт: очередь к человеку, которого месяц не было, — ожидаемое.
    val state = if (online) stringResource(R.string.outbox_sending) else stringResource(R.string.outbox_offline_for, agoLabel(r.oldest))
    CardRow(onClick = onTap, divider = divider, inset = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m + Tokens.Space.xxs) {
        SquareAvatar(name, size = Tokens.Size.avatarPick, avatarBytes = model.contactAvatars[c?.avatarId], circle = true)
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f).heightIn(min = Tokens.Size.personRow - Tokens.Space.l), verticalArrangement = Arrangement.Center) {
            Text(name, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1)
            Text(
                stringResource(R.string.outbox_row_sub, fmtCount(r.count), fmtSizeLong(r.bytes), state),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                maxLines = 1,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
        Icon(TablerIcons.ChevronRight, null, tint = Tokens.Colors.chevron, modifier = Modifier.size(Tokens.Size.iconS))
    }
}

/**
 * Красная кнопка подтверждения удержанием: пока палец на кнопке, растёт заливка; дошла до
 * конца (2 с) — действие. Отпустил раньше — всё обнуляется.
 */
@Composable
private fun HoldButton(text: String, onConfirm: () -> Unit) {
    var progress by remember { mutableFloatStateOf(0f) }
    val scope = rememberCoroutineScope()
    Box(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.touch)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .background(Tokens.Colors.dangerPlate)
            .semantics { role = Role.Button }
            .pointerInput(Unit) {
                detectTapGestures(
                    onPress = {
                        val fill = scope.launch {
                            while (progress < 1f) {
                                kotlinx.coroutines.delay(HoldStepMs)
                                progress = (progress + HoldStep).coerceAtMost(1f)
                            }
                            onConfirm()
                        }
                        tryAwaitRelease()
                        if (progress < 1f) {
                            fill.cancel()
                            progress = 0f
                        }
                    },
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.matchParentSize().wrapContentWidth(Alignment.Start).fillMaxWidth(progress)
                .background(Tokens.Colors.holdFill),
        )
        Text(text, color = Tokens.Colors.iconDangerMuted, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
    }
}

private const val HoldStepMs = 50L
private const val HoldStep = 0.025f // 40 шагов по 50 мс — 2 секунды

@Composable
private fun peerName(model: AppModel, peer: String): String =
    model.contacts.firstOrNull { it.key == peer }?.displayName ?: peer.take(12)

/** «1 248» — с разделителем разрядов по языку. */
private fun fmtCount(n: Int): String = java.text.NumberFormat.getIntegerInstance().format(n)

/** «24 дня» / «3 часа» — возраст самой старой записи в очереди. */
@Composable
private fun agoLabel(since: Long): String {
    val sec = (System.currentTimeMillis() / 1000 - since).coerceAtLeast(0)
    return when {
        sec >= 86_400 -> (sec / 86_400).toInt().let { pluralStringResource(R.plurals.ago_days, it, it) }
        sec >= 3_600 -> (sec / 3_600).toInt().let { pluralStringResource(R.plurals.ago_hours, it, it) }
        else -> stringResource(R.string.ago_minutes, (sec / 60).coerceAtLeast(1))
    }
}
