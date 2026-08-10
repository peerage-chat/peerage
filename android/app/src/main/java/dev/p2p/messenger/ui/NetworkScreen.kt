@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.os.Build
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
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Share
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
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

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
                Spacer(Modifier.width(10.dp))
                if (model.offline) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.notif_offline_title), color = Color.White, fontSize = 15.sp)
                        Text(
                            stringResource(R.string.notif_offline_body),
                            color = HoloSecondary,
                            fontSize = 12.sp,
                            modifier = Modifier.padding(top = 2.dp),
                        )
                    }
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(R.string.flood_back_online),
                        color = HoloAccent,
                        fontSize = 13.sp,
                        modifier = Modifier.clickable { model.goOnline() },
                    )
                } else {
                    Row {
                        // Пробел ставит РАЗМЕТКА, а не хвост строки в ресурсе: XML
                        // обрезает концевые пробелы, и получалось «Connected tomesh».
                        Text(
                            stringResource(R.string.net_connected_to).trim(),
                            color = Color.White,
                            fontSize = 15.sp,
                        )
                        Spacer(Modifier.width(Tokens.Space.xs))
                        Text("mesh", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Bold)
                    }
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
                    fontSize = 13.sp,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                Icon(
                    Icons.Filled.ContentCopy,
                    stringResource(R.string.net_copy_address),
                    tint = HoloAccent,
                    modifier = Modifier.size(20.dp).clickable { copyToClipboard(ctx, me.address, ctx.getString(R.string.net_address)) },
                )
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
                Icon(Icons.Filled.Autorenew, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.net_rotate_row), color = Color.White, fontSize = 15.sp)
                    Text(
                        if (model.transportEpoch > 0) {
                            stringResource(R.string.net_epoch, model.transportEpoch)
                        } else {
                            stringResource(R.string.net_rotate_row_desc)
                        },
                        color = HoloSecondary,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(18.dp))
            }
        }

        // 4. Режим участия — выгоды + цена.
        SectionTitle(stringResource(R.string.net_mode))
        SurfaceCard {
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Share, null, tint = HoloOnline, modifier = Modifier.size(26.dp))
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.net_node_mode), color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                        Text(
                            stringResource(R.string.net_node_mode_desc),
                            color = HoloSecondary,
                            fontSize = 12.sp,
                        )
                    }
                    Spacer(Modifier.width(8.dp))
                    Switch(
                        checked = model.relayEnabled,
                        onCheckedChange = { model.relayEnabled = it },
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = Color.White,
                            checkedTrackColor = HoloAccent,
                            uncheckedThumbColor = HoloSecondary,
                            uncheckedTrackColor = HoloBg,
                        ),
                    )
                }
                HorizontalDivider(color = HoloBorder, modifier = Modifier.padding(vertical = 12.dp))
                BenefitRow(Icons.Filled.Bolt, HoloOnline, stringResource(R.string.net_benefit_priority))
                Spacer(Modifier.height(10.dp))
                BenefitRow(Icons.Filled.Schedule, HoloOnline, stringResource(R.string.net_benefit_queue))
                Spacer(Modifier.height(10.dp))
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
                    fontSize = 13.sp,
                    modifier = Modifier.padding(vertical = 4.dp),
                )
            } else {
                Column {
                    peers.forEachIndexed { i, p ->
                        PeerRow(p)
                        if (i < peers.lastIndex) HorizontalDivider(color = HoloBorder, thickness = 0.5.dp)
                    }
                }
            }
        }

        // 6. Офлайн-очередь.
        SurfaceCard(onClick = onOpenOutbox) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Schedule, null, tint = HoloAccent, modifier = Modifier.size(26.dp))
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.net_offline_queue, queued), color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    Text(stringResource(R.string.net_offline_queue_desc), color = HoloSecondary, fontSize = 12.sp)
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            }
        }
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
    Box((if (onClick != null) base.clickable { onClick() } else base).padding(14.dp)) { content() }
}

/** Метрика: подпись + крупное значение (+ опц. единица). */
@Composable
internal fun MetricCard(label: String, value: String, unit: String?, modifier: Modifier = Modifier) {
    Box(modifier.clip(RoundedCornerShape(Tokens.Radius.card)).background(Tokens.Colors.surface).padding(14.dp)) {
        Column {
            Text(label, color = HoloSecondary, fontSize = 11.sp)
            Spacer(Modifier.height(6.dp))
            Row(verticalAlignment = Alignment.Bottom) {
                Text(value, color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.Medium)
                if (unit != null) {
                    Spacer(Modifier.width(3.dp))
                    Text(unit, color = HoloSecondary, fontSize = 11.sp, modifier = Modifier.padding(bottom = 3.dp))
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
            .clickable { onClick() }.padding(vertical = 12.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text(text, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

/** Строка «выгода/цена» режима участия. */
@Composable
internal fun BenefitRow(icon: ImageVector, tint: Color, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, color = if (tint == HoloSecondary) HoloSecondary else Color.White, fontSize = 13.sp)
    }
}

/** Строка пира: направление (вход/исход) · короткий адрес · пинг с цветом. */
@Composable
internal fun PeerRow(p: NetPeer) {
    Row(Modifier.fillMaxWidth().padding(vertical = 7.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(if (p.inbound) "↙" else "↗", color = HoloSecondary, fontSize = 15.sp)
        Spacer(Modifier.width(10.dp))
        Text(
            shortHex(p.key),
            color = Color.White,
            fontSize = 13.sp,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.weight(1f),
        )
        val ms = p.latencyMs
        val col = when {
            ms <= 0 -> HoloSecondary
            ms < 60 -> HoloOnline
            ms < 150 -> WarnColor
            else -> HoloError
        }
        Text(if (ms <= 0) "—" else stringResource(R.string.net_ms_value, "%.0f".format(ms)), color = col, fontSize = 13.sp)
    }
}

/** Короткий вид hex-адреса: первые 8 … последние 4. */
internal fun shortHex(h: String): String =
    if (h.length > 14) h.take(8) + "…" + h.takeLast(4) else h

internal fun copyToClipboard(ctx: android.content.Context, text: String, label: String) {
    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
    cm.setPrimaryClip(android.content.ClipData.newPlainText(label, text))
    if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
        android.widget.Toast.makeText(ctx, ctx.getString(R.string.common_copied), android.widget.Toast.LENGTH_SHORT).show()
    }
}

internal fun shareText(ctx: android.content.Context, text: String) {
    val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(android.content.Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(android.content.Intent.createChooser(send, ctx.getString(R.string.net_share_chooser)))
}

/** Встроенный Compose-сканер QR-кода на CameraX. Вызывает [onResult] ровно один раз. */
