package dev.p2p.messenger

import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView

/**
 * Экран активного звонка-комнаты (Discord-style сетка). Дизайн — docs/design/call-ui.md.
 *
 * Тайл = ОДИН компонент: показывает живое видео (`SurfaceView` + приёмник) если у
 * участника видео и есть свободный декодер, иначе аватар. Своё видео НЕ
 * декодируется (тайл «Вы» пока аватар; превью с камеры — следующий шаг). `key(peer)`
 * стабилизирует surface через перерисовки → нет тёрки декодеров. UI (chrome)
 * рисуется ПОВЕРХ неподвижной сетки, тап скрывает его.
 *
 * Active-speaker (зелёная рамка, свап декодеров с гистерезисом, пиннинг),
 * чип трафика и оверфлоу «+N» — отдельные шаги; на малом N не нужны.
 */
@Composable
fun RoomCallScreen(model: AppModel, peerKey: String) {
    val remotes = model.roomMembers
    var chrome by remember { mutableStateOf(true) }

    Box(
        Modifier.fillMaxSize().background(Color.Black).clickable { chrome = !chrome },
    ) {
        if (remotes.isEmpty()) {
            AloneTile(model, peerKey, chrome)
        } else {
            // тайлы: удалённые участники + ты (последним)
            val tiles = remotes.map { Tile(it.peer, false, it.audio, it.video) } +
                Tile("me", true, !model.muted, model.videoActive)
            ParticipantGrid(model, tiles, chrome)
        }

        if (chrome) {
            // верх-право: смена камеры + настройки (заглушки на первый заход)
            Row(
                Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ChromeChip(Icons.Filled.Cameraswitch, stringResource(R.string.call_btn_switch_camera)) {}
                ChromeChip(Icons.Filled.Settings, stringResource(R.string.call_btn_settings)) {}
            }
            // низ: панель управления (выше системной навигации)
            Box(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 18.dp)) {
                ControlBar(model, peerKey)
            }
        }
    }
}

private data class Tile(val id: String, val me: Boolean, val audio: Boolean, val video: Boolean)

@Composable
private fun ParticipantGrid(model: AppModel, tiles: List<Tile>, chrome: Boolean) {
    // Раскладка: 1-2 → стопкой (без горизонтального сплита); 3+ → по 2 в ряд,
    // одиночный последний занимает всю ширину. Оверфлоу >6 — на потом.
    val rows = if (tiles.size <= 2) tiles.map { listOf(it) } else tiles.chunked(2)
    Column(Modifier.fillMaxSize().padding(2.dp)) {
        rows.forEach { row ->
            Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                row.forEach { t ->
                    key(t.id) {
                        ParticipantTile(model, t, chrome, Modifier.weight(1f).fillMaxHeight().padding(2.dp))
                    }
                }
            }
        }
    }
}

