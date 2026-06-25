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
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView

/**
 * Экран активного DM-звонка (комнатная модель), 1:1: один удалённый участник на
 * весь экран. Если он с видео — живой [SurfaceView]; без видео — аватар; если его
 * нет в комнате — «Вы одни в комнате» (или «Вызов…», пока никто ещё не входил).
 *
 * ВАЖНО — стабильность декодера: ровно ОДНА [AndroidView] в стабильной позиции
 * (не в пересобираемой сетке), поэтому surfaceCreated/Destroyed срабатывают только
 * на реальных переходах присутствие↔отсутствие, а не на каждой перерисовке. Раньше
 * сетка с тайлами пересоздавала сёрфейсы → тёрка аппаратных декодеров → залипание
 * всего звонка. Сетку на N>1 (групповые) сделаем отдельно через TextureView.
 */
@Composable
fun RoomCallScreen(model: AppModel, peerKey: String) {
    val remote = model.roomMembers.firstOrNull { it.peer == peerKey }
    val name = model.contacts.firstOrNull { it.key == peerKey }?.name ?: (peerKey.take(6) + "…")
    var everConnected by remember { mutableStateOf(false) }
    if (remote != null) everConnected = true

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        when {
            remote != null && remote.video -> VideoSurface(model, peerKey, Modifier.fillMaxSize())
            remote != null -> CenterAvatar(name, 128, sub = if (!remote.audio) "Микрофон выключен" else "В звонке")
            else -> CenterAvatar(name, 112, sub = if (everConnected) "Вы одни в комнате" else "Вызов…")
        }

        // Имя сверху
        Text(
            name, color = Color.White, fontSize = 20.sp, fontWeight = FontWeight.Medium,
            modifier = Modifier.align(Alignment.TopStart).padding(16.dp)
                .clip(RoundedCornerShape(8.dp)).background(Color(0x88000000))
                .padding(horizontal = 12.dp, vertical = 6.dp),
        )

        // Управление снизу: mute + завершить
        Row(
            Modifier.align(Alignment.BottomCenter).padding(bottom = 48.dp),
            horizontalArrangement = Arrangement.spacedBy(28.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CallControl(
                if (model.muted) Icons.Filled.MicOff else Icons.Filled.Mic,
                if (model.muted) "Включить микрофон" else "Выключить микрофон",
                if (model.muted) Color(0xFF555555) else Color(0xFF2A2F35),
            ) { model.toggleMute() }
            CallControl(Icons.Filled.CallEnd, "Завершить", HoloError) { model.leaveRoom() }
        }
    }
}

/** Стабильный SurfaceView, привязанный к видео-потоку участника (один на экран). */
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
private fun CenterAvatar(name: String, size: Int, sub: String) {
    Column(
        Modifier.fillMaxSize(),
        verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier.size(size.dp).clip(CircleShape).background(Color(0xFF3A3F45))
                .border(3.dp, Color(0xFF4A4F55), CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Text(name.take(1).uppercase(), color = Color.White,
                fontWeight = FontWeight.Bold, fontSize = (size / 2.4f).sp)
        }
        Spacer(Modifier.height(20.dp))
        Text(name, color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.height(8.dp))
        Text(sub, color = HoloSecondary, fontSize = 15.sp)
    }
}

@Composable
private fun CallControl(icon: ImageVector, desc: String, bg: Color, onClick: () -> Unit) {
    Box(
        Modifier.clip(CircleShape).background(bg).size(60.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Color.White, modifier = Modifier.size(26.dp)) }
}
