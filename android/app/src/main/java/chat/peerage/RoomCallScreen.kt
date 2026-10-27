package chat.peerage

import android.Manifest
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.CloseFullscreen
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PhoneInTalk
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Icon
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
import androidx.compose.ui.viewinterop.AndroidView
import chat.peerage.ui.PhotoOr
import chat.peerage.ui.initialOf
import chat.peerage.ui.rememberPermissionRunner

/**
 * Экран активного звонка-комнаты (Discord-style сетка). Дизайн — docs/design/call-ui.md.
 *
 * Тайл = ОДИН компонент: живое видео (`TextureView` + приёмник) если у участника
 * видео, иначе аватар; у себя — превью с камеры. `key(peer)` стабилизирует surface
 * через перерисовки → нет тёрки декодеров. UI (chrome) рисуется ПОВЕРХ неподвижной
 * сетки, тап скрывает его. Говорящий — зелёная рамка (`speakingPeers`).
 *
 * Свап декодеров с гистерезисом, пиннинг и оверфлоу «+N» — на малом N не нужны.
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
    // Фуллскрин-фокус снимаем, если участник исчез. Ключ — состав, а не размер:
    // ушёл один и пришёл другой — размер тот же, а фокус висел на ушедшем.
    LaunchedEffect(remotes.map { it.peer }) {
        if (focused != null && focused != "me" && remotes.none { it.peer == focused }) focused = null
    }
    // Оверлей НЕ прячется сам (по просьбе) — скрыть/показать только тапом по тайлу.

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
                    else -> Avatar(tileName(model, foc), 104, tileAvatar(model, foc))
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
                    Modifier.align(Alignment.TopStart).statusBarsPadding().padding(Tokens.Space.m),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    NamePill(tileName(model, foc))
                    if (!foc.me) TrafficChip(if (foc.video) AppModel.TRAFFIC_VIDEO_MB_MIN else AppModel.TRAFFIC_AUDIO_MB_MIN)
                }
                Box(Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(Tokens.Space.m)) {
                    ChromeChip(Icons.Filled.CloseFullscreen, stringResource(R.string.call_minimize)) { focused = null }
                }
            } else {
                // сетка: чип трафика слева-сверху, смена камеры + настройки справа-сверху
                val incoming = remotes.sumOf {
                    when {
                        it.video -> AppModel.TRAFFIC_VIDEO_MB_MIN
                        it.audio -> AppModel.TRAFFIC_AUDIO_MB_MIN
                        else -> 0
                    }
                }
                if (incoming > 0) {
                    Box(Modifier.align(Alignment.TopStart).statusBarsPadding().padding(Tokens.Space.m)) {
                        TrafficChip(incoming)
                    }
                }
                Row(
                    Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(Tokens.Space.m),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
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
            Box(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = Tokens.Space.l)) {
                ControlBar(model)
            }
        }
        if (showSettings) CallSettings(model) { showSettings = false }
        if (model.showVideoShare) ShareVideoScreen(model)
        if (model.showAudioSettings) AudioSettingsScreen(model)
        // «Назад» закрывает верхний слой (в порядке отрисовки — сверху вниз). Сам
        // звонок системная «назад» не трогает: это делает оболочка.
        BackHandler(
            enabled = model.showAudioSettings || model.showVideoShare || showSettings || focused != null,
        ) {
            when {
                model.showAudioSettings -> model.closeAudioSettings()
                model.showVideoShare -> model.closeVideoShare()
                showSettings -> showSettings = false
                else -> focused = null
            }
        }
    }
}

/** Имя тайла: «Вы» для себя, иначе имя контакта / короткий ключ. */
private fun tileAvatar(model: AppModel, t: Tile): ByteArray? =
    if (t.me) model.selfAvatar else model.peerAvatar(t.id)

@Composable
private fun tileName(model: AppModel, t: Tile): String =
    if (t.me) {
        stringResource(R.string.call_you)
    } else {
        model.peerName(t.id)
    }

/** Настройки звонка (кнопка ⚙️): вывод звука (динамик), качество видео. Оверлей
 *  поверх звонка; тап по фону не закрывает (только крестик). */
