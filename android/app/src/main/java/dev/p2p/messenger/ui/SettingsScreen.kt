@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Настройки (M4) — по app-ui.md: профиль-шапка, затем группы «Конфиденциальность»,
 * «Безопасность», «Медиа и сеть», футер «О программе».
 *
 * Про конфиденциальность: тумблеров нет намеренно. Отчёты о прочтении и индикатор
 * набора мы не отправляем ВООБЩЕ — переключатель подразумевал бы, что сигнал есть и
 * его можно выключить. Показываем факт, а не иллюзию выбора. Появится сигнал —
 * появится и тумблер.
 */
@Composable
internal fun SettingsScreen(
    model: AppModel,
    onEditProfile: () -> Unit = {},
    onMyQr: () -> Unit = {},
    onNetwork: () -> Unit = {},
    onStorage: () -> Unit = {},
    onContacts: () -> Unit = {},
) {
    var showPhrase by remember { mutableStateOf(false) }
    if (showPhrase) {
        SeedPhraseSheet(model) { showPhrase = false }
        return
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        ProfileHeader(model, onEditProfile, onMyQr)

        // ── Конфиденциальность ──────────────────────────────────────────────
        FieldLabel(stringResource(R.string.settings_privacy))
        SurfaceCard {
            Row {
                Icon(Icons.Filled.VisibilityOff, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
                Spacer(Modifier.width(12.dp))
                Column {
                    Text(stringResource(R.string.settings_privacy_none), color = Color.White, fontSize = 15.sp)
                    Text(
                        stringResource(R.string.settings_privacy_none_desc),
                        color = HoloSecondary,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                    Text(
                        stringResource(R.string.settings_privacy_presence),
                        color = HoloSecondary,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
            }
        }

        // ── Безопасность ────────────────────────────────────────────────────
        FieldLabel(stringResource(R.string.settings_security))
        SettingRow(
            Icons.Filled.Key,
            stringResource(R.string.settings_key_backup),
            if (model.keyBackupDone) {
                stringResource(R.string.settings_key_backup_done)
            } else {
                stringResource(R.string.settings_key_backup_todo)
            },
            valueColor = if (model.keyBackupDone) HoloOnline else HoloWarn,
        ) { showPhrase = true }
        // Блокировка: подтверждение — системное (биометрия/PIN устройства), своего
        // секрета мы не храним. Без экрана блокировки на устройстве включать нечего.
        val ctx = LocalContext.current
        val deviceSecure = remember { AppLock.deviceSecure(ctx) }
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Lock, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.settings_app_lock), color = Color.White, fontSize = 15.sp)
                    Text(
                        stringResource(
                            if (deviceSecure) R.string.settings_app_lock_desc else R.string.settings_app_lock_no_device,
                        ),
                        color = if (deviceSecure) HoloSecondary else HoloWarn,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
                Switch(
                    checked = AppLock.enabled,
                    enabled = deviceSecure,
                    onCheckedChange = { AppLock.setEnabled(ctx, it) },
                )
            }
        }
        SettingRow(
            Icons.Filled.Shield,
            stringResource(R.string.settings_at_rest),
            stringResource(R.string.settings_at_rest_keystore),
        )
        SettingRow(
            Icons.Filled.Verified,
            stringResource(R.string.settings_verified),
            stringResource(
                R.string.settings_verified_count,
                model.contacts.count { it.verified },
                model.contacts.size,
            ),
            onClick = onContacts,
        )

        // ── Медиа и сеть ────────────────────────────────────────────────────
        FieldLabel(stringResource(R.string.settings_media_net))
        SettingRow(Icons.Filled.Share, stringResource(R.string.settings_net_battery), null, onClick = onNetwork)
        SettingRow(Icons.Filled.Storage, stringResource(R.string.menu_storage), null, onClick = onStorage)

        FieldLabel(stringResource(R.string.settings_video_quality))
        VideoQuality.presets.forEach { preset ->
            val selected = model.videoQuality == preset
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                    .clickable { model.changeVideoQuality(preset) }
                    .background(if (selected) HoloAccentDark else HoloBg)
                    .padding(horizontal = 12.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                RadioButton(
                    selected = selected,
                    onClick = { model.changeVideoQuality(preset) },
                    colors = RadioButtonDefaults.colors(selectedColor = HoloAccent, unselectedColor = HoloSecondary),
                )
                Column {
                    Text(
                        preset.label,
                        color = if (selected) HoloAccent else Color.White,
                        fontSize = 15.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                    Text(
                        stringResource(R.string.settings_bitrate, "%.0f".format(preset.bitrate / 1_000_000.0)),
                        color = HoloSecondary,
                        fontSize = 12.sp,
                    )
                }
            }
        }
        Text(stringResource(R.string.settings_video_apply), color = HoloSecondary, fontSize = 12.sp)

        // Разрастание очереди — единственное, что мы уведомляем помимо сообщений.
        FieldLabel(stringResource(R.string.settings_notifications))
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.settings_outbox_warn), color = Color.White, fontSize = 15.sp)
                    Text(
                        stringResource(R.string.settings_outbox_warn_hint),
                        color = HoloSecondary,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
                Switch(
                    checked = model.warnOutboxGrowth,
                    onCheckedChange = { model.toggleOutboxWarning() },
                )
            }
        }

        // ── Футер ───────────────────────────────────────────────────────────
        FieldLabel(stringResource(R.string.settings_about))
        SettingInfoRow(stringResource(R.string.settings_version_label), stringResource(R.string.settings_version_value))
        SettingInfoRow(stringResource(R.string.settings_transport_label), stringResource(R.string.settings_transport_value))
        SettingInfoRow(stringResource(R.string.settings_encryption_label), stringResource(R.string.settings_encryption_value))
        SettingInfoRow(stringResource(R.string.settings_license_label), "GPL-3.0")
        Text(
            stringResource(R.string.settings_about_future),
            color = HoloSecondary,
            fontSize = 12.sp,
            modifier = Modifier.padding(top = 4.dp, bottom = 12.dp),
        )
    }
}

