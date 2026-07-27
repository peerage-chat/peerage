@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

@Composable
internal fun SettingsScreen(model: AppModel) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        FieldLabel(stringResource(R.string.settings_video_quality))
        Spacer(Modifier.height(4.dp))
        VideoQuality.presets.forEach { preset ->
            val selected = model.videoQuality == preset
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { model.changeVideoQuality(preset) }
                    .background(if (selected) HoloAccentDark else HoloBg)
                    .padding(horizontal = 12.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                RadioButton(
                    selected = selected,
                    onClick = { model.changeVideoQuality(preset) },
                    colors = androidx.compose.material3.RadioButtonDefaults.colors(
                        selectedColor = HoloAccent,
                        unselectedColor = HoloSecondary,
                    ),
                )
                Column {
                    Text(
                        preset.label,
                        color = if (selected) HoloAccent else Color.White,
                        fontSize = 15.sp,
                        fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    )
                    Text(
                        stringResource(R.string.settings_bitrate, "%.0f".format(preset.bitrate / 1_000_000.0)),
                        color = HoloSecondary,
                        fontSize = 11.sp,
                    )
                }
            }
            Divider(color = HoloBorder, thickness = 0.5.dp)
        }
        Spacer(Modifier.height(16.dp))
        Text(
            stringResource(R.string.settings_video_apply),
            color = HoloSecondary,
            fontSize = 11.sp,
        )

        Spacer(Modifier.height(24.dp))
        FieldLabel(stringResource(R.string.settings_notifications))
        Spacer(Modifier.height(4.dp))
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(10.dp))
                .clickable { model.toggleOutboxWarning() }
                .padding(horizontal = 4.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.settings_outbox_warn), color = Color.White, fontSize = 14.sp)
                Text(
                    stringResource(R.string.settings_outbox_warn_hint),
                    color = HoloSecondary,
                    fontSize = 11.sp,
                )
            }
            Switch(
                checked = model.warnOutboxGrowth,
                onCheckedChange = { model.toggleOutboxWarning() },
            )
        }

        Spacer(Modifier.height(24.dp))
        FieldLabel(stringResource(R.string.settings_about))
        Spacer(Modifier.height(4.dp))
        SettingInfoRow(stringResource(R.string.settings_version_label), stringResource(R.string.settings_version_value))
        SettingInfoRow(stringResource(R.string.settings_transport_label), stringResource(R.string.settings_transport_value))
        SettingInfoRow(stringResource(R.string.settings_encryption_label), stringResource(R.string.settings_encryption_value))
        SettingInfoRow(stringResource(R.string.settings_license_label), "GPL-3.0")
        Spacer(Modifier.height(12.dp))
        Text(
            stringResource(R.string.settings_about_future),
            color = HoloSecondary,
            fontSize = 11.sp,
        )
    }
}

/** Строка «параметр — значение» в настройках (только чтение). */
@Composable
private fun SettingInfoRow(label: String, value: String) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Color.White, fontSize = 14.sp)
        Text(value, color = HoloSecondary, fontSize = 13.sp)
    }
}

// ── Экран «Сеть» (mesh) ─────────────────────────────────────────────────────