@Composable
private fun CallSettings(model: AppModel, onClose: () -> Unit) {
    Column(
        Modifier.fillMaxSize().background(Tokens.Colors.overlay).clickable(enabled = false) {}
            .statusBarsPadding().navigationBarsPadding().padding(horizontal = Tokens.Space.l),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.call_settings_title),
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.weight(1f),
            )
            Box(
                Modifier.clip(CircleShape).background(Tokens.Colors.glass).size(36.dp).clickable { onClose() },
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Filled.Close, stringResource(R.string.call_close), tint = Color.White, modifier = Modifier.size(Tokens.Size.iconS)) }
        }

        Text(stringResource(R.string.call_audio_output), color = HoloSecondary, fontSize = Tokens.Font.caption, fontWeight = FontWeight.Medium)
        Spacer(Modifier.height(Tokens.Space.s))
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { model.toggleSpeaker() }
                .background(Tokens.Colors.tile).padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                if (model.speakerOn) Icons.Filled.VolumeUp else Icons.Filled.PhoneInTalk,
                null,
                tint = if (model.speakerOn) HoloAccent else HoloSecondary,
                modifier = Modifier.size(Tokens.Size.iconM),
            )
            Spacer(Modifier.width(Tokens.Space.m))
            Text(stringResource(R.string.call_speaker), color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
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

        Spacer(Modifier.height(Tokens.Space.m))
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { model.toggleTechInfo() }
                .background(Tokens.Colors.tile).padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(stringResource(R.string.call_tech_info), color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
            Switch(
                checked = model.showTechInfo,
                onCheckedChange = { model.toggleTechInfo() },
                colors = SwitchDefaults.colors(
                    checkedThumbColor = Color.White,
                    checkedTrackColor = HoloAccent,
                    uncheckedThumbColor = HoloSecondary,
                    uncheckedTrackColor = Tokens.Colors.tile,
                ),
            )
        }

        // Видео — на полноценный экран «Показать видео» (разрешения из камеры, fps,
        // битрейт): здесь раньше был жёсткий список пресетов, предлагавший и то, что
        // камера не умеет.
        Spacer(Modifier.height(Tokens.Space.m))
        val vq = model.videoQuality
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.chip))
                .clickable {
                    onClose()
                    model.openVideoShare()
                }
                .background(Tokens.Colors.tile).padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconM))
            Spacer(Modifier.width(Tokens.Space.m))
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.call_video_settings), color = Color.White, fontSize = Tokens.Font.label)
                Text(
                    "${vq.height}p · ${stringResource(R.string.share_fps_value, vq.fps)}",
                    color = HoloSecondary,
                    fontSize = Tokens.Font.small,
                )
            }
            Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconM))
        }
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
    Column(Modifier.fillMaxSize().padding(Tokens.Space.xxs)) {
        rows.forEach { row ->
            Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs)) {
                row.forEach { t ->
                    key(t.id) {
                        ParticipantTile(
                            model,
                            t,
                            chrome,
                            Modifier.weight(1f).fillMaxHeight().padding(Tokens.Space.xxs),
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
        modifier.clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.tile)
            // одиночный тап — показать/скрыть chrome; даблтап — фуллскрин участника.
            .pointerInput(t.id) {
                detectTapGestures(onTap = { onTap() }, onDoubleTap = { onFocus() })
            },
        contentAlignment = Alignment.Center,
    ) {
        when {
            t.video && t.me -> SelfPreviewSurface(model, Modifier.fillMaxSize())
            t.video -> VideoSurface(model, t.id, Modifier.fillMaxSize())
            else -> Avatar(name, 64, tileAvatar(model, t))
        }
        // рамка: голубая у себя; зелёная — у говорящего сейчас участника (active-speaker)
        if (t.me) {
            Box(Modifier.matchParentSize().border(2.dp, HoloAccent, RoundedCornerShape(Tokens.Radius.button)))
        } else if (t.id in model.speakingPeers) {
            Box(Modifier.matchParentSize().border(2.dp, Tokens.Colors.online, RoundedCornerShape(Tokens.Radius.button)))
        }
        if (chrome) {
            Row(
                Modifier.align(Alignment.BottomStart).padding(Tokens.Space.s)
                    .clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.tileScrim)
                    .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xxs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
            ) {
                if (!t.audio) Icon(Icons.Filled.MicOff, null, tint = Tokens.Colors.iconDangerMuted, modifier = Modifier.size(Tokens.Size.iconXs))
                Text(name, color = Color.White, fontSize = Tokens.Font.small, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        // Тех-инфо чип (тумблер в ⚙️) — ЧАСТЬ оверлея, право-верх тайла. 4 строки:
        // разрешение·fps · битрейт(цель) · трафик(реальный kbps, ↑своё/↓чужое) · пинг.
        if (chrome && model.showTechInfo) {
            val vq = model.videoQuality
            val kbps = model.peerKbps[if (t.me) "self" else t.id] ?: 0
            Column(
                // Ниже строки chrome-кнопок (шестерёнка/смена камеры в TopEnd), чтобы
                // не перекрываться. Настроено под текущий 1:1 (тайл = экран); при
                // мультипати-сетке (#24) вернуться к per-tile-раскладке.
                Modifier.align(Alignment.TopEnd).statusBarsPadding().padding(top = Tokens.Space.m + Tokens.Size.chromeChip + Tokens.Space.s, end = Tokens.Space.s)
                    .clip(RoundedCornerShape(Tokens.Radius.badge)).background(Tokens.Colors.chipScrim)
                    .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs),
                horizontalAlignment = Alignment.End,
                verticalArrangement = Arrangement.spacedBy(1.dp),
            ) {
                TechLine("${vq.height}p·${vq.fps}")
                TechLine("${vq.bitrate / 1000} kbps")
                TechRate(up = t.me, kbps = kbps)
                TechLine("— ms") // пинг: нужен E2E RTT (Ping/Pong), отдельная задача
            }
        }
    }
}

/** Строка тех-инфо чипа (цвет/размер chrome-оверлея). */
@Composable
private fun TechLine(text: String) {
    Text(text, color = Tokens.Colors.iconChrome, fontSize = Tokens.Font.caption, maxLines = 1)
}

/** Строка трафика: иконка направления (↑ исходящий своё · ↓ входящий чужое) + kbps. */
@Composable
private fun TechRate(up: Boolean, kbps: Int) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xxs)) {
        Icon(
            if (up) Icons.Filled.ArrowUpward else Icons.Filled.ArrowDownward,
            null,
            tint = Tokens.Colors.iconChrome,
            modifier = Modifier.size(Tokens.Size.iconXs),
        )
        Text("$kbps kbps", color = Tokens.Colors.iconChrome, fontSize = Tokens.Font.caption, maxLines = 1)
    }
}

