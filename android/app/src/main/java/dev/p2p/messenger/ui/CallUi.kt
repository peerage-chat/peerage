@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.Manifest
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Login
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

@Composable
internal fun RoomGrid(
    model: AppModel,
    inRoom: Boolean,
    presenceMembers: List<RoomMember>,
    onFocus: (String) -> Unit,
) {
    val members = if (inRoom) model.roomMembers else presenceMembers
    val total = if (inRoom) members.size + 1 else members.size
    Surface(color = HoloSurface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 10.dp, vertical = 10.dp)) {
            Text(
                stringResource(if (inRoom) R.string.call_in_room_caps else R.string.call_in_room_presence_caps, total),
                color = HoloSecondary,
                fontSize = 11.sp,
                modifier = Modifier.padding(start = 2.dp, bottom = 8.dp),
            )
            val all = if (inRoom) {
                listOf(Triple("me", true, false)) + members.map { Triple(it.peer, it.audio, it.video) }
            } else {
                members.map { Triple(it.peer, it.audio, it.video) }
            }
            all.chunked(2).forEach { row ->
                Row(
                    Modifier.fillMaxWidth().padding(bottom = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    row.forEach { (key, audio, video) ->
                        val isMe = key == "me"
                        val name = if (isMe) {
                            stringResource(R.string.common_you)
                        } else {
                            model.contacts.firstOrNull { it.key == key }?.displayName ?: (key.take(6) + "…")
                        }
                        GridTile(name, audio, video, isMe, Modifier.weight(1f)) { onFocus(key) }
                    }
                    if (row.size == 1) Spacer(Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
internal fun GridTile(
    name: String,
    audio: Boolean,
    video: Boolean,
    me: Boolean,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    Box(
        modifier
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .background(if (me) HoloAccentDark else Tokens.Colors.tileIdle)
            .clickable { onClick() }
            .aspectRatio(1f),
    ) {
        Column(
            Modifier.fillMaxSize().padding(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(64.dp).clip(CircleShape)
                    .background(if (me) HoloAccent else Tokens.Colors.avatar),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    name.take(1).uppercase(),
                    color = if (me) HoloAccentDark else Color.White,
                    fontWeight = FontWeight.Bold,
                    fontSize = 26.sp,
                )
            }
            Spacer(Modifier.height(10.dp))
            Text(
                name,
                color = if (me) HoloAccent else Color.White,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth(),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        }
        // Статус микрофона/видео — правый верхний угол
        if (!audio || video) {
            Row(
                Modifier.align(Alignment.TopEnd).padding(6.dp)
                    .clip(RoundedCornerShape(Tokens.Radius.badge)).background(Tokens.Colors.tileScrim)
                    .padding(horizontal = 5.dp, vertical = 3.dp),
                horizontalArrangement = Arrangement.spacedBy(3.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (!audio) Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(13.dp))
                if (video) Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(13.dp))
            }
        }
    }
}

@Composable
internal fun ParticipantFullscreen(
    name: String,
    audio: Boolean,
    video: Boolean,
    me: Boolean,
    onDismiss: () -> Unit,
) {
    Box(Modifier.fillMaxSize().background(Tokens.Colors.overlay).clickable { onDismiss() }) {
        // Кнопка закрытия — правый верхний угол
        Box(
            Modifier.align(Alignment.TopEnd).padding(16.dp).size(36.dp)
                .clip(CircleShape).background(Tokens.Colors.glass).clickable { onDismiss() },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                Icons.AutoMirrored.Filled.ArrowBack,
                stringResource(R.string.call_close),
                tint = Color.White,
                modifier = Modifier.size(18.dp).then(
                    Modifier.graphicsLayer { rotationZ = 90f },
                ),
            )
        }
        // Центральный контент
        Column(
            Modifier.fillMaxSize(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(128.dp).clip(RoundedCornerShape(Tokens.Radius.avatar))
                    .background(if (me) HoloAccent else Tokens.Colors.avatar)
                    .border(
                        3.dp,
                        if (me) HoloAccent.copy(alpha = 0.4f) else Tokens.Colors.tileBorder,
                        RoundedCornerShape(Tokens.Radius.avatar),
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    name.take(1).uppercase(),
                    color = if (me) HoloAccentDark else Color.White,
                    fontWeight = FontWeight.Bold,
                    fontSize = 52.sp,
                )
            }
            Spacer(Modifier.height(20.dp))
            Text(name, color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(10.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (!audio) {
                    Row(
                        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.glassDanger)
                            .padding(horizontal = 10.dp, vertical = 5.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(5.dp),
                    ) {
                        Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(14.dp))
                        Text(stringResource(R.string.call_mic_off), color = WarnColor, fontSize = 12.sp)
                    }
                }
                if (video) {
                    Row(
                        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.glassDim)
                            .padding(horizontal = 10.dp, vertical = 5.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(5.dp),
                    ) {
                        Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(14.dp))
                        Text(stringResource(R.string.call_btn_video), color = HoloSecondary, fontSize = 12.sp)
                    }
                }
            }
        }
    }
}

@Composable
internal fun IncomingCallOverlay(model: AppModel, ic: IncomingRoomCall) {
    val name = model.contacts.firstOrNull { it.key == ic.peer }?.name ?: stringResource(R.string.common_contact)
    // ПРЕСЕТ join'ера: в каком состоянии войдём (мик/камера). Дефолт — мик вкл, видео выкл.
    var micOn by remember { mutableStateOf(true) }
    var videoOn by remember { mutableStateOf(false) }
    // Кто уже в комнате помимо звонящего (групповой room-invite); в 1:1 пусто.
    val others = model.groupPresence[ic.roomId] ?: emptyList()
    // Оценка суммарного входящего «~»: видео-поток ≈30, аудио ≈2 МБ/мин (per call-ui.md).
    val trafficMb = (if (ic.video) 30 else 2) + others.map { if (it.video) 30 else 2 }.sum()

    Box(Modifier.fillMaxSize().background(Tokens.Colors.callBg)) {
        Column(
            Modifier.fillMaxSize().padding(bottom = 56.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Spacer(Modifier.height(96.dp))
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    Modifier.size(88.dp).clip(RoundedCornerShape(Tokens.Radius.avatar)).background(HoloAccent),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(name.take(1).uppercase(), color = HoloAccentDark, fontWeight = FontWeight.Bold, fontSize = 40.sp)
                }
                Spacer(Modifier.height(20.dp))
                Text(name, color = Color.White, fontSize = 26.sp, fontWeight = FontWeight.Medium)
                val groupName = model.groups.firstOrNull { it.gid == ic.roomId }?.name
                Text(
                    if (groupName != null) {
                        stringResource(R.string.call_invite_group_subtitle, groupName)
                    } else {
                        stringResource(R.string.call_invite_subtitle)
                    },
                    color = HoloSecondary,
                    fontSize = 15.sp,
                    modifier = Modifier.padding(top = 8.dp),
                )
                // Кто уже в комнате: «Боря [и ещё N] в комнате» (мини-аватар). 1:1 — нет.
                if (others.isNotEmpty()) {
                    val firstName = model.contacts.firstOrNull { it.key == others.first().peer }?.displayName
                        ?: (others.first().peer.take(6) + "…")
                    val more = others.size - 1
                    Row(
                        Modifier.padding(top = 12.dp),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(22.dp).clip(RoundedCornerShape(Tokens.Radius.badge)).background(HoloOnline), contentAlignment = Alignment.Center) {
                            Text(firstName.take(1).uppercase(), color = Tokens.Colors.onOnline, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                        }
                        Text(
                            if (more > 0) {
                                "$firstName " + pluralStringResource(R.plurals.call_others_in_room, more, more)
                            } else {
                                firstName
                            },
                            color = HoloSecondary,
                            fontSize = 13.sp,
                        )
                    }
                }
                // Ожидаемый суммарный входящий трафик.
                Spacer(Modifier.height(22.dp))
                Text(stringResource(R.string.call_traffic_rate, trafficMb), color = Color.White, fontSize = 28.sp, fontWeight = FontWeight.Bold)
                Text(
                    stringResource(R.string.call_invite_traffic_caption),
                    color = HoloSecondary,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(top = 2.dp),
                )
            }
            // Низ: Отклонить(✕) · Микрофон(пресет) · Видео(пресет) · Принять(дверь).
            // Разрешения спрашиваем на «Принять», по пресетам: камеру — только если
            // входим с видео. Отказ = в звонок не входим (иначе сессия стартует без
            // потока и выглядит как зависший звонок).
            val runWithPerm = rememberPermissionRunner()
            Row(
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                CallButton(Icons.Filled.Close, stringResource(R.string.call_invite_btn_decline), HoloError) { model.declineDmCall() }
                PresetButton(if (micOn) Icons.Filled.Mic else Icons.Filled.MicOff, micOn, stringResource(R.string.call_btn_mic)) { micOn = !micOn }
                PresetButton(if (videoOn) Icons.Filled.Videocam else Icons.Filled.VideocamOff, videoOn, stringResource(R.string.call_btn_video)) { videoOn = !videoOn }
                CallButton(Icons.AutoMirrored.Filled.Login, stringResource(R.string.call_invite_btn_join), HoloOnline) {
                    val perms = if (videoOn) {
                        arrayOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.CAMERA)
                    } else {
                        arrayOf(Manifest.permission.RECORD_AUDIO)
                    }
                    runWithPerm(perms, R.string.perm_denied_call) { model.acceptDmCall(micOn, videoOn) }
                }
            }
        }
    }
}

/** Тумблер пресета mic/video на входящем: вкл — нейтральный, выкл — перечёркнут+красный. */
@Composable
private fun PresetButton(icon: ImageVector, on: Boolean, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.button)).background(if (on) Tokens.Colors.btnNeutral else Tokens.Colors.btnDangerMuted)
            .size(56.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = if (on) Color.White else Tokens.Colors.iconDangerMuted, modifier = Modifier.size(24.dp)) }
}

/** Цветная кнопка-сквиркул звонка (отклонить/принять). */
@Composable
internal fun CallButton(icon: ImageVector, desc: String, color: Color, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.button)).background(color).size(56.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Color.White, modifier = Modifier.size(26.dp)) }
}
