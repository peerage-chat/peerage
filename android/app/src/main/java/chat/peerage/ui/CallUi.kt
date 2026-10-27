@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

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
import chat.peerage.*
import chat.peerage.R

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
        Column(Modifier.padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m)) {
            Text(
                stringResource(if (inRoom) R.string.call_in_room_caps else R.string.call_in_room_presence_caps, total),
                color = HoloSecondary,
                fontSize = Tokens.Font.caption,
                modifier = Modifier.padding(start = Tokens.Space.xxs, bottom = Tokens.Space.s),
            )
            val all = if (inRoom) {
                // Своё состояние — из модели: раньше плитка «Вы» всегда показывала
                // включённый микрофон, даже когда человек себя заглушил.
                listOf(Triple("me", !model.muted, model.videoActive)) + members.map { Triple(it.peer, it.audio, it.video) }
            } else {
                members.map { Triple(it.peer, it.audio, it.video) }
            }
            all.chunked(2).forEach { row ->
                Row(
                    Modifier.fillMaxWidth().padding(bottom = Tokens.Space.s),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                ) {
                    row.forEach { (key, audio, video) ->
                        val isMe = key == "me"
                        val name = if (isMe) {
                            stringResource(R.string.common_you)
                        } else {
                            model.peerName(key)
                        }
                        val photo = if (isMe) model.selfAvatar else model.peerAvatar(key)
                        GridTile(name, audio, video, isMe, photo, Modifier.weight(1f)) { onFocus(key) }
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
    photo: ByteArray?,
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
            Modifier.fillMaxSize().padding(Tokens.Space.m),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(64.dp).clip(CircleShape)
                    .background(if (me) HoloAccent else Tokens.Colors.avatar),
                contentAlignment = Alignment.Center,
            ) {
                PhotoOr(photo, 64.dp) {
                    Text(
                        initialOf(name),
                        color = if (me) HoloAccentDark else Color.White,
                        fontWeight = FontWeight.Bold,
                        fontSize = Tokens.Font.initial(64.dp),
                    )
                }
            }
            Spacer(Modifier.height(Tokens.Space.m))
            Text(
                name,
                color = if (me) HoloAccent else Color.White,
                fontSize = Tokens.Font.body,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth(),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            )
        }
        // Статус микрофона/видео — правый верхний угол
        if (!audio || video) {
            Row(
                Modifier.align(Alignment.TopEnd).padding(Tokens.Space.s)
                    .clip(RoundedCornerShape(Tokens.Radius.badge)).background(Tokens.Colors.tileScrim)
                    .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.xxs),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xxs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (!audio) Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(Tokens.Size.iconXs))
                if (video) Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconXs))
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
    photo: ByteArray?,
    onDismiss: () -> Unit,
) {
    Box(Modifier.fillMaxSize().background(Tokens.Colors.overlay).clickable { onDismiss() }) {
        // Кнопка закрытия — правый верхний угол
        Box(
            Modifier.align(Alignment.TopEnd).padding(Tokens.Space.l).size(36.dp)
                .clip(CircleShape).background(Tokens.Colors.glass).clickable { onDismiss() },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                Icons.AutoMirrored.Filled.ArrowBack,
                stringResource(R.string.call_close),
                tint = Color.White,
                modifier = Modifier.size(Tokens.Size.iconS).then(
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
                PhotoOr(photo, 128.dp) {
                    Text(
                        initialOf(name),
                        color = if (me) HoloAccentDark else Color.White,
                        fontWeight = FontWeight.Bold,
                        fontSize = Tokens.Font.initial(128.dp),
                    )
                }
            }
            Spacer(Modifier.height(Tokens.Space.xl))
            Text(name, color = Color.White, fontSize = Tokens.Font.big, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(Tokens.Space.m))
            Row(
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (!audio) {
                    Row(
                        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.glassDanger)
                            .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.xs),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
                    ) {
                        Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(Tokens.Size.iconXs))
                        Text(stringResource(R.string.call_mic_off), color = WarnColor, fontSize = Tokens.Font.small)
                    }
                }
                if (video) {
                    Row(
                        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.glassDim)
                            .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.xs),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
                    ) {
                        Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconXs))
                        Text(stringResource(R.string.call_btn_video), color = HoloSecondary, fontSize = Tokens.Font.small)
                    }
                }
            }
        }
    }
}

