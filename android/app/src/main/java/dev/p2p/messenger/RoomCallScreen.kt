package dev.p2p.messenger

import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.CloseFullscreen
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.PhoneInTalk
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.Icon
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.delay

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
    var showSettings by remember { mutableStateOf(false) }
    var focused by remember { mutableStateOf<String?>(null) }

    // тайлы: удалённые участники + ты (последним)
    val tiles = remotes.map { Tile(it.peer, false, it.audio, it.video) } +
        Tile("me", true, !model.muted, model.videoActive)
    // Фуллскрин-фокус снимаем, если участник исчез.
    LaunchedEffect(remotes.size) {
        if (focused != null && focused != "me" && remotes.none { it.peer == focused }) focused = null
    }
    // Авто-скрытие chrome: через паузу UI прячется сам (видео/аватар остаются).
    LaunchedEffect(chrome, showSettings) {
        if (chrome && !showSettings) {
            delay(4000)
            chrome = false
        }
    }

    val foc = focused?.let { f -> tiles.firstOrNull { it.id == f } }
    Box(Modifier.fillMaxSize().background(Tokens.Colors.callBg)) {
        when {
            // Фуллскрин участника (даблтап тайла): видео cover на весь экран.
            foc != null -> Box(
                Modifier.fillMaxSize().pointerInput(foc.id) {
                    detectTapGestures(onTap = { chrome = !chrome }, onDoubleTap = { focused = null })
                },
                contentAlignment = Alignment.Center,
            ) {
                when {
                    foc.video && foc.me -> SelfPreviewSurface(model, Modifier.fillMaxSize())
                    foc.video -> VideoSurface(model, foc.id, Modifier.fillMaxSize())
                    else -> Avatar(tileName(model, foc), 104)
                }
            }
            remotes.isEmpty() -> Box(Modifier.fillMaxSize().clickable { chrome = !chrome }) {
                AloneTile(model, peerKey, chrome)
            }
            else -> ParticipantGrid(
                model,
                tiles,
                chrome,
                onTap = { chrome = !chrome },
                onFocus = { focused = it },
            )
        }

        if (chrome) {
            if (foc != null) {
                // фуллскрин: [имя-пилюля · чип трафика] слева-сверху, «свернуть» справа-сверху
                Row(
                    Modifier.align(Alignment.TopStart).statusBarsPadding().padding(10.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    NamePill(tileName(model, foc))
                    if (!foc.me) TrafficChip(if (foc.video) 30 else 2)
                }
                Box(Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(10.dp)) {
                    ChromeChip(Icons.Filled.CloseFullscreen, stringResource(R.string.call_minimize)) { focused = null }
                }
            } else {
                // сетка: чип трафика слева-сверху, смена камеры + настройки справа-сверху
                val incoming = remotes.map {
                    if (it.video) {
                        30
                    } else if (it.audio) {
                        2
                    } else {
                        0
                    }
                }.sum()
                if (incoming > 0) {
                    Box(Modifier.align(Alignment.TopStart).statusBarsPadding().padding(10.dp)) {
                        TrafficChip(incoming)
                    }
                }
                Row(
                    Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(10.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (model.videoActive) {
                        ChromeChip(Icons.Filled.Cameraswitch, stringResource(R.string.call_btn_switch_camera)) {
                            model.switchCamera()
                        }
                    }
                    ChromeChip(Icons.Filled.Settings, stringResource(R.string.call_btn_settings)) { showSettings = true }
                }
            }
            // низ: панель управления (выше системной навигации)
            Box(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 18.dp)) {
                ControlBar(model, peerKey)
            }
        }
        if (showSettings) CallSettings(model) { showSettings = false }
        if (model.showVideoShare) ShareVideoScreen(model)
    }
}

/** Имя тайла: «Вы» для себя, иначе имя контакта / короткий ключ. */
@Composable
private fun tileName(model: AppModel, t: Tile): String =
    if (t.me) {
        stringResource(R.string.call_you)
    } else {
        model.contacts.firstOrNull { it.key == t.id }?.name ?: (t.id.take(6) + "…")
    }

/** Настройки звонка (кнопка ⚙️): вывод звука (динамик), качество видео. Оверлей
 *  поверх звонка; тап по фону не закрывает (только крестик). */