@Composable
private fun AloneTile(model: AppModel, peerKey: String, chrome: Boolean) {
    val name = model.peerName(peerKey)
    Box(Modifier.fillMaxSize()) {
        if (model.videoActive) {
            // Камера включена в одиночестве → своё превью на весь экран (его surface и
            // запускает VideoSender). Текст «вы одни» ложится поверх.
            SelfPreviewSurface(model, Modifier.fillMaxSize())
        } else {
            // Аватар — неподвижный базовый слой, чуть выше центра, чтобы текст лёг под ним.
            Box(Modifier.align(Alignment.Center).offset(y = (-32).dp)) { Avatar(name, 104, model.peerAvatar(peerKey)) }
        }
        if (chrome) {
            Column(
                Modifier.align(Alignment.Center).offset(y = 78.dp)
                    .clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.barScrim)
                    .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    stringResource(R.string.call_alone_title),
                    color = Color.White,
                    fontSize = Tokens.Font.title,
                    fontWeight = FontWeight.Medium,
                )
                Text(
                    stringResource(R.string.call_alone_subtitle),
                    color = HoloSecondary,
                    fontSize = Tokens.Font.body,
                    modifier = Modifier.padding(top = Tokens.Space.xs),
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

/** «Позвать в звонок»: участники группы, которых нет в комнате, с отметками. Оверлей
 *  в стиле настроек звонка; тап по фону не закрывает (только крестик). */
@Composable
internal fun RingSheet(model: AppModel, onClose: () -> Unit) {
    // Ростер мог ещё не прийти в кэш модели — подтянуть при открытии.
    LaunchedEffect(Unit) { model.currentRoom?.let { model.loadGroupMembers(it) } }
    val candidates = model.ringCandidates()
    var picked by remember { mutableStateOf(emptySet<String>()) }
    Column(
        Modifier.fillMaxSize().background(Tokens.Colors.overlay).clickable(enabled = false) {}
            .statusBarsPadding().navigationBarsPadding().padding(horizontal = Tokens.Space.l),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                stringResource(R.string.call_ring_title),
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.weight(1f),
            )
            Box(
                Modifier.clip(CircleShape).background(Tokens.Colors.glass).size(36.dp).clickable { onClose() },
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Filled.Close, stringResource(R.string.call_close), tint = Color.White, modifier = Modifier.size(Tokens.Size.iconS)) }
        }
        if (candidates.isEmpty()) {
            Text(
                stringResource(R.string.call_ring_empty),
                color = HoloSecondary,
                fontSize = Tokens.Font.label,
                modifier = Modifier.padding(top = Tokens.Space.s),
            )
            return@Column
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
            candidates.forEach { peer ->
                val on = peer in picked
                val name = model.peerName(peer)
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.chip))
                        .clickable { picked = if (on) picked - peer else picked + peer }
                        .background(if (on) Tokens.Colors.accentMuted else Tokens.Colors.tile)
                        .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
                ) {
                    Avatar(name, 36, model.peerAvatar(peer))
                    Text(
                        name,
                        color = if (on) HoloAccent else Color.White,
                        fontSize = Tokens.Font.label,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    Checkbox(
                        checked = on,
                        onCheckedChange = { picked = if (on) picked - peer else picked + peer },
                        colors = CheckboxDefaults.colors(checkedColor = HoloAccent, uncheckedColor = HoloSecondary),
                    )
                }
                Spacer(Modifier.height(Tokens.Space.s))
            }
        }
        Box(
            Modifier.fillMaxWidth().padding(vertical = Tokens.Space.m).clip(RoundedCornerShape(Tokens.Radius.button))
                .background(if (picked.isEmpty()) Tokens.Colors.tile else HoloAccent)
                .clickable(enabled = picked.isNotEmpty()) {
                    model.ringIntoRoom(picked.toList())
                    onClose()
                }
                .padding(vertical = Tokens.Space.m),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                stringResource(R.string.call_ring_btn),
                color = if (picked.isEmpty()) HoloSecondary else HoloAccentDark,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
            )
        }
    }
}

