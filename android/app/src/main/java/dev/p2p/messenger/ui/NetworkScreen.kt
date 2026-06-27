@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.BatteryStd
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.EmojiEmotions
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Create
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Forum
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material3.ExperimentalMaterial3Api
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.abs
import dev.p2p.messenger.*

@Composable
internal fun NetworkScreen(model: AppModel, onMyQr: () -> Unit) {
    val me = model.me ?: return
    val ctx = LocalContext.current
    // Офлайн-очередь = сообщения, ждущие появления получателя (статус "queued").
    val queued = model.messages.values.sumOf { l -> l.count { it.status == "queued" } }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // 1. Статус — ровно, в карточке.
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(10.dp).clip(CircleShape).background(HoloOnline))
                Spacer(Modifier.width(10.dp))
                Row {
                    Text("Подключено к ", color = Color.White, fontSize = 15.sp)
                    Text("mesh", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Bold)
                }
            }
        }

        // 2. Метрики — из снапшота движка (агрегаты считает движок).
        val ns = model.netStats
        val pingTxt = ns?.pingMs?.takeIf { it > 0 }?.let { "%.0f".format(it) } ?: "—"
        val trafficTxt = if (ns == null) "—" else "↓${ns.rxRate / 1024} ↑${ns.txRate / 1024}"
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            MetricCard("Пиры", "${ns?.peerCount ?: model.netPeers}", null, Modifier.weight(1f))
            MetricCard("Пинг", pingTxt, if (pingTxt == "—") null else "мс", Modifier.weight(1f))
            MetricCard("Трафик", trafficTxt, "КБ/с", Modifier.weight(1f))
        }

        // 3. Мой узел.
        FieldLabel("МОЙ УЗЕЛ")
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(me.address, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace,
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(8.dp))
                Icon(Icons.Filled.ContentCopy, "Скопировать адрес", tint = HoloAccent,
                    modifier = Modifier.size(20.dp).clickable { copyToClipboard(ctx, me.address, "Адрес") })
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            OutlineAction("Мой QR", Icons.Filled.QrCode2, Modifier.weight(1f), onMyQr)
            OutlineAction("Поделиться", Icons.Filled.Share, Modifier.weight(1f)) { shareText(ctx, me.pubkey) }
        }

        // 4. Режим участия — выгоды + цена.
        FieldLabel("РЕЖИМ УЧАСТИЯ")
        SurfaceCard {
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Share, null, tint = HoloOnline, modifier = Modifier.size(26.dp))
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text("Узел сети", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                        Text("Ретранслирую трафик других — помогаю сети",
                            color = HoloSecondary, fontSize = 12.sp)
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
                BenefitRow(Icons.Filled.Bolt, HoloOnline, "Приоритет доставки твоих сообщений")
                Spacer(Modifier.height(10.dp))
                BenefitRow(Icons.Filled.Schedule, HoloOnline, "Дольше хранится офлайн-очередь")
                Spacer(Modifier.height(10.dp))
                BenefitRow(Icons.Filled.BatteryStd, HoloSecondary, "Цена: выше расход батареи и трафика")
            }
        }

        // 5. Пиры — реальный список (направление/адрес/пинг с цветом).
        Row(verticalAlignment = Alignment.CenterVertically) {
            FieldLabel("ПИРЫ · ${ns?.peerCount ?: model.netPeers}")
            Spacer(Modifier.weight(1f))
        }
        SurfaceCard {
            val peers = ns?.peers ?: emptyList()
            if (peers.isEmpty()) {
                Text("Нет прямых пиров", color = HoloSecondary, fontSize = 13.sp,
                    modifier = Modifier.padding(vertical = 4.dp))
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
        SurfaceCard(onClick = { /* TODO: экран очереди */ }) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Schedule, null, tint = HoloAccent, modifier = Modifier.size(26.dp))
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text("Офлайн-очередь · $queued", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    Text("доставится, когда получатель появится", color = HoloSecondary, fontSize = 12.sp)
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            }
        }
    }
}

/** Карточка-контейнер (тёмная поверхность, скругление) для разделов Сети. */
@Composable
internal fun SurfaceCard(onClick: (() -> Unit)? = null, content: @Composable () -> Unit) {
    val base = Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(HoloSurface)
    Box((if (onClick != null) base.clickable { onClick() } else base).padding(14.dp)) { content() }
}

/** Метрика: подпись + крупное значение (+ опц. единица). */
@Composable
internal fun MetricCard(label: String, value: String, unit: String?, modifier: Modifier = Modifier) {
    Box(modifier.clip(RoundedCornerShape(14.dp)).background(HoloSurface).padding(14.dp)) {
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
        modifier.clip(RoundedCornerShape(12.dp)).border(1.dp, HoloBorder, RoundedCornerShape(12.dp))
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
        Text(shortHex(p.key), color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace,
            modifier = Modifier.weight(1f))
        val ms = p.latencyMs
        val col = when {
            ms <= 0 -> HoloSecondary
            ms < 60 -> HoloOnline
            ms < 150 -> WarnColor
            else -> HoloError
        }
        Text(if (ms <= 0) "—" else "%.0f мс".format(ms), color = col, fontSize = 13.sp)
    }
}

/** Короткий вид hex-адреса: первые 8 … последние 4. */
internal fun shortHex(h: String): String =
    if (h.length > 14) h.take(8) + "…" + h.takeLast(4) else h

internal fun copyToClipboard(ctx: android.content.Context, text: String, label: String) {
    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
    cm.setPrimaryClip(android.content.ClipData.newPlainText(label, text))
    if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
        android.widget.Toast.makeText(ctx, "Скопировано", android.widget.Toast.LENGTH_SHORT).show()
    }
}

internal fun shareText(ctx: android.content.Context, text: String) {
    val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(android.content.Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(android.content.Intent.createChooser(send, "Поделиться ключом"))
}

/** Встроенный Compose-сканер QR-кода на CameraX. Вызывает [onResult] ровно один раз. */

