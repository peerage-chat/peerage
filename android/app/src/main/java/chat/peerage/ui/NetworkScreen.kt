@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import chat.peerage.*
import chat.peerage.R

/**
 * Шапка «Сети» (макет `screens/Network`): заголовок со статусом и плитки Пиры · Пинг · Трафик —
 * над панелью, на фоне экрана. В офлайне справа — пилюля «В сеть».
 */
@Composable
internal fun NetworkHeader(model: AppModel, onBack: () -> Unit) {
    val ns = model.netStats
    val peers = ns?.peerCount ?: model.netPeers
    val (status, color) = when {
        model.offline -> stringResource(R.string.net_status_away) to Tokens.Colors.warn
        peers == 0 -> stringResource(R.string.net_status_offline) to Tokens.Colors.tertiary
        else -> stringResource(R.string.net_connected) to Tokens.Colors.online
    }
    Column(Modifier.fillMaxWidth().background(Tokens.Colors.bg)) {
        TopBar(stringResource(R.string.menu_network), onBack = onBack, subtitle = status, subtitleColor = color) {
            if (model.offline) PillButton(stringResource(R.string.net_go_online)) { model.goOnline() }
        }
        val ping = ns?.pingMs?.takeIf { it > 0 }
        val (rate, unit) = splitSize(fmtSizeLong((ns?.rxRate ?: 0) + (ns?.txRate ?: 0)))
        Row(
            Modifier.padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.s, bottom = Tokens.Space.l),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s + Tokens.Space.xxs),
        ) {
            StatTile("$peers", stringResource(R.string.net_metric_peers), Modifier.weight(1f))
            StatTile(
                ping?.let { "%.0f".format(it) } ?: "—",
                stringResource(R.string.net_metric_ping),
                Modifier.weight(1f),
                unit = if (ping != null) stringResource(R.string.net_unit_ms) else null,
            )
            StatTile(
                if (ns == null) "—" else rate,
                stringResource(R.string.net_metric_traffic),
                Modifier.weight(1f),
                unit = if (ns == null) null else stringResource(R.string.net_rate, unit),
            )
        }
    }
}