@Composable
private fun CallSettings(model: AppModel, onClose: () -> Unit) {
    Column(
        Modifier.fillMaxSize().background(Tokens.Colors.overlay).clickable(enabled = false) {}
            .statusBarsPadding().navigationBarsPadding().padding(horizontal = 18.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.call_settings_title),
                color = Color.White,
                fontSize = 19.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.weight(1f),
            )
            Box(
                Modifier.clip(CircleShape).background(Color(0x33FFFFFF)).size(36.dp).clickable { onClose() },
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Filled.Close, stringResource(R.string.call_close), tint = Color.White, modifier = Modifier.size(18.dp)) }
        }

        Text(stringResource(R.string.call_audio_output), color = HoloSecondary, fontSize = 11.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.height(6.dp))
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { model.toggleSpeaker() }
                .background(Tokens.Colors.tile).padding(horizontal = 14.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                if (model.speakerOn) Icons.Filled.VolumeUp else Icons.Filled.PhoneInTalk,
                null,
                tint = if (model.speakerOn) HoloAccent else HoloSecondary,
                modifier = Modifier.size(22.dp),
            )
            Spacer(Modifier.width(14.dp))
            Text(stringResource(R.string.call_speaker), color = Color.White, fontSize = 15.sp, modifier = Modifier.weight(1f))
            Switch(
                checked = model.speakerOn,
                onCheckedChange = { model.toggleSpeaker() },
                colors = SwitchDefaults.colors(
                    checkedThumbColor = Color.White,
                    checkedTrackColor = HoloAccent,
                    uncheckedThumbColor = HoloSecondary,
                    uncheckedTrackColor = Tokens.Colors.tile,
                ),
            )
        }

        Spacer(Modifier.height(22.dp))
        Text(stringResource(R.string.settings_video_quality), color = HoloSecondary, fontSize = 11.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.height(6.dp))
        VideoQuality.presets.forEach { preset ->
            val selected = model.videoQuality == preset
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp))
                    .clickable { model.changeVideoQuality(preset) }
                    .background(if (selected) Tokens.Colors.accentMuted else Tokens.Colors.tile)
                    .padding(horizontal = 14.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                RadioButton(
                    selected = selected,
                    onClick = { model.changeVideoQuality(preset) },
                    colors = RadioButtonDefaults.colors(selectedColor = HoloAccent, unselectedColor = HoloSecondary),
                )
                Text(preset.label, color = if (selected) HoloAccent else Color.White, fontSize = 15.sp)
            }
            Spacer(Modifier.height(6.dp))
        }
        Text(stringResource(R.string.settings_video_apply), color = HoloSecondary, fontSize = 11.sp, modifier = Modifier.padding(top = 4.dp))
    }
}

private data class Tile(val id: String, val me: Boolean, val audio: Boolean, val video: Boolean)