@Composable
internal fun IncomingCallOverlay(model: AppModel, ic: IncomingRoomCall) {
    val name = model.peerName(ic.peer)
    // ПРЕСЕТ join'ера: в каком состоянии войдём (мик/камера). Дефолт — мик вкл, видео выкл.
    var micOn by remember { mutableStateOf(true) }
    var videoOn by remember { mutableStateOf(false) }
    // Кто уже в комнате помимо звонящего (групповой room-invite); в 1:1 пусто.
    val others = model.groupPresence[ic.roomId] ?: emptyList()
    // Оценка суммарного входящего «~»: видео-поток ≈30, аудио ≈2 МБ/мин (per call-ui.md).
    val trafficMb = (listOf(ic.video) + others.map { it.video })
        .sumOf { if (it) AppModel.TRAFFIC_VIDEO_MB_MIN else AppModel.TRAFFIC_AUDIO_MB_MIN }

    Box(Modifier.fillMaxSize().background(Tokens.Colors.callBg)) {
        Column(
            Modifier.fillMaxSize().padding(bottom = 56.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Spacer(Modifier.height(96.dp))
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    Modifier.size(Tokens.Size.avatarBig).clip(RoundedCornerShape(Tokens.Radius.avatar)).background(HoloAccent),
                    contentAlignment = Alignment.Center,
                ) {
                    PhotoOr(model.peerAvatar(ic.peer), Tokens.Size.avatarBig) {
                        Text(initialOf(name), color = HoloAccentDark, fontWeight = FontWeight.Bold, fontSize = Tokens.Font.initial(Tokens.Size.avatarBig))
                    }
                }
                Spacer(Modifier.height(Tokens.Space.xl))
                Text(name, color = Color.White, fontSize = Tokens.Font.big, fontWeight = FontWeight.Medium)
                val groupName = model.groups.firstOrNull { it.gid == ic.roomId }?.name
                Text(
                    if (groupName != null) {
                        stringResource(R.string.call_invite_group_subtitle, groupName)
                    } else {
                        stringResource(R.string.call_invite_subtitle)
                    },
                    color = HoloSecondary,
                    fontSize = Tokens.Font.label,
                    modifier = Modifier.padding(top = Tokens.Space.s),
                )
                // Кто уже в комнате: «Боря [и ещё N] в комнате» (мини-аватар). 1:1 — нет.
                if (others.isNotEmpty()) {
                    val firstName = model.peerName(others.first().peer)
                    val more = others.size - 1
                    Row(
                        Modifier.padding(top = Tokens.Space.m),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(22.dp).clip(RoundedCornerShape(Tokens.Radius.badge)).background(HoloOnline), contentAlignment = Alignment.Center) {
                            Text(initialOf(firstName), color = Tokens.Colors.onOnline, fontSize = Tokens.Font.caption, fontWeight = FontWeight.Bold)
                        }
                        Text(
                            if (more > 0) {
                                "$firstName " + pluralStringResource(R.plurals.call_others_in_room, more, more)
                            } else {
                                firstName
                            },
                            color = HoloSecondary,
                            fontSize = Tokens.Font.body,
                        )
                    }
                }
                // Принять, уже сидя в звонке, — значит выйти из него: говорим заранее,
                // а не после того, как старый звонок молча оборвался.
                if (model.currentRoom != null) {
                    Text(
                        stringResource(R.string.call_invite_will_leave),
                        color = WarnColor,
                        fontSize = Tokens.Font.body,
                        modifier = Modifier.padding(top = Tokens.Space.m),
                    )
                }
                // Ожидаемый суммарный входящий трафик.
                Spacer(Modifier.height(Tokens.Space.xxl))
                Text(stringResource(R.string.call_traffic_rate, trafficMb), color = Color.White, fontSize = Tokens.Font.huge, fontWeight = FontWeight.Bold)
                Text(
                    stringResource(R.string.call_invite_traffic_caption),
                    color = HoloSecondary,
                    fontSize = Tokens.Font.small,
                    modifier = Modifier.padding(top = Tokens.Space.xxs),
                )
            }
            // Низ: Отклонить(✕) · Микрофон(пресет) · Видео(пресет) · Принять(дверь).
            // Разрешения спрашиваем на «Принять», по пресетам: камеру — только если
            // входим с видео. Отказ = в звонок не входим (иначе сессия стартует без
            // потока и выглядит как зависший звонок).
            val runWithPerm = rememberPermissionRunner()
            Row(
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.l),
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
            .size(Tokens.Size.callButton).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = if (on) Color.White else Tokens.Colors.iconDangerMuted, modifier = Modifier.size(Tokens.Size.iconL)) }
}

/** Цветная кнопка-сквиркул звонка (отклонить/принять). */
@Composable
internal fun CallButton(icon: ImageVector, desc: String, color: Color, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.button)).background(color).size(Tokens.Size.callButton).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Color.White, modifier = Modifier.size(Tokens.Size.iconL)) }
}