@Composable
private fun ParticipantTile(model: AppModel, t: Tile, chrome: Boolean, modifier: Modifier) {
    val name = if (t.me) stringResource(R.string.call_you)
    else model.contacts.firstOrNull { it.key == t.id }?.name ?: (t.id.take(6) + "…")
    Box(
        modifier.clip(RoundedCornerShape(12.dp)).background(Color(0xFF1A1D21)),
        contentAlignment = Alignment.Center,
    ) {
        if (t.video && !t.me) {
            VideoSurface(model, t.id, Modifier.fillMaxSize())
        } else {
            Avatar(name, 64)
        }
        // голубая рамка у себя (скруглённая — по форме тайла)
        if (t.me) {
            Box(Modifier.matchParentSize().border(2.dp, HoloAccent, RoundedCornerShape(12.dp)))
        }
        if (chrome) {
            Row(
                Modifier.align(Alignment.BottomStart).padding(7.dp)
                    .clip(RoundedCornerShape(7.dp)).background(Color(0xAA000000))
                    .padding(horizontal = 7.dp, vertical = 3.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (!t.audio) Icon(Icons.Filled.MicOff, null, tint = Color(0xFFFF6B6B), modifier = Modifier.size(13.dp))
                Text(name, color = Color.White, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun AloneTile(model: AppModel, peerKey: String, chrome: Boolean) {
    val name = model.contacts.firstOrNull { it.key == peerKey }?.name ?: (peerKey.take(6) + "…")
    Box(Modifier.fillMaxSize()) {
        // Аватар — неподвижный базовый слой, чуть выше центра, чтобы текст лёг под ним.
        Box(Modifier.align(Alignment.Center).offset(y = (-32).dp)) { Avatar(name, 104) }
        if (chrome) {
            Column(
                Modifier.align(Alignment.Center).offset(y = 78.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(stringResource(R.string.call_alone_title), color = Color.White,
                    fontSize = 18.sp, fontWeight = FontWeight.Medium)
                Text(stringResource(R.string.call_alone_subtitle), color = HoloSecondary,
                    fontSize = 13.sp, modifier = Modifier.padding(top = 5.dp))
            }
        }
    }
}

/** Стабильный SurfaceView, привязанный к видео-потоку участника. */
@Composable
private fun VideoSurface(model: AppModel, peer: String, modifier: Modifier) {
    AndroidView(
        factory = { ctx ->
            SurfaceView(ctx).apply {
                holder.addCallback(object : SurfaceHolder.Callback {
                    override fun surfaceCreated(h: SurfaceHolder) = model.attachVideoReceiver(peer, h.surface)
                    override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, ht: Int) {}
                    override fun surfaceDestroyed(h: SurfaceHolder) = model.detachVideoReceiver(peer)
                })
            }
        },
        modifier = modifier,
    )
}

@Composable
private fun ControlBar(model: AppModel, peerKey: String) {
    Row(
        Modifier.clip(RoundedCornerShape(18.dp)).background(Color(0xBC141518))
            .padding(horizontal = 12.dp, vertical = 9.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CtlButton(Icons.Filled.PersonAdd, stringResource(R.string.call_btn_invite), Color(0xFF3A3D44)) {}
        CtlButton(
            if (model.muted) Icons.Filled.MicOff else Icons.Filled.Mic,
            stringResource(R.string.call_btn_mic),
            if (model.muted) Color(0xFF3A2A2C) else Color(0xFF3A3D44),
            tint = if (model.muted) Color(0xFFFF8A8A) else Color.White,
        ) { model.toggleMute() }
        CtlButton(
            if (model.videoActive) Icons.Filled.Videocam else Icons.Filled.VideocamOff,
            stringResource(R.string.call_btn_video),
            if (model.videoActive) Color(0xFF3A3D44) else Color(0xFF3A2A2C),
            tint = if (model.videoActive) Color.White else Color(0xFFFF8A8A),
        ) { model.toggleVideo() }
        CtlButton(Icons.AutoMirrored.Filled.Logout, stringResource(R.string.call_btn_leave), HoloError) { model.leaveRoom() }
    }
}

@Composable
private fun CtlButton(icon: ImageVector, desc: String, bg: Color, tint: Color = Color.White, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(12.dp)).background(bg).size(width = 52.dp, height = 40.dp)
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(22.dp)) }
}

@Composable
private fun ChromeChip(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(10.dp)).background(Color(0xB8141518)).size(38.dp)
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Color(0xFFE6E7EA), modifier = Modifier.size(20.dp)) }
}

@Composable
private fun Avatar(name: String, size: Int) {
    Box(
        Modifier.size(size.dp).clip(CircleShape).background(Color(0xFF3A3F45)),
        contentAlignment = Alignment.Center,
    ) {
        Text(name.take(1).uppercase(), color = Color.White,
            fontWeight = FontWeight.Bold, fontSize = (size / 2.4f).sp)
    }
}
