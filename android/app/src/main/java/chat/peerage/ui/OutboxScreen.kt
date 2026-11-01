package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.launch

/**
 * Очередь отправки (макет `docs/design/mockups/outbox.html`).
 *
 * Здесь лежат СОБСТВЕННЫЕ сообщения пользователя, которые ещё не дошли. Сервера,
 * который подержал бы их за нас, нет — поэтому очередь не чистится ни по сроку, ни по
 * объёму, а разрастание только ПРЕДУПРЕЖДАЕТ. Удаление — всегда явное: по одному
 * длинным тапом, всем — в два шага.
 */
@Composable
internal fun OutboxScreen(model: AppModel, onOpenChat: (String) -> Unit) {
    var menuFor by remember { mutableStateOf<OutboxPeer?>(null) }
    var confirmFor by remember { mutableStateOf<OutboxPeer?>(null) }
    var confirmAll by remember { mutableStateOf(0) } // 0 — нет, 1 — первый шаг, 2 — второй
    LaunchedEffect(Unit) { model.refreshOutbox() }

    val rows = model.outboxByPeer
    val total = rows.sumOf { it.count }

    if (rows.isEmpty()) {
        EmptyState(
            Icons.Filled.Check,
            stringResource(R.string.outbox_empty_title),
            stringResource(R.string.outbox_empty_hint),
        )
        return
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // Что это вообще такое — сразу, а не мелким шрифтом внизу.
        InfoCard(Icons.Filled.Schedule, HoloAccent) {
            Text(
                pluralStringResource(R.plurals.outbox_total, total, total),
                color = Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
            )
            Text(
                stringResource(R.string.outbox_lead_hint),
                color = HoloSecondary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = Tokens.Space.xs),
            )
        }

        // Предупреждение, а не лимит: молча выкинуть чужие... точнее СВОИ сообщения нельзя.
        if (model.outboxWarn && model.warnOutboxGrowth) {
            InfoCard(Icons.Filled.Warning, WarnColor, bg = WarnColor.copy(alpha = 0.12f)) {
                Text(
                    stringResource(R.string.outbox_warn_title),
                    color = WarnColor,
                    fontSize = Tokens.Font.body,
                    fontWeight = FontWeight.Medium,
                )
                Text(
                    stringResource(R.string.outbox_warn_hint),
                    color = HoloSecondary,
                    fontSize = Tokens.Font.small,
                    modifier = Modifier.padding(top = Tokens.Space.xxs),
                )
                // Кнопкой: у голого текста зона касания — высота строки мелкого шрифта.
                TextButton(
                    onClick = { model.toggleOutboxWarning() },
                    contentPadding = PaddingValues(0.dp),
                ) {
                    Text(stringResource(R.string.outbox_warn_off), color = HoloAccent, fontSize = Tokens.Font.small)
                }
            }
        }

        SectionTitle(stringResource(R.string.outbox_by_peer))
        rows.forEach { r -> OutboxRow(model, r, onTap = { onOpenChat(r.peer) }) { menuFor = r } }
        Text(
            stringResource(R.string.outbox_longpress_hint),
            color = HoloSecondary,
            fontSize = Tokens.Font.small,
            modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.s),
        )

        SectionTitle(stringResource(R.string.outbox_manage))
        Row(
            Modifier.fillMaxWidth().clickable { confirmAll = 1 }
                .padding(horizontal = ScreenPad, vertical = RowPadV),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Delete, null, tint = HoloError, modifier = Modifier.size(Tokens.Size.iconM))
            Spacer(Modifier.width(Tokens.Space.m))
            Text(stringResource(R.string.outbox_clear_all), color = HoloError, fontSize = Tokens.Font.label)
        }
        Text(
            stringResource(R.string.outbox_clear_all_hint),
            color = HoloSecondary,
            fontSize = Tokens.Font.small,
            modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.xs),
        )
        Spacer(Modifier.height(Tokens.Space.l))
    }

    // Долгий тап по получателю → меню действий.
    menuFor?.let { r ->
        val name = model.contacts.firstOrNull { it.key == r.peer }?.displayName ?: r.peer.take(8)
        AlertDialog(
            onDismissRequest = { menuFor = null },
            title = { Text(name) },
            text = { Text(stringResource(R.string.outbox_menu_hint)) },
            confirmButton = {
                TextButton(onClick = {
                    menuFor = null
                    confirmFor = r
                }) { Text(stringResource(R.string.outbox_clear_one), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = {
                    menuFor = null
                    onOpenChat(r.peer)
                }) { Text(stringResource(R.string.outbox_open_chat), color = HoloAccent) }
            },
        )
    }

    // Подтверждение по одному: с точными числами, включая возраст самой старой записи.
    confirmFor?.let { r ->
        val name = model.contacts.firstOrNull { it.key == r.peer }?.displayName ?: r.peer.take(8)
        AlertDialog(
            onDismissRequest = { confirmFor = null },
            title = { Text(stringResource(R.string.outbox_clear_one_title, name)) },
            text = {
                Column {
                    Text(stringResource(R.string.outbox_clear_one_body))
                    Spacer(Modifier.height(Tokens.Space.s))
                    SumRow(stringResource(R.string.outbox_sum_count), "${r.count}")
                    SumRow(stringResource(R.string.outbox_sum_bytes), fmtSizeLong(r.bytes))
                    SumRow(stringResource(R.string.outbox_sum_oldest), agoLabel(r.oldest))
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    model.clearOutboxFor(r.peer)
                    confirmFor = null
                }) { Text(stringResource(R.string.outbox_clear_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirmFor = null }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }

    // «Очистить всё» — два шага: сперва у КОГО и сколько пропадёт, потом удержание.
    if (confirmAll == 1) {
        AlertDialog(
            onDismissRequest = { confirmAll = 0 },
            title = { Text(stringResource(R.string.outbox_clear_all_title)) },
            text = {
                Column {
                    Text(pluralStringResource(R.plurals.outbox_clear_all_body, rows.size, rows.size))
                    Spacer(Modifier.height(Tokens.Space.s))
                    rows.take(6).forEach { r ->
                        val nm = model.contacts.firstOrNull { it.key == r.peer }?.displayName ?: r.peer.take(8)
                        SumRow(nm, "${r.count} · ${fmtSizeLong(r.bytes)}")
                    }
                    // Остальные — не молча: шесть строк из двадцати читались бы как «все».
                    if (rows.size > 6) SumRow(stringResource(R.string.outbox_more, rows.size - 6), "")
                }
            },
            confirmButton = {
                TextButton(onClick = { confirmAll = 2 }) {
                    Text(stringResource(R.string.common_next), color = HoloError)
                }
            },
            dismissButton = {
                TextButton(onClick = { confirmAll = 0 }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
    if (confirmAll == 2) {
        HoldToConfirmDialog(
            title = pluralStringResource(R.plurals.outbox_clear_all_final, total, total),
            hint = stringResource(R.string.outbox_hold_hint),
            onCancel = { confirmAll = 0 },
            onConfirm = {
                model.clearOutboxAll()
                confirmAll = 0
            },
        )
    }
}

/** Строка получателя: сколько ждёт, сколько весит и почему не уходит. */
@Composable
private fun OutboxRow(model: AppModel, r: OutboxPeer, onTap: () -> Unit, onLongPress: () -> Unit) {
    val c = model.contacts.firstOrNull { it.key == r.peer }
    val name = c?.displayName ?: r.peer.take(12)
    val online = model.online[r.peer] == true
    Row(
        Modifier.fillMaxWidth()
            .pointerInput(r.peer) { detectTapGestures(onTap = { onTap() }, onLongPress = { onLongPress() }) }
            .padding(horizontal = ScreenPad, vertical = RowPadV),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SquareAvatar(name, size = Tokens.Size.avatarRow, online = online, avatarBytes = model.contactAvatars[c?.avatarId])
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Text(name, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (online) Icons.Filled.Send else Icons.Filled.Schedule,
                    null,
                    tint = if (online) HoloOnline else HoloSecondary,
                    modifier = Modifier.size(Tokens.Size.iconXs),
                )
                Spacer(Modifier.width(Tokens.Space.xs))
                // Причина, а не только факт: разросшаяся очередь к человеку, которого
                // месяц не было в сети, — это ожидаемое поведение, а не поломка.
                Text(
                    if (online) stringResource(R.string.outbox_sending) else stringResource(R.string.outbox_offline_for, agoLabel(r.oldest)),
                    color = if (online) HoloOnline else HoloSecondary,
                    fontSize = Tokens.Font.body,
                )
            }
        }
        Column(horizontalAlignment = Alignment.End) {
            Text("${r.count}", color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            Text(fmtSizeLong(r.bytes), color = HoloSecondary, fontSize = Tokens.Font.small)
        }
    }
}

/** Финальное подтверждение УДЕРЖАНИЕМ: промахнуться по кнопке можно, случайно
 *  удержать её две секунды — нет. */
@Composable
internal fun HoldToConfirmDialog(
    title: String,
    hint: String,
    onCancel: () -> Unit,
    onConfirm: () -> Unit,
) {
    var progress by remember { mutableFloatStateOf(0f) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(title) },
        text = {
            Column {
                Text(hint)
                Spacer(Modifier.height(Tokens.Space.l))
                Box(
                    Modifier.fillMaxWidth().height(46.dp).clip(RoundedCornerShape(Tokens.Radius.button))
                        .background(HoloBg)
                        .pointerInput(Unit) {
                            detectTapGestures(
                                onPress = {
                                    // Пока палец на кнопке — полоса растёт; дошла до конца
                                    // (2 с) — подтверждаем. Отпустил раньше — всё обнуляем.
                                    val fill = scope.launch {
                                        while (progress < 1f) {
                                            kotlinx.coroutines.delay(50)
                                            progress = (progress + 0.025f).coerceAtMost(1f)
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
                        Modifier.fillMaxHeight().fillMaxWidth(progress)
                            .background(Tokens.Colors.errorFill.copy(alpha = 0.2f)),
                    )
                    Text(stringResource(R.string.common_hold), color = HoloError, fontWeight = FontWeight.Medium)
                }
            }
        },
        confirmButton = {},
        dismissButton = {
            TextButton(onClick = onCancel) {
                Text(stringResource(R.string.common_cancel), color = HoloSecondary)
            }
        },
    )
}

@Composable
private fun SumRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 1.dp)) {
        Text(label, color = HoloSecondary, fontSize = Tokens.Font.body, modifier = Modifier.weight(1f))
        Text(value, color = Color.White, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium)
    }
}

/** Карточка-пояснение с иконкой. */
@Composable
private fun InfoCard(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    tint: Color,
    bg: Color = HoloSurface,
    content: @Composable ColumnScope.() -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = ScreenPad, vertical = Tokens.Space.s)
            .clip(RoundedCornerShape(Tokens.Radius.tile)).background(bg).padding(Tokens.Space.l),
    ) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Column(content = content)
    }
}

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
