@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Autorenew
import androidx.compose.material.icons.filled.BatteryStd
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.NorthEast
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.SouthWest
import androidx.compose.material.icons.filled.Speed
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

@Composable
internal fun NetworkScreen(
    model: AppModel,
    onMyQr: () -> Unit,
    onOpenOutbox: () -> Unit,
    onFlood: () -> Unit = {},
    onRotate: () -> Unit = {},
) {
    val me = model.me ?: return
    val ctx = LocalContext.current
    var benchPick by remember { mutableStateOf(false) }
    var benchTarget by remember { mutableStateOf<Contact?>(null) }
    // Офлайн-очередь = сообщения, ждущие появления получателя (статус "queued").
    val queued = model.messages.values.sumOf { l -> l.count { it.status == "queued" } }

    // Отступ по бокам даёт сама карточка (как на всех экранах), а не контейнер:
    // иначе карточки этого экрана стояли бы по другой вертикали, чем везде.
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.s, bottom = Tokens.Space.xxl),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        // 0. Поток трафика — только когда он есть. Выше статуса: пока идёт флуд,
        // это самое важное на экране.
        FloodCard(model, onFlood)

        // 1. Статус — ровно, в карточке.
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // Цвет и текст — по фактическому состоянию. «Подключено» при
                // разорванных соединениях было бы прямой ложью на главном индикаторе.
                Box(
                    Modifier.size(10.dp).clip(CircleShape)
                        .background(if (model.offline) HoloWarn else HoloOnline),
                )
                Spacer(Modifier.width(Tokens.Space.m))
                if (model.offline) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.notif_offline_title), color = Color.White, fontSize = Tokens.Font.label)
                        Text(
                            stringResource(R.string.notif_offline_body),
                            color = HoloSecondary,
                            fontSize = Tokens.Font.small,
                            modifier = Modifier.padding(top = Tokens.Space.xxs),
                        )
                    }
                    Spacer(Modifier.width(Tokens.Space.s))
                    TextButton(onClick = { model.goOnline() }) {
                        Text(stringResource(R.string.flood_back_online), color = HoloAccent, fontSize = Tokens.Font.body)
                    }
                } else {
                    // Одной строкой ресурса: склейка «Подключено к » + "mesh" держала
                    // английское слово прямо в коде и не переводилась.
                    Text(stringResource(R.string.net_connected), color = Color.White, fontSize = Tokens.Font.label)
                }
            }
        }

        // 2. Метрики — из снапшота движка (агрегаты считает движок).
        val ns = model.netStats
        val pingTxt = ns?.pingMs?.takeIf { it > 0 }?.let { "%.0f".format(it) } ?: "—"
        val trafficTxt = if (ns == null) "—" else "↓${ns.rxRate / 1024} ↑${ns.txRate / 1024}"
        Row(
            Modifier.padding(horizontal = Tokens.Space.s),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            MetricCard(stringResource(R.string.net_metric_peers), "${ns?.peerCount ?: model.netPeers}", null, Modifier.weight(1f))
            MetricCard(stringResource(R.string.net_metric_ping), pingTxt, if (pingTxt == "—") null else stringResource(R.string.net_unit_ms), Modifier.weight(1f))
            MetricCard(stringResource(R.string.net_metric_traffic), trafficTxt, stringResource(R.string.net_unit_kbs), Modifier.weight(1f))
        }

        // 3. Мой узел.
        SectionTitle(stringResource(R.string.net_my_node))
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    me.address,
                    color = Color.White,
                    fontSize = Tokens.Font.body,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                // Плашкой BarIcon: голая иконка 20dp была мишенью меньше пальца.
                BarIcon(Icons.Filled.ContentCopy, stringResource(R.string.net_copy_address), tint = HoloAccent) {
                    copyToClipboard(ctx, me.address, ctx.getString(R.string.net_address))
                }
            }
        }
        Row(
            Modifier.padding(horizontal = Tokens.Space.s),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            OutlineAction(stringResource(R.string.net_my_qr), Icons.Filled.QrCode2, Modifier.weight(1f), onMyQr)
            OutlineAction(stringResource(R.string.net_share), Icons.Filled.Share, Modifier.weight(1f)) { shareText(ctx, model.myContactCode()) }
        }
        // Смена адреса живёт рядом с самим адресом — тут её и ищут.
        SurfaceCard(onClick = onRotate) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Autorenew, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconM))
                Spacer(Modifier.width(Tokens.Space.m))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.net_rotate_row), color = Color.White, fontSize = Tokens.Font.label)
                    Text(
                        if (model.transportEpoch > 0) {
                            stringResource(R.string.net_epoch, model.transportEpoch)
                        } else {
                            stringResource(R.string.net_rotate_row_desc)
                        },
                        color = HoloSecondary,
                        fontSize = Tokens.Font.small,
                        modifier = Modifier.padding(top = Tokens.Space.xxs),
                    )
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }

        // 4. Режим участия. Здесь был переключатель «Узел сети» с обещанными
        // выгодами — он не был подключён ни к чему: узел Yggdrasil пересылает чужой
        // трафик всегда, а приоритета и длинной очереди нет. Выключатель, который
        // ничего не выключает, хуже его отсутствия — человек думает, что сберёг
        // батарею. Пока режима нет в движке, экран говорит как есть.
        SectionTitle(stringResource(R.string.net_mode))
        SurfaceCard {
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Share, null, tint = HoloOnline, modifier = Modifier.size(Tokens.Size.iconL))
                    Spacer(Modifier.width(Tokens.Space.m))
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.net_node_mode), color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
                        Text(
                            stringResource(R.string.net_node_mode_desc),
                            color = HoloSecondary,
                            fontSize = Tokens.Font.small,
                        )
                    }
                }
                HorizontalDivider(color = Tokens.Colors.divider, modifier = Modifier.padding(vertical = Tokens.Space.m))
                BenefitRow(Icons.Filled.BatteryStd, HoloSecondary, stringResource(R.string.net_benefit_cost))
            }
        }

        // 5. Пиры — реальный список (направление/адрес/пинг с цветом).
        Row(verticalAlignment = Alignment.CenterVertically) {
            SectionTitle(stringResource(R.string.net_peers_label, ns?.peerCount ?: model.netPeers))
            Spacer(Modifier.weight(1f))
        }
        SurfaceCard {
            val peers = ns?.peers ?: emptyList()
            if (peers.isEmpty()) {
                Text(
                    stringResource(R.string.net_no_peers),
                    color = HoloSecondary,
                    fontSize = Tokens.Font.body,
                    modifier = Modifier.padding(vertical = Tokens.Space.xs),
                )
            } else {
                Column {
                    peers.forEachIndexed { i, p ->
                        PeerRow(p)
                        if (i < peers.lastIndex) HorizontalDivider(color = Tokens.Colors.divider, thickness = 0.5.dp)
                    }
                }
            }
        }

        // 6. Диагностика. Замер жил кнопкой в профиле КАЖДОГО контакта — один тап
        // отправлял человеку ~30 МБ, то есть флуд с парадного входа. Инструмент
        // сам по себе нужен (меш без цифр не отладить), поэтому он переехал в
        // диагностику: контакт выбирают явно, объём назван до запуска.
        SectionTitle(stringResource(R.string.net_diagnostics))
        SurfaceCard(onClick = { benchPick = true }) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Speed, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconL))
                Spacer(Modifier.width(Tokens.Space.m))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.net_bench_row), color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
                    Text(
                        model.benchResult.ifEmpty { stringResource(R.string.net_bench_row_desc) },
                        color = HoloSecondary,
                        fontSize = Tokens.Font.small,
                    )
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }

        // 7. Офлайн-очередь.
        SurfaceCard(onClick = onOpenOutbox) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Schedule, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconL))
                Spacer(Modifier.width(Tokens.Space.m))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.net_offline_queue, queued), color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
                    Text(stringResource(R.string.net_offline_queue_desc), color = HoloSecondary, fontSize = Tokens.Font.small)
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }
    }

    // Выбор пира: только те, кто сейчас на связи. Оффлайновому замер всё равно
    // осядет в очередь и уедет к нему потом — это уже не диагностика, а сюрприз.
    if (benchPick) {
        val online = model.contacts.filter { model.online[it.key] == true }
        AlertDialog(
            onDismissRequest = { benchPick = false },
            title = { Text(stringResource(R.string.net_bench_row)) },
            text = {
                if (online.isEmpty()) {
                    Text(stringResource(R.string.net_bench_nobody), color = HoloSecondary)
                } else {
                    Column(Modifier.verticalScroll(rememberScrollState())) {
                        online.forEach { c ->
                            Row(
                                Modifier.fillMaxWidth().clickable {
                                    benchPick = false
                                    benchTarget = c
                                }.padding(vertical = Tokens.Space.m),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                SquareAvatar(c.displayName, size = 32.dp, circle = true)
                                Spacer(Modifier.width(Tokens.Space.m))
                                Text(c.displayName, color = Color.White, fontSize = Tokens.Font.label)
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { benchPick = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }

    // Второй шаг — цена вопроса словами. Тридцать мегабайт по мешу платит и
    // получатель: его трафиком и его батареей.
    benchTarget?.let { c ->
        AlertDialog(
            onDismissRequest = { benchTarget = null },
            title = { Text(stringResource(R.string.net_bench_confirm_title)) },
            text = { Text(stringResource(R.string.net_bench_confirm, c.displayName)) },
            confirmButton = {
                TextButton(onClick = {
                    model.blastBench(c.key)
                    benchTarget = null
                }) { Text(stringResource(R.string.net_bench_start), color = HoloAccent) }
            },
            dismissButton = {
                TextButton(onClick = { benchTarget = null }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}

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

/** Метрика: подпись + крупное значение (+ опц. единица). */
@Composable
internal fun MetricCard(label: String, value: String, unit: String?, modifier: Modifier = Modifier) {
    Box(modifier.clip(RoundedCornerShape(Tokens.Radius.card)).background(Tokens.Colors.surface).padding(Tokens.Space.m)) {
        Column {
            Text(label, color = HoloSecondary, fontSize = Tokens.Font.caption)
            Spacer(Modifier.height(Tokens.Space.s))
            Row(verticalAlignment = Alignment.Bottom) {
                Text(value, color = Color.White, fontSize = Tokens.Font.title, fontWeight = FontWeight.Medium)
                if (unit != null) {
                    Spacer(Modifier.width(Tokens.Space.xxs))
                    Text(unit, color = HoloSecondary, fontSize = Tokens.Font.caption, modifier = Modifier.padding(bottom = Tokens.Space.xxs))
                }
            }
        }
    }
}

/** Кнопка-контур с иконкой (Мой QR / Поделиться). */
@Composable
internal fun OutlineAction(text: String, icon: ImageVector, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Row(
        modifier.clip(RoundedCornerShape(Tokens.Radius.button)).border(1.dp, HoloBorder, RoundedCornerShape(Tokens.Radius.button))
            .clickable { onClick() }.padding(vertical = Tokens.Space.m),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconS))
        Spacer(Modifier.width(Tokens.Space.s))
        Text(text, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
    }
}

/** Строка «выгода/цена» режима участия. */
@Composable
internal fun BenefitRow(icon: ImageVector, tint: Color, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(Tokens.Size.iconS))
        Spacer(Modifier.width(Tokens.Space.m))
        Text(text, color = if (tint == HoloSecondary) HoloSecondary else Color.White, fontSize = Tokens.Font.body)
    }
}

/** Строка пира: направление (вход/исход) · короткий адрес · пинг с цветом. */
@Composable
internal fun PeerRow(p: NetPeer) {
    Row(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.s), verticalAlignment = Alignment.CenterVertically) {
        // Направление — иконкой с подписью: стрелки-глифы TalkBack читал как
        // «стрелка влево вниз», а шрифт мог их и не иметь.
        Icon(
            if (p.inbound) Icons.Filled.SouthWest else Icons.Filled.NorthEast,
            stringResource(if (p.inbound) R.string.net_peer_inbound else R.string.net_peer_outbound),
            tint = HoloSecondary,
            modifier = Modifier.size(Tokens.Size.iconS),
        )
        Spacer(Modifier.width(Tokens.Space.m))
        // Пир из конфига, с которым соединение ещё не поднялось: ключа у него нет,
        // и строка была пустой — будто сломанная запись.
        Text(
            if (p.key.isEmpty()) stringResource(R.string.net_peer_connecting) else shortHex(p.key),
            color = if (p.up && p.key.isNotEmpty()) Color.White else HoloSecondary,
            fontSize = Tokens.Font.body,
            fontFamily = if (p.key.isEmpty()) null else FontFamily.Monospace,
            modifier = Modifier.weight(1f),
        )
        val ms = p.latencyMs
        val col = when {
            ms <= 0 -> HoloSecondary
            ms < 60 -> HoloOnline
            ms < 150 -> WarnColor
            else -> HoloError
        }
        Text(if (ms <= 0) "—" else stringResource(R.string.net_ms_value, "%.0f".format(ms)), color = col, fontSize = Tokens.Font.body)
    }
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
