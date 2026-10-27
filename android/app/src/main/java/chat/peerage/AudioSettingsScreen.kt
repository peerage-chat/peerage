package chat.peerage

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
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
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Smartphone
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.Icon
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/**
 * Экран «Настройки звука» (долгий тап «Микрофон» в звонке). Дизайн — макет v2:
 * живой уровень микрофона, выбор устройства входа/выхода (только реально доступные),
 * качество (Голос↔Аудио + Авто), тумблеры обработки AEC/NS/AGC.
 */
@Composable
fun AudioSettingsScreen(model: AppModel) {
    val outs = model.availableAudioOuts()
    val ins = model.availableAudioIns()
    Column(
        Modifier.fillMaxSize().background(Tokens.Colors.callBg).statusBarsPadding().navigationBarsPadding()
            .verticalScroll(rememberScrollState()).padding(horizontal = 18.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.clip(CircleShape).clickable { model.closeAudioSettings() }.padding(6.dp)) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), tint = Color.White, modifier = Modifier.size(Tokens.Size.iconL))
            }
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.audio_settings_title), color = Color.White, fontSize = Tokens.Font.title, fontWeight = FontWeight.Medium)
        }

        // Живой уровень микрофона (20 сегментов).
        Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.tile).padding(15.dp)) {
            Column {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Icon(Icons.Filled.Mic, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconS))
                    Text(stringResource(R.string.audio_mic_level), color = Tokens.Colors.secondary, fontSize = Tokens.Font.body)
                }
                Spacer(Modifier.height(11.dp))
                val lit = (model.micLevel * 20).roundToInt().coerceIn(0, 20)
                Row(Modifier.fillMaxWidth().height(22.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    repeat(20) { i ->
                        Box(
                            Modifier.weight(1f).fillMaxHeight()
                                .clip(RoundedCornerShape(2.dp))
                                .background(if (i < lit) Tokens.Colors.accent else Tokens.Colors.trackIdle),
                        )
                    }
                }
            }
        }

        Spacer(Modifier.height(18.dp))
        SectionLabel(stringResource(R.string.audio_input), stringResource(R.string.share_available_on_device))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if ("builtin" in ins) DevChip(Icons.Filled.Smartphone, stringResource(R.string.audio_dev_builtin), model.audioIn == "builtin") { model.pickAudioIn("builtin") }
            if ("headset" in ins) DevChip(Icons.Filled.Headset, stringResource(R.string.audio_dev_headset), model.audioIn == "headset") { model.pickAudioIn("headset") }
            if ("bluetooth" in ins) DevChip(Icons.Filled.Bluetooth, stringResource(R.string.audio_dev_bluetooth), model.audioIn == "bluetooth") { model.pickAudioIn("bluetooth") }
        }

        Spacer(Modifier.height(18.dp))
        SectionLabel(stringResource(R.string.audio_output))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if ("builtin" in outs) DevChip(Icons.Filled.Smartphone, stringResource(R.string.audio_dev_builtin), model.audioOut == "builtin") { model.pickAudioOut("builtin") }
            if ("speaker" in outs) DevChip(Icons.Filled.VolumeUp, stringResource(R.string.audio_dev_speaker), model.audioOut == "speaker") { model.pickAudioOut("speaker") }
            if ("bluetooth" in outs) DevChip(Icons.Filled.Bluetooth, stringResource(R.string.audio_dev_bluetooth), model.audioOut == "bluetooth") { model.pickAudioOut("bluetooth") }
        }

        Spacer(Modifier.height(18.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.audio_quality), color = Tokens.Colors.secondary, fontSize = Tokens.Font.small, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
            val auto = model.audioQualityAuto
            Row(
                Modifier.clip(RoundedCornerShape(Tokens.Radius.chip))
                    .background(if (auto) Tokens.Colors.accentMuted else Tokens.Colors.tile)
                    .then(if (auto) Modifier.border(1.5.dp, Tokens.Colors.accent, RoundedCornerShape(Tokens.Radius.chip)) else Modifier)
                    .clickable { model.setAudioQuality(!auto) }
                    .padding(horizontal = 11.dp, vertical = 5.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Icon(Icons.Filled.Bolt, null, tint = if (auto) Tokens.Colors.accent else Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconXs))
                Text(stringResource(R.string.audio_quality_auto), color = if (auto) Tokens.Colors.accent else Color.White, fontSize = Tokens.Font.caption)
            }
        }
        Spacer(Modifier.height(4.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(stringResource(R.string.audio_quality_voice), color = Tokens.Colors.secondary, fontSize = Tokens.Font.caption)
            Text(stringResource(R.string.audio_quality_audio), color = Tokens.Colors.secondary, fontSize = Tokens.Font.caption)
        }
        Slider(
            value = model.audioBitrate.toFloat(),
            onValueChange = { model.setAudioQuality(false, it.roundToInt()) },
            valueRange = AppModel.AUDIO_BITRATE_MIN.toFloat()..AppModel.AUDIO_BITRATE_MAX.toFloat(),
            enabled = !model.audioQualityAuto,
            colors = SliderDefaults.colors(
                thumbColor = Tokens.Colors.accent,
                activeTrackColor = Tokens.Colors.accent,
                inactiveTrackColor = Tokens.Colors.trackIdle,
                disabledThumbColor = Tokens.Colors.secondary,
                disabledActiveTrackColor = Tokens.Colors.trackIdle,
            ),
        )
        Text(
            if (model.audioQualityAuto) {
                stringResource(R.string.audio_quality_adaptive, AppModel.AUDIO_BITRATE_MIN / 1000, AppModel.AUDIO_BITRATE_MAX / 1000)
            } else {
                stringResource(R.string.audio_quality_rate, model.audioBitrate / 1000)
            },
            color = Tokens.Colors.secondary,
            fontSize = Tokens.Font.small,
            modifier = Modifier.fillMaxWidth(),
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )

        Spacer(Modifier.height(18.dp))
        SectionLabel(stringResource(R.string.audio_processing))
        FxToggle(stringResource(R.string.audio_ns), model.audioFx.ns) { model.setAudioFx(ns = it) }
        FxToggle(stringResource(R.string.audio_aec), model.audioFx.aec) { model.setAudioFx(aec = it) }
        FxToggle(stringResource(R.string.audio_agc), model.audioFx.agc) { model.setAudioFx(agc = it) }

        Spacer(Modifier.height(20.dp))
        Box(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.accent)
                .clickable { model.applyAudioSettings() }.padding(vertical = 15.dp),
            contentAlignment = Alignment.Center,
        ) { Text(stringResource(R.string.audio_apply), color = Tokens.Colors.accentDark, fontSize = Tokens.Font.label, fontWeight = FontWeight.Bold) }
        Spacer(Modifier.height(20.dp))
    }
}

