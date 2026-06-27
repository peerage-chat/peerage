@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*

@Composable
internal fun SettingsScreen(model: AppModel) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        FieldLabel("КАЧЕСТВО ВИДЕО-ЗВОНКА")
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
                        "%.0f Мбит/с".format(preset.bitrate / 1_000_000.0),
                        color = HoloSecondary,
                        fontSize = 11.sp,
                    )
                }
            }
            Divider(color = HoloBorder, thickness = 0.5.dp)
        }
        Spacer(Modifier.height(16.dp))
        Text(
            "Изменение применяется к следующему звонку.",
            color = HoloSecondary,
            fontSize = 11.sp,
        )

        Spacer(Modifier.height(24.dp))
        FieldLabel("О ПРИЛОЖЕНИИ")
        Spacer(Modifier.height(4.dp))
        SettingInfoRow("Версия", "0.1.0 (dev)")
        SettingInfoRow("Транспорт", "Yggdrasil mesh (serverless)")
        SettingInfoRow("Шифрование", "E2E, всегда включено")
        SettingInfoRow("Лицензия", "GPL-3.0")
        Spacer(Modifier.height(12.dp))
        Text(
            "Энергия, медиа и хранилище — настройки появятся ближе к релизу.",
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