/** Профиль-шапка: аватар, имя, короткий адрес, QR. Тап — редактировать. */
@Composable
private fun ProfileHeader(model: AppModel, onEdit: () -> Unit, onMyQr: () -> Unit) {
    val me = model.me
    SurfaceCard(onClick = onEdit) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SquareAvatar(model.selfName.ifBlank { "?" }, size = 52.dp, avatarBytes = model.selfAvatar)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    model.selfName.ifBlank { stringResource(R.string.settings_no_name) },
                    color = Color.White,
                    fontSize = 17.sp,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    me?.address.orEmpty().take(24) + if ((me?.address?.length ?: 0) > 24) "…" else "",
                    color = HoloSecondary,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                    modifier = Modifier.padding(top = 2.dp),
                )
            }
            Icon(
                Icons.Filled.QrCode2,
                stringResource(R.string.net_my_qr),
                tint = HoloAccent,
                modifier = Modifier.size(24.dp).clickable { onMyQr() },
            )
        }
    }
}

/** Строка настройки: иконка, заголовок, значение под ним. */
@Composable
private fun SettingRow(
    icon: ImageVector,
    title: String,
    value: String?,
    valueColor: Color = HoloSecondary,
    onClick: (() -> Unit)? = null,
) {
    SurfaceCard(onClick = onClick) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(title, color = Color.White, fontSize = 15.sp)
                if (value != null) {
                    Text(value, color = valueColor, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
                }
            }
            if (onClick != null) {
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(18.dp))
            }
        }
    }
}

/**
 * Показ сид-фразы из настроек. Тот же путь, что в онбординге: экран под FLAG_SECURE,
 * фраза нигде не сохраняется, «Записал» отмечает резервную копию сделанной.
 */
@Composable
private fun SeedPhraseSheet(model: AppModel, onClose: () -> Unit) {
    LaunchedEffect(Unit) { model.revealSeedPhrase() }
    SecureFlag()
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            stringResource(R.string.onb_phrase_title),
            color = Color.White,
            fontSize = 19.sp,
            fontWeight = FontWeight.Medium,
        )
        Text(stringResource(R.string.onb_phrase_sub), color = HoloSecondary, fontSize = 13.sp)
        PhraseGrid(model.revealedPhrase)
        AccentButton(stringResource(R.string.onb_wrote_down)) {
            model.markKeyBackupDone()
            model.clearRevealedPhrase()
            onClose()
        }
        Text(
            stringResource(R.string.common_back),
            color = HoloAccent,
            fontSize = 14.sp,
            modifier = Modifier.clickable {
                model.clearRevealedPhrase()
                onClose()
            },
        )
    }
}

/** Строка «параметр — значение» в футере (только чтение). */
@Composable
private fun SettingInfoRow(label: String, value: String) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Color.White, fontSize = 14.sp)
        Text(value, color = HoloSecondary, fontSize = 13.sp)
    }
}

/** Редактирование профиля: имя и аватар (то же, что в онбординге). */
@Composable
internal fun EditProfileScreen(model: AppModel, onDone: () -> Unit) {
    var name by remember { mutableStateOf(model.selfName) }
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        uri ?: return@rememberLauncherForActivityResult
        scope.launch(Dispatchers.IO) {
            val mime = ctx.contentResolver.getType(uri) ?: "image/jpeg"
            val bytes = ctx.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
            model.setSelfAvatar(bytes, mime)
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(FormGap),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(Modifier.clickable { picker.launch(arrayOf("image/*")) }) {
            SquareAvatar(name.ifBlank { "?" }, size = 96.dp, avatarBytes = model.selfAvatar)
        }
        Text(stringResource(R.string.settings_tap_avatar), color = HoloSecondary, fontSize = 12.sp)

        FieldLabel(stringResource(R.string.settings_your_name))
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors(), singleLine = true)
        Text(stringResource(R.string.settings_name_hint), color = HoloSecondary, fontSize = 12.sp)

        AccentButton(stringResource(R.string.common_done)) {
            model.updateSelfName(name.trim())
            onDone()
        }
    }
}

/** Экран замка: ничего лишнего, только повод и кнопка подтверждения. */
@Composable
internal fun LockScreen(onUnlock: () -> Unit) {
    Column(
        Modifier.fillMaxSize().background(HoloBg).padding(ScreenPad),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(Icons.Filled.Lock, null, tint = HoloAccent, modifier = Modifier.size(56.dp))
        Spacer(Modifier.height(20.dp))
        Text(
            stringResource(R.string.lock_title),
            color = Color.White,
            fontSize = 19.sp,
            fontWeight = FontWeight.Medium,
        )
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.lock_subtitle), color = HoloSecondary, fontSize = 13.sp)
        Spacer(Modifier.height(28.dp))
        AccentButton(stringResource(R.string.lock_unlock), onClick = onUnlock)
    }
}