@Composable
private fun ControlBar(model: AppModel) {
    Row(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.pill)).background(Tokens.Colors.barScrim)
            .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Камера нужна только при ВКЛЮЧЕНИИ видео; выключение разрешения не требует.
        val runWithPerm = rememberPermissionRunner()
        CtlButton(
            if (model.muted) Icons.Filled.MicOff else Icons.Filled.Mic,
            stringResource(R.string.call_btn_mic),
            if (model.muted) Tokens.Colors.btnDangerMuted else Tokens.Colors.btnNeutral,
            tint = if (model.muted) Tokens.Colors.iconDangerMuted else Color.White,
            // долгий тап → настройки звука (вход/выход/качество/обработка)
            onLongClick = { model.openAudioSettings() },
        ) { model.toggleMute() }
        CtlButton(
            if (model.videoActive) Icons.Filled.Videocam else Icons.Filled.VideocamOff,
            stringResource(R.string.call_btn_video),
            if (model.videoActive) Tokens.Colors.btnNeutral else Tokens.Colors.btnDangerMuted,
            tint = if (model.videoActive) Color.White else Tokens.Colors.iconDangerMuted,
            // долгий тап → экран «Показать видео» (источник/разрешение/fps/битрейт)
            onLongClick = { model.openVideoShare() },
        ) {
            if (model.videoActive) {
                model.toggleVideo()
            } else {
                runWithPerm(arrayOf(Manifest.permission.CAMERA), R.string.perm_denied_camera) { model.toggleVideo() }
            }
        }
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
        Modifier.clip(RoundedCornerShape(Tokens.Radius.button)).background(bg).size(width = Tokens.Size.ctlButtonW, height = Tokens.Size.ctlButtonH)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = tint, modifier = Modifier.size(Tokens.Size.iconL)) }
}

@Composable
private fun ChromeChip(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.chipScrim).size(Tokens.Size.chromeChip)
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconS)) }
}

/** Пилюля имени (фуллскрин) — высота как у верхних чипов (34dp). */
@Composable
private fun NamePill(name: String) {
    Box(
        Modifier.height(Tokens.Size.chromeChip).clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.chipScrim)
            .padding(horizontal = Tokens.Space.m),
        contentAlignment = Alignment.Center,
    ) {
        Text(name, color = Color.White, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Чип трафика «↓ ~M МБ/мин» — оценка входящего (34dp). Реальный метеринг — позже. */
@Composable
private fun TrafficChip(mbMin: Int) {
    Row(
        Modifier.height(Tokens.Size.chromeChip).clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.chipScrim)
            .padding(horizontal = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        Icon(Icons.Filled.ArrowDownward, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconXs))
        Text(stringResource(R.string.call_traffic_rate, mbMin), color = Color.White, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun Avatar(name: String, size: Int, photo: ByteArray?) {
    Box(
        Modifier.size(size.dp).clip(RoundedCornerShape(Tokens.Radius.avatar)).background(Tokens.Colors.avatar),
        contentAlignment = Alignment.Center,
    ) {
        PhotoOr(photo, size.dp) {
            Text(
                initialOf(name),
                color = Color.White,
                fontWeight = FontWeight.Bold,
                fontSize = Tokens.Font.initial(size.dp),
            )
        }
    }
}