@Composable
private fun ParticipantGrid(
    model: AppModel,
    tiles: List<Tile>,
    chrome: Boolean,
    onTap: () -> Unit,
    onFocus: (String) -> Unit,
) {
    // Раскладка: 1-2 → стопкой (без горизонтального сплита); 3+ → по 2 в ряд,
    // одиночный последний занимает всю ширину. Оверфлоу >6 — на потом.
    val rows = if (tiles.size <= 2) tiles.map { listOf(it) } else tiles.chunked(2)
    Column(Modifier.fillMaxSize().padding(2.dp)) {
        rows.forEach { row ->
            Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                row.forEach { t ->
                    key(t.id) {
                        ParticipantTile(
                            model,
                            t,
                            chrome,
                            Modifier.weight(1f).fillMaxHeight().padding(2.dp),
                            onTap = onTap,
                            onFocus = { onFocus(t.id) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ParticipantTile(
    model: AppModel,
    t: Tile,
    chrome: Boolean,
    modifier: Modifier,
    onTap: () -> Unit,
    onFocus: () -> Unit,
) {
    val name = tileName(model, t)
    Box(
        modifier.clip(RoundedCornerShape(12.dp)).background(Tokens.Colors.tile)
            // одиночный тап — показать/скрыть chrome; даблтап — фуллскрин участника.
            .pointerInput(t.id) {
                detectTapGestures(onTap = { onTap() }, onDoubleTap = { onFocus() })
            },
        contentAlignment = Alignment.Center,
    ) {
        when {
            t.video && t.me -> SelfPreviewSurface(model, Modifier.fillMaxSize())
            t.video -> VideoSurface(model, t.id, Modifier.fillMaxSize())
            else -> Avatar(name, 64)
        }
        // голубая рамка у себя (скруглённая — по форме тайла)
        if (t.me) {
            Box(Modifier.matchParentSize().border(2.dp, HoloAccent, RoundedCornerShape(12.dp)))
        }
        if (chrome) {
            Row(
                Modifier.align(Alignment.BottomStart).padding(7.dp)
                    .clip(RoundedCornerShape(7.dp)).background(Tokens.Colors.tileScrim)
                    .padding(horizontal = 7.dp, vertical = 3.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (!t.audio) Icon(Icons.Filled.MicOff, null, tint = Tokens.Colors.iconDangerMuted, modifier = Modifier.size(13.dp))
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
                Text(
                    stringResource(R.string.call_alone_title),
                    color = Color.White,
                    fontSize = 18.sp,
                    fontWeight = FontWeight.Medium,
                )
                Text(
                    stringResource(R.string.call_alone_subtitle),
                    color = HoloSecondary,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(top = 5.dp),
                )
            }
        }
    }
}

/** Локальное превью «Вы» — TextureView (рисуется как обычная вьюха, без
 *  z-order-конфликтов нескольких SurfaceView в сетке). Surface отдаётся
 *  VideoSender'у (камера рендерит прямо сюда, без энкодера/декодера). */
@Composable
private fun SelfPreviewSurface(model: AppModel, modifier: Modifier) {
    val bw = model.videoQuality.width
    val bh = model.videoQuality.height
    // Зеркало — только у фронталки (селфи); у тыловой камеры без зеркала.
    val mirror = model.cameraFront
    AndroidView(
        factory = { ctx ->
            TextureView(ctx).apply {
                surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                    override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                        st.setDefaultBufferSize(bw, bh)
                        // Камера уже поворачивает превью в портрет → матрицу считаем по
                        // ПОРТРЕТНЫМ размерам (bh×bw), rotation=0. Cover-crop как у
                        // приёмника (WYSIWYG) + зеркало (для фронталки).
                        applyTextureTransform(this@apply, bh, bw, 0, model.cameraFront)
                        model.setSelfPreviewSurface(Surface(st))
                    }
                    override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {
                        applyTextureTransform(this@apply, bh, bw, 0, model.cameraFront)
                    }
                    override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean {
                        model.setSelfPreviewSurface(null)
                        return true
                    }
                    override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
                }
            }
        },
        // Флип камеры меняет зеркало без пересоздания surface → re-apply из update.
        update = { view -> applyTextureTransform(view, bh, bw, 0, mirror) },
        modifier = modifier,
    )
}

/** Видео участника — TextureView (несколько в сетке без z-order-конфликтов).
 *  Surface отдаётся приёмнику (декодер рендерит сюда). */
@Composable
private fun VideoSurface(model: AppModel, peer: String, modifier: Modifier) {
    val bw = model.videoQuality.width
    val bh = model.videoQuality.height
    // Угол сенсора отправителя (энкодер шлёт сырой кадр боком) — доворачиваем тайл.
    val rotation = model.roomMemberRotation[peer] ?: 0
    AndroidView(
        factory = { ctx ->
            TextureView(ctx).apply {
                surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                    override fun onSurfaceTextureAvailable(st: SurfaceTexture, w: Int, h: Int) {
                        applyTextureTransform(this@apply, bw, bh, model.roomMemberRotation[peer] ?: 0, mirror = false)
                        model.attachVideoReceiver(peer, Surface(st))
                    }
                    override fun onSurfaceTextureSizeChanged(st: SurfaceTexture, w: Int, h: Int) {
                        applyTextureTransform(this@apply, bw, bh, model.roomMemberRotation[peer] ?: 0, mirror = false)
                    }
                    override fun onSurfaceTextureDestroyed(st: SurfaceTexture): Boolean {
                        model.detachVideoReceiver(peer)
                        return true
                    }
                    override fun onSurfaceTextureUpdated(st: SurfaceTexture) {}
                }
            }
        },
        // Угол приходит из потока ПОСЛЕ создания surface → re-apply при изменении.
        update = { view -> applyTextureTransform(view, bw, bh, rotation, mirror = false) },
        modifier = modifier,
    )
}

/** Трансформация TextureView: поворот на [rotationDeg] (0/90/180/270) + заполнение
 *  тайла с СОХРАНЕНИЕМ аспекта (cover-crop, по центру) + опц. зеркало [mirror].
 *  bufW/bufH — размер буфера (видео); размер вьюхи берём из самой [view]. */
internal fun applyTextureTransform(view: TextureView, bufW: Int, bufH: Int, rotationDeg: Int, mirror: Boolean) {
    val vw = view.width.toFloat()
    val vh = view.height.toFloat()
    if (vw <= 0f || vh <= 0f || bufW <= 0 || bufH <= 0) return
    val cx = vw / 2f
    val cy = vh / 2f
    val rot = ((rotationDeg % 360) + 360) % 360
    val rotated = rot == 90 || rot == 270
    // Эффективные размеры контента после поворота.
    val effW = if (rotated) bufH.toFloat() else bufW.toFloat()
    val effH = if (rotated) bufW.toFloat() else bufH.toFloat()
    val scale = maxOf(vw / effW, vh / effH)
    val drawW = effW * scale
    val drawH = effH * scale
    // Размер ДО поворота (чтобы после поворота получить drawW×drawH).
    val preW = if (rotated) drawH else drawW
    val preH = if (rotated) drawW else drawH
    val m = Matrix()
    m.setScale(preW / vw, preH / vh, cx, cy)
    if (rot != 0) m.postRotate(rot.toFloat(), cx, cy)
    if (mirror) m.postScale(-1f, 1f, cx, cy)
    view.setTransform(m)
}

@Composable
private fun ControlBar(model: AppModel, peerKey: String) {
    Row(
        Modifier.clip(RoundedCornerShape(18.dp)).background(Tokens.Colors.barScrim)
            .padding(horizontal = 12.dp, vertical = 9.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CtlButton(Icons.Filled.PersonAdd, stringResource(R.string.call_btn_invite), Tokens.Colors.btnNeutral) {}
        CtlButton(
            if (model.muted) Icons.Filled.MicOff else Icons.Filled.Mic,
            stringResource(R.string.call_btn_mic),
            if (model.muted) Tokens.Colors.btnDangerMuted else Tokens.Colors.btnNeutral,
            tint = if (model.muted) Tokens.Colors.iconDangerMuted else Color.White,
        ) { model.toggleMute() }
        CtlButton(
            if (model.videoActive) Icons.Filled.Videocam else Icons.Filled.VideocamOff,
            stringResource(R.string.call_btn_video),
            if (model.videoActive) Tokens.Colors.btnNeutral else Tokens.Colors.btnDangerMuted,
            tint = if (model.videoActive) Color.White else Tokens.Colors.iconDangerMuted,
            // долгий тап → экран «Показать видео» (источник/разрешение/fps/битрейт)
            onLongClick = { model.openVideoShare() },
        ) { model.toggleVideo() }
        CtlButton(Icons.AutoMirrored.Filled.Logout, stringResource(R.string.call_btn_leave), HoloError) { model.leaveRoom() }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun CtlButton(
    icon: ImageVector,
    desc: String,
    bg: Color,
    tint: Color = Color.White,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier.clip(RoundedCornerShape(14.dp)).background(bg).size(width = 60.dp, height = 48.dp)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(25.dp)) }
}

@Composable
private fun ChromeChip(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(10.dp)).background(Tokens.Colors.chipScrim).size(34.dp)
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(18.dp)) }
}

/** Пилюля имени (фуллскрин) — высота как у верхних чипов (34dp). */
@Composable
private fun NamePill(name: String) {
    Box(
        Modifier.height(34.dp).clip(RoundedCornerShape(10.dp)).background(Tokens.Colors.chipScrim)
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Чип трафика «↓ ~M МБ/мин» — оценка входящего (34dp). Реальный метеринг — позже. */
@Composable
private fun TrafficChip(mbMin: Int) {
    Row(
        Modifier.height(34.dp).clip(RoundedCornerShape(10.dp)).background(Tokens.Colors.chipScrim)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Icon(Icons.Filled.ArrowDownward, null, tint = HoloSecondary, modifier = Modifier.size(14.dp))
        Text(stringResource(R.string.call_traffic_rate, mbMin), color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun Avatar(name: String, size: Int) {
    Box(
        Modifier.size(size.dp).clip(CircleShape).background(Tokens.Colors.avatar),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            name.take(1).uppercase(),
            color = Color.White,
            fontWeight = FontWeight.Bold,
            fontSize = (size / 2.4f).sp,
        )
    }
}
