package dev.p2p.messenger

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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToInt

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
        Modifier.fillMaxSize().background(Color(0xFF0E0F11)).statusBarsPadding().navigationBarsPadding()
            .verticalScroll(rememberScrollState()).padding(horizontal = 18.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.clip(CircleShape).clickable { model.closeVideoShare() }.padding(6.dp),
            ) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), tint = Color.White, modifier = Modifier.size(24.dp)) }
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.share_title), color = Color.White, fontSize = 19.sp, fontWeight = FontWeight.Medium)
        }

        // Источник: камера / экран.
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            SegButton(Icons.Filled.Videocam, stringResource(R.string.share_source_camera), !model.shareScreenMode, Modifier.weight(1f)) {
                model.pickSource(false)
            }
            SegButton(Icons.Filled.DesktopWindows, stringResource(R.string.share_source_screen), model.shareScreenMode, Modifier.weight(1f)) {
                model.pickSource(true)
            }
        }

        Spacer(Modifier.height(16.dp))

        // Превью — плейсхолдер (живое превью не нужно; переключатель фронт/тыл для камеры).
        Box(
            Modifier.fillMaxWidth().height(180.dp).clip(RoundedCornerShape(14.dp)).background(Color(0xFF1A1D21)),
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
                        tint = Color(0xFF5A5F66),
                        modifier = Modifier.size(40.dp),
                    )
                    Spacer(Modifier.height(10.dp))
                    Text(
                        when {
                            model.shareScreenMode -> stringResource(R.string.share_preview_screen)
                            model.shareFront -> stringResource(R.string.share_preview_front)
                            else -> stringResource(R.string.share_preview_back)
                        },
                        color = HoloSecondary,
                        fontSize = 13.sp,
                    )
                }
            }
            if (!model.shareScreenMode) {
                Box(
                    Modifier.align(Alignment.TopEnd).padding(10.dp).clip(RoundedCornerShape(10.dp))
                        .background(Color(0xCC0E0F11)).size(34.dp).clickable { model.pickFacing(!model.shareFront) },
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Filled.Cameraswitch, stringResource(R.string.call_btn_switch_camera), tint = Color.White, modifier = Modifier.size(18.dp)) }
            }
        }

        // Параметры — только для камеры (у экрана свой путь, MediaProjection — позже).
        if (!model.shareScreenMode) {
            SectionLabel(stringResource(R.string.share_resolution), stringResource(R.string.share_available_on_device))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                resolutions.forEach { r ->
                    Chip(r.label, model.shareResW == r.width && model.shareResH == r.height) { model.pickRes(r.width, r.height) }
                }
            }

            SectionLabel(stringResource(R.string.share_fps))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Chip(stringResource(R.string.share_fps_value, 30), model.shareFps == 30) { model.pickFps(30) }
                if (has60) Chip(stringResource(R.string.share_fps_value, 60), model.shareFps == 60) { model.pickFps(60) }
            }

            SectionLabel(
                stringResource(R.string.share_bitrate),
                if (model.shareBitrateAuto) stringResource(R.string.share_auto) else null,
            )
            val sel = (model.shareBitrate / 1_000_000.0).roundToInt()
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                bitrateOptionsMbps(model).forEach { mb ->
                    Chip(stringResource(R.string.share_bitrate_value, mb), sel == mb) { model.pickBitrate(mb * 1_000_000) }
                }
            }
        }

        Spacer(Modifier.height(22.dp))
        Box(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(HoloAccent)
                .clickable { model.applyVideoShare() }.padding(vertical = 15.dp),
            contentAlignment = Alignment.Center,
        ) { Text(stringResource(R.string.share_btn_show), color = Color(0xFF04222E), fontSize = 16.sp, fontWeight = FontWeight.Bold) }
        Spacer(Modifier.height(20.dp))
    }
}

/** Три варианта битрейта (Мбит) вокруг авто-рекомендации: ниже · авто · выше. */
private fun bitrateOptionsMbps(model: AppModel): List<Int> {
    val a = (model.autoBitrate(model.shareResW, model.shareResH, model.shareFps) / 1_000_000.0).roundToInt().coerceAtLeast(1)
    return listOf((a * 0.6).roundToInt().coerceAtLeast(1), a, (a * 1.5).roundToInt()).distinct()
}

@Composable
private fun SegButton(icon: ImageVector, label: String, selected: Boolean, modifier: Modifier, onClick: () -> Unit) {
    Row(
        modifier.clip(RoundedCornerShape(10.dp)).background(if (selected) Color(0xFF13313D) else Color(0xFF1A1D21))
            .then(if (selected) Modifier.border(1.5.dp, HoloAccent, RoundedCornerShape(10.dp)) else Modifier)
            .clickable { onClick() }.padding(vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = if (selected) HoloAccent else HoloSecondary, modifier = Modifier.size(18.dp))
        Text(label, color = if (selected) HoloAccent else Color.White, fontSize = 14.sp)
    }
}

@Composable
private fun Chip(text: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(10.dp)).background(if (selected) Color(0xFF13313D) else Color(0xFF1A1D21))
            .then(if (selected) Modifier.border(1.5.dp, HoloAccent, RoundedCornerShape(10.dp)) else Modifier)
            .clickable { onClick() }.padding(horizontal = 16.dp, vertical = 10.dp),
        contentAlignment = Alignment.Center,
    ) { Text(text, color = if (selected) HoloAccent else Color.White, fontSize = 14.sp) }
}

@Composable
private fun SectionLabel(text: String, badge: String? = null) {
    Row(
        Modifier.padding(top = 18.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(text, color = HoloSecondary, fontSize = 12.sp, fontWeight = FontWeight.Medium)
        if (badge != null) {
            Box(
                Modifier.clip(RoundedCornerShape(6.dp)).background(Color(0x2233B5E5)).padding(horizontal = 7.dp, vertical = 2.dp),
            ) { Text(badge, color = HoloAccent, fontSize = 10.sp) }
        }
    }
}