/** «Сеть» на панели: поток (если идёт), мой узел, режим участия, пиры, очередь и замер. */
@Composable
internal fun NetworkScreen(
    model: AppModel,
    onOpenOutbox: () -> Unit,
    onFlood: () -> Unit,
    onRotate: () -> Unit,
) {
    // Счётчик «Офлайн-очередь» — свежий на входе: событий о каждой постановке в очередь нет.
    LaunchedEffect(Unit) { model.refreshOutbox() }
    val me = model.me ?: return
    val ctx = LocalContext.current
    var benchPick by remember { mutableStateOf(false) }
    var benchTarget by remember { mutableStateOf<Contact?>(null) }
    var allPeers by remember { mutableStateOf(false) }
    // Офлайн-очередь = сообщения, ждущие появления получателя (статус "queued").
    val queued = model.messages.values.sumOf { l -> l.count { it.status == "queued" } }
    val flood = model.floodPeers.filter { it.bps > 0 }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.s, bottom = Tokens.Space.xxl),
    ) {
        // Поток трафика — только пока он идёт, выше всего: это самое важное на экране.
        if (flood.isNotEmpty()) {
            Spacer(Modifier.height(Tokens.Space.s))
            CardSection {
                NavRow(
                    TablerIcons.AlertTriangle,
                    stringResource(R.string.flood_screen_title),
                    divider = false,
                    sub = stringResource(
                        R.string.flood_card_sub,
                        stringResource(R.string.net_rate, fmtSizeLong(flood.sumOf { it.bps })),
                        fmtSizeLong(model.floodPeers.sumOf { it.bytes }),
                    ),
                    iconTint = Tokens.Colors.warn,
                    iconBg = Tokens.Colors.stateWarn,
                    onClick = onFlood,
                )
            }
        }

        SectionTitle(stringResource(R.string.net_my_node))
        CardSection {
            CardRow(
                divider = false,
                modifier = Modifier.clickable(role = Role.Button, onClickLabel = stringResource(R.string.net_copy_address)) {
                    copyToClipboard(ctx, me.address, ctx.getString(R.string.net_address))
                },
            ) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.net_address), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
                    MonoText(me.address, Color.White)
                }
                Spacer(Modifier.width(Tokens.Space.m))
                Icon(
                    TablerIcons.Copy,
                    stringResource(R.string.net_copy_address),
                    tint = Tokens.Colors.secondary,
                    modifier = Modifier.size(Tokens.Size.iconRow),
                )
            }
            val since = rotatedOn(model.transportEpoch)
            NetRow(
                stringResource(R.string.net_rotate_row),
                if (since != null) {
                    stringResource(R.string.net_rotate_row_since, since)
                } else {
                    stringResource(R.string.net_rotate_row_desc)
                },
                onClick = onRotate,
            )
        }

        // Переключателя нет: узел Yggdrasil пересылает чужой трафик всегда, а режима пира
        // в движке пока нет (ADR-016). Выключатель, который ничего не выключает, хуже его
        // отсутствия — строка говорит, как есть.
        SectionTitle(stringResource(R.string.net_mode))
        CardSection {
            NetRow(stringResource(R.string.net_node_mode), stringResource(R.string.net_node_mode_desc), divider = false)
        }

        val peers = model.netStats?.peers ?: emptyList()
        SectionTitle(stringResource(R.string.net_peers_label, model.netStats?.peerCount ?: model.netPeers))
        CardSection {
            if (peers.isEmpty()) {
                CardRow(divider = false) {
                    Text(stringResource(R.string.net_no_peers), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
                }
            }
            val shown = if (allPeers) peers else peers.take(PEERS_SHOWN)
            shown.forEachIndexed { i, p -> PeerRow(p, divider = i > 0) }
            if (!allPeers && peers.size > PEERS_SHOWN) {
                CardRow(inset = Tokens.Space.l, modifier = Modifier.clickable(role = Role.Button) { allPeers = true }) {
                    Text(stringResource(R.string.net_all_peers), color = Tokens.Colors.accent, fontSize = Tokens.Font.label)
                }
            }
        }

        Spacer(Modifier.height(Tokens.Space.l))
        CardSection {
            NetRow(
                stringResource(R.string.net_offline_queue, queued),
                stringResource(R.string.net_offline_queue_desc),
                divider = false,
                onClick = onOpenOutbox,
            )
            // Замер — диагностика: контакт выбирают явно, объём назван до запуска. Кнопкой в
            // профиле каждого контакта он был флудом с парадного входа (~30 МБ за тап).
            NetRow(
                stringResource(R.string.net_bench_row),
                model.benchResult.ifEmpty { stringResource(R.string.net_bench_row_desc) },
                onClick = { benchPick = true },
            )
        }
    }

    // Выбор пира: только те, кто сейчас на связи. Оффлайновому замер осядет в очередь и
    // уедет к нему потом — это уже не диагностика, а сюрприз.
    if (benchPick) {
        val online = model.contacts.filter { model.online[it.key] == true }
        ActionSheet(
            stringResource(R.string.net_bench_row),
            if (online.isEmpty()) stringResource(R.string.net_bench_nobody) else "",
            onClose = { benchPick = false },
        ) {
            online.forEach { c ->
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.button))
                        .clickable(role = Role.Button) {
                            benchPick = false
                            benchTarget = c
                        }
                        .padding(vertical = Tokens.Space.s, horizontal = Tokens.Space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SquareAvatar(c.displayName, size = Tokens.Size.avatarPick, circle = true)
                    Spacer(Modifier.width(Tokens.Space.m))
                    Text(c.displayName, color = Color.White, fontSize = Tokens.Font.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            PlateButton(stringResource(R.string.common_cancel), modifier = Modifier.fillMaxWidth()) { benchPick = false }
        }
    }

    // Второй шаг — цена словами: тридцать мегабайт по мешу платит и получатель.
    benchTarget?.let { c ->
        ActionSheet(
            stringResource(R.string.net_bench_confirm_title),
            stringResource(R.string.net_bench_confirm),
            onClose = { benchTarget = null },
        ) {
            PlateButton(stringResource(R.string.net_bench_start), modifier = Modifier.fillMaxWidth(), primary = true) {
                model.blastBench(c.key)
                benchTarget = null
            }
            PlateButton(stringResource(R.string.common_cancel), modifier = Modifier.fillMaxWidth()) { benchTarget = null }
        }
    }
}

/** Сколько пиров видно до «Все пиры». */
private const val PEERS_SHOWN = 2

/** Строка группы «Сети»: название и подпись под ним. */
@Composable
private fun NetRow(
    title: String,
    sub: String,
    divider: Boolean = true,
    inset: Dp = Tokens.Space.l,
    onClick: (() -> Unit)? = null,
) {
    CardRow(
        divider = divider,
        inset = inset,
        modifier = if (onClick != null) Modifier.clickable(role = Role.Button) { onClick() } else Modifier,
    ) {
        Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xxs)) {
            Text(title, color = Color.White, fontSize = Tokens.Font.name)
            Text(
                sub,
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
    }
}

