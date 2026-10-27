package chat.peerage

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.DesktopWindows
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/**
 * Экран «Показать видео» (долгий тап «Видео» в звонке). Дизайн — макет v2:
 * сегмент Камера/Экран, превью-плейсхолдер с переключателем фронт/тыл, выбор
 * разрешения (СПИСОК из реальных caps устройства, [CameraCaps]), fps и битрейта
 * (авто: пересчитывается при смене разрешения/fps, ручной override снимает «авто»).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ShareVideoScreen(model: AppModel) {
    val ctx = LocalContext.current
    val resolutions = remember(model.shareFront) { CameraCaps.resolutions(ctx, model.shareFront) }
    val has60 = remember(model.shareFront) { CameraCaps.supports60(ctx, model.shareFront) }

    Column(
        Modifier.fillMaxSize().background(Tokens.Colors.callBg).statusBarsPadding().navigationBarsPadding()
            .verticalScroll(rememberScrollState()).padding(horizontal = Tokens.Space.l),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.clip(CircleShape).clickable { model.closeVideoShare() }.padding(Tokens.Space.s),
            ) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), tint = Color.White, modifier = Modifier.size(Tokens.Size.iconL)) }
            Spacer(Modifier.width(Tokens.Space.s))
            Text(stringResource(R.string.share_title), color = Color.White, fontSize = Tokens.Font.title, fontWeight = FontWeight.Medium)
        }

        // Источник: камера / экран.
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m)) {
            SegButton(Icons.Filled.Videocam, stringResource(R.string.share_source_camera), !model.shareScreenMode, Modifier.weight(1f)) {
                model.pickSource(false)
            }
            SegButton(Icons.Filled.DesktopWindows, stringResource(R.string.share_source_screen), model.shareScreenMode, Modifier.weight(1f)) {
                model.pickSource(true)
            }
        }

        Spacer(Modifier.height(Tokens.Space.l))

        // Превью — плейсхолдер (живое превью не нужно; переключатель фронт/тыл для камеры).
        Box(
            Modifier.fillMaxWidth().height(180.dp).clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.tile),
            contentAlignment = Alignment.Center,
        ) {
            // Живое превью только если камера свободна (видео в звонке выключено).
            // Иначе (экран / камера занята отправителем) — плейсхолдер.
            if (!model.shareScreenMode && !model.videoActive) {
                key(model.shareFront) { CameraPreview(model.shareFront, Modifier.fillMaxSize()) }
            } else {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(
                        if (model.shareScreenMode) Icons.Filled.DesktopWindows else Icons.Filled.Videocam,
                        null,
                        tint = Tokens.Colors.iconMuted,
                        modifier = Modifier.size(Tokens.Size.iconHero),
                    )
                    Spacer(Modifier.height(Tokens.Space.m))
                    Text(
                        when {
                            model.shareScreenMode -> stringResource(R.string.share_preview_screen)
                            model.shareFront -> stringResource(R.string.share_preview_front)
                            else -> stringResource(R.string.share_preview_back)
                        },
                        color = HoloSecondary,
                        fontSize = Tokens.Font.body,
                    )
                }
            }
            if (!model.shareScreenMode) {
                Box(
                    Modifier.align(Alignment.TopEnd).padding(Tokens.Space.m).clip(RoundedCornerShape(Tokens.Radius.chip))
                        .background(Tokens.Colors.scrimStrong).size(34.dp).clickable { model.pickFacing(!model.shareFront) },
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Filled.Cameraswitch, stringResource(R.string.call_btn_switch_camera), tint = Color.White, modifier = Modifier.size(Tokens.Size.iconS)) }
            }
        }

        // Параметры — только для камеры (у экрана свой путь, MediaProjection — позже).
        if (!model.shareScreenMode) {
            SectionLabel(stringResource(R.string.share_resolution), stringResource(R.string.share_available_on_device))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m), verticalArrangement = Arrangement.spacedBy(Tokens.Space.m)) {
                resolutions.forEach { r ->
                    Chip(r.label, model.shareResW == r.width && model.shareResH == r.height) { model.pickRes(r.width, r.height) }
                }
            }

            SectionLabel(stringResource(R.string.share_fps))
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m)) {
                Chip(stringResource(R.string.share_fps_value, 30), model.shareFps == 30) { model.pickFps(30) }
                if (has60) Chip(stringResource(R.string.share_fps_value, 60), model.shareFps == 60) { model.pickFps(60) }
            }

            Row(Modifier.fillMaxWidth().padding(top = Tokens.Space.l), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.share_bitrate), color = HoloSecondary, fontSize = Tokens.Font.small, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                val auto = model.shareBitrateAuto
                Row(
                    Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(if (auto) Tokens.Colors.accentMuted else Tokens.Colors.tile)
                        .then(if (auto) Modifier.border(1.5.dp, HoloAccent, RoundedCornerShape(Tokens.Radius.chip)) else Modifier)
                        .clickable { model.pickBitrateAuto() }.padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
                ) {
                    Icon(Icons.Filled.Bolt, null, tint = if (auto) HoloAccent else HoloSecondary, modifier = Modifier.size(Tokens.Size.iconXs))
                    Text(stringResource(R.string.share_auto), color = if (auto) HoloAccent else Color.White, fontSize = Tokens.Font.caption)
                }
            }
            Spacer(Modifier.height(Tokens.Space.s))
            Row(
                Modifier.alpha(if (model.shareBitrateAuto) 0.45f else 1f),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
            ) {
                model.shareBitrateOptions().forEachIndexed { i, bps ->
                    Chip(
                        stringResource(R.string.share_bitrate_value, chat.peerage.ui.fmtMbps(bps)),
                        !model.shareBitrateAuto && model.shareBitrateIdx == i,
                    ) { model.pickBitrateIdx(i) }
                }
            }
        }

        Spacer(Modifier.height(Tokens.Space.xl))
        Box(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.button)).background(HoloAccent)
                .clickable { model.applyVideoShare() }.padding(vertical = Tokens.Space.l),
            contentAlignment = Alignment.Center,
        ) { Text(stringResource(R.string.audio_apply), color = Tokens.Colors.accentDark, fontSize = Tokens.Font.label, fontWeight = FontWeight.Bold) }
        Spacer(Modifier.height(Tokens.Space.xl))
    }
}

@Composable
private fun SegButton(icon: ImageVector, label: String, selected: Boolean, modifier: Modifier, onClick: () -> Unit) {
    Row(
        modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(if (selected) Tokens.Colors.accentMuted else Tokens.Colors.tile)
            .then(if (selected) Modifier.border(1.5.dp, HoloAccent, RoundedCornerShape(Tokens.Radius.chip)) else Modifier)
            .clickable { onClick() }.padding(vertical = Tokens.Space.m),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = if (selected) HoloAccent else HoloSecondary, modifier = Modifier.size(Tokens.Size.iconS))
        Text(label, color = if (selected) HoloAccent else Color.White, fontSize = Tokens.Font.body)
    }
}

@Composable
private fun Chip(text: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(if (selected) Tokens.Colors.accentMuted else Tokens.Colors.tile)
            .then(if (selected) Modifier.border(1.5.dp, HoloAccent, RoundedCornerShape(Tokens.Radius.chip)) else Modifier)
            .clickable { onClick() }.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
        contentAlignment = Alignment.Center,
    ) { Text(text, color = if (selected) HoloAccent else Color.White, fontSize = Tokens.Font.body) }
}

@Composable
private fun SectionLabel(text: String, badge: String? = null) {
    Row(
        Modifier.padding(top = Tokens.Space.l, bottom = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        Text(text, color = HoloSecondary, fontSize = Tokens.Font.small, fontWeight = FontWeight.Medium)
        if (badge != null) {
            chat.peerage.ui.TagBadge(badge)
        }
    }
}