@Composable
private fun DevChip(icon: ImageVector, label: String, selected: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(if (selected) Tokens.Colors.accentMuted else Tokens.Colors.tile)
            .then(if (selected) Modifier.border(1.5.dp, Tokens.Colors.accent, RoundedCornerShape(Tokens.Radius.chip)) else Modifier)
            .clickable { onClick() }.padding(horizontal = 13.dp, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(icon, null, tint = if (selected) Tokens.Colors.accent else Color.White, modifier = Modifier.size(Tokens.Size.iconXs))
        Text(label, color = if (selected) Tokens.Colors.accent else Color.White, fontSize = Tokens.Font.body)
    }
}

@Composable
private fun FxToggle(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.tile)
            .clickable { onChange(!on) }.padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
        Switch(
            checked = on,
            onCheckedChange = onChange,
            colors = SwitchDefaults.colors(
                checkedThumbColor = Color.White,
                checkedTrackColor = Tokens.Colors.accent,
                uncheckedThumbColor = Tokens.Colors.secondary,
                uncheckedTrackColor = Tokens.Colors.tile,
            ),
        )
    }
}

@Composable
private fun SectionLabel(text: String, badge: String? = null) {
    Row(
        Modifier.padding(bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(text, color = Tokens.Colors.secondary, fontSize = Tokens.Font.small, fontWeight = FontWeight.Medium)
        if (badge != null) {
            chat.peerage.ui.TagBadge(badge)
        }
    }
}