/** Пир: адрес меша, направление, пинг. Пир из конфига без ключа — «подключается…». */
@Composable
private fun PeerRow(p: NetPeer, divider: Boolean) {
    CardRow(divider = divider, inset = Tokens.Space.l) {
        Column(Modifier.weight(1f)) {
            if (p.addr.isEmpty()) {
                Text(stringResource(R.string.net_peer_connecting), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
            } else {
                MonoText(p.addr, if (p.up) Color.White else Tokens.Colors.tertiary)
            }
            Text(
                stringResource(if (p.inbound) R.string.net_peer_inbound else R.string.net_peer_outbound),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
        Spacer(Modifier.width(Tokens.Space.m))
        val ms = p.latencyMs
        Text(
            if (ms <= 0) "—" else stringResource(R.string.net_ms_value, "%.0f".format(ms)),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.option,
        )
    }
}

/** Адрес меша моноширинным: группы цифр читаются по столбцам. */
@Composable
internal fun MonoText(text: String, color: Color, size: TextUnit = Tokens.Font.body) {
    Text(text, color = color, fontSize = size, fontFamily = FontFamily.Monospace, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/**
 * Плитка-показатель (макеты `screens/Network`, `misc/Flood`): крупное значение, единица
 * мельче рядом, подпись под ним. [bg] — ступень под плиткой: над панелью — panel, на ней — group.
 */
@Composable
internal fun StatTile(
    value: String,
    label: String,
    modifier: Modifier = Modifier,
    unit: String? = null,
    bg: Color = Tokens.Colors.panel,
    size: TextUnit = Tokens.Font.title,
) {
    Column(
        modifier.clip(RoundedCornerShape(Tokens.Radius.card)).background(bg)
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs, vertical = Tokens.Space.m),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs),
    ) {
        Row {
            Text(
                value,
                color = Color.White,
                fontSize = size,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
                style = LocalTextStyle.current.copy(fontFeatureSettings = "tnum"),
                modifier = Modifier.alignByBaseline(),
            )
            if (unit != null) {
                Text(
                    unit,
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    modifier = Modifier.alignByBaseline().padding(start = Tokens.Space.xxs),
                )
            }
        }
        Text(label, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.small, maxLines = 1)
    }
}

/** «12 КБ» → («12», «КБ»): число в плитке крупно, единица мельче. */
internal fun splitSize(s: String): Pair<String, String> {
    val clean = s.replace("⁨", "").replace("⁩", "").trim()
    val i = clean.indexOfLast { it == ' ' || it == ' ' || it == ' ' }
    return if (i < 0) clean to "" else clean.substring(0, i) to clean.substring(i + 1)
}

/** Дата смены адреса: эпоха — секунды Unix (шаг 7g); 0 — адреса ещё нет. */
internal fun rotatedOn(epoch: Int): String? = if (epoch <= 0) null else fmtSince(epoch.toLong())

/** Карточка-контейнер (тёмная поверхность, скругление) для разделов Сети. */
@Composable
internal fun SurfaceCard(
    onClick: (() -> Unit)? = null,
    /** Внешний боковой отступ. 0 — если контейнер экрана уже держит свой: иначе
     *  отступы складываются и карточка уезжает от общей вертикали. */
    outer: Dp = Tokens.Space.s,
    content: @Composable () -> Unit,
) {
    // Радиус и боковой отступ — общие карточные: карточки сети, хранилища и списка
    // чатов стоят по ОДНОЙ вертикали, иначе переход между экранами читается как
    // сдвиг всего макета.
    val base = Modifier.fillMaxWidth().padding(horizontal = outer)
        .clip(RoundedCornerShape(Tokens.Radius.card))
        .background(Tokens.Colors.surface)
    Box((if (onClick != null) base.clickable { onClick() } else base).padding(Tokens.Space.m)) { content() }
}

/** Короткий вид hex-адреса: первые 8 … последние 4. */
internal fun shortHex(h: String): String =
    if (h.length > 14) h.take(8) + "…" + h.takeLast(4) else h

internal fun shareText(ctx: android.content.Context, text: String) {
    val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(android.content.Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(android.content.Intent.createChooser(send, ctx.getString(R.string.net_share_chooser)))
}

/** Встроенный Compose-сканер QR-кода на CameraX. Вызывает [onResult] ровно один раз. */
