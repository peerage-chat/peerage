@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
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
    onVaultLevel: () -> Unit = {},
    vaultLevel: VaultLevel = VaultLevel.KEYSTORE,
) {
    var showPhrase by remember { mutableStateOf(false) }
    if (showPhrase) {
        SeedPhraseSheet(model) { showPhrase = false }
        return
    }

    val ctx = LocalContext.current
    val deviceSecure = remember { AppLock.deviceSecure(ctx) }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        // Профиля здесь больше нет: он живёт на экране «Я», а держать его шапкой и
        // тут значило бы иметь два места, где меняют одно и то же. Здесь — только
        // то, что настраивается.

        // ── Безопасность ────────────────────────────────────────────────────
        SectionTitle(stringResource(R.string.settings_security))
        CardSection {
            SettingRow(
                Icons.Filled.Key,
                stringResource(R.string.settings_key_backup),
                if (model.keyBackupDone) {
                    stringResource(R.string.settings_key_backup_done)
                } else {
                    stringResource(R.string.settings_key_backup_todo)
                },
                valueColor = if (model.keyBackupDone) HoloOnline else HoloWarn,
                divider = false,
            ) { showPhrase = true }
            // Блокировка: подтверждение — системное (биометрия/PIN устройства), своего
            // секрета мы не храним. Без экрана блокировки на устройстве включать нечего.
            SettingSwitchRow(
                Icons.Filled.Lock,
                stringResource(R.string.settings_app_lock),
                stringResource(
                    if (deviceSecure) R.string.settings_app_lock_desc else R.string.settings_app_lock_no_device,
                ),
                checked = AppLock.enabled,
                enabled = deviceSecure,
                valueColor = if (deviceSecure) Tokens.Colors.secondary else HoloWarn,
            ) { AppLock.setEnabled(ctx, it) }
            SettingRow(
                Icons.Filled.Shield,
                stringResource(R.string.settings_at_rest),
                stringResource(
                    when (vaultLevel) {
                        VaultLevel.KEYSTORE -> R.string.vault_lvl_keystore
                        VaultLevel.PASSWORD -> R.string.vault_lvl_password
                        VaultLevel.BOTH -> R.string.vault_lvl_both
                    },
                ),
                onClick = onVaultLevel,
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
        }

        // ── Профиль и данные ────────────────────────────────────────────────
        SectionTitle(stringResource(R.string.settings_title_profile_data))
        CardSection {
            SettingRow(
                Icons.Filled.Person,
                stringResource(R.string.settings_edit_profile),
                null,
                divider = false,
                onClick = onEditProfile,
            )
            SettingRow(Icons.Filled.Share, stringResource(R.string.settings_net_battery), null, onClick = onNetwork)
            SettingRow(Icons.Filled.Storage, stringResource(R.string.menu_storage), null, onClick = onStorage)
        }

        // ── Качество видео ──────────────────────────────────────────────────
        // Пресеты внутри одной карточки: раньше каждый был отдельной плашкой на
        // фоне экрана и читался как самостоятельная кнопка, хотя это один выбор.
        SectionTitle(stringResource(R.string.settings_video_quality))
        CardSection {
            VideoQuality.presets.forEachIndexed { i, preset ->
                val selected = model.videoQuality == preset
                CardRow(onClick = { model.changeVideoQuality(preset) }, divider = i > 0) {
                    RadioButton(
                        selected = selected,
                        onClick = { model.changeVideoQuality(preset) },
                        colors = RadioButtonDefaults.colors(
                            selectedColor = HoloAccent,
                            unselectedColor = Tokens.Colors.tertiary,
                        ),
                    )
                    Spacer(Modifier.width(Tokens.Space.s))
                    Column(Modifier.weight(1f)) {
                        Text(
                            preset.label,
                            color = if (selected) HoloAccent else Color.White,
                            fontSize = 15.sp,
                            fontFamily = FontFamily.Monospace,
                        )
                        Text(
                            stringResource(R.string.settings_bitrate, "%.0f".format(preset.bitrate / 1_000_000.0)),
                            color = Tokens.Colors.secondary,
                            fontSize = 12.sp,
                        )
                    }
                }
            }
        }
        Text(
            stringResource(R.string.settings_video_apply),
            color = Tokens.Colors.secondary,
            fontSize = 12.sp,
            modifier = Modifier.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.xs),
        )

        // ── Уведомления ─────────────────────────────────────────────────────
        // Разрастание очереди — единственное, о чём мы уведомляем помимо сообщений.
        SectionTitle(stringResource(R.string.settings_notifications))
        CardSection {
            SettingSwitchRow(
                Icons.Filled.Notifications,
                stringResource(R.string.settings_outbox_warn),
                stringResource(R.string.settings_outbox_warn_hint),
                checked = model.warnOutboxGrowth,
                divider = false,
            ) { model.toggleOutboxWarning() }
        }

        // ── О приложении ────────────────────────────────────────────────────
        SectionTitle(stringResource(R.string.settings_about))
        CardSection {
            SettingInfoRow(
                stringResource(R.string.settings_version_label),
                stringResource(R.string.settings_version_value),
                divider = false,
            )
            SettingInfoRow(
                stringResource(R.string.settings_transport_label),
                stringResource(R.string.settings_transport_value),
            )
            SettingInfoRow(
                stringResource(R.string.settings_encryption_label),
                stringResource(R.string.settings_encryption_value),
            )
            SettingInfoRow(stringResource(R.string.settings_license_label), "GPL-3.0")
        }
        Text(
            stringResource(R.string.settings_about_future),
            color = Tokens.Colors.secondary,
            fontSize = 12.sp,
            modifier = Modifier.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
        )
    }
}

/** Строка настройки внутри карточки-секции: иконка, заголовок, значение под ним. */
@Composable
private fun SettingRow(
    icon: ImageVector,
    title: String,
    value: String?,
    valueColor: Color = Tokens.Colors.secondary,
    divider: Boolean = true,
    onClick: (() -> Unit)? = null,
) {
    CardRow(onClick = onClick, divider = divider) {
        Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Text(title, color = Color.White, fontSize = 15.sp)
            if (value != null) {
                Text(value, color = valueColor, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
            }
        }
        if (onClick != null) {
            Icon(
                Icons.Filled.ChevronRight,
                null,
                tint = Tokens.Colors.tertiary,
                modifier = Modifier.size(Tokens.Size.iconS),
            )
        }
    }
}

/** Строка-тумблер: тот же вид, что у обычной строки, но справа переключатель. */
@Composable
private fun SettingSwitchRow(
    icon: ImageVector,
    title: String,
    value: String?,
    checked: Boolean,
    enabled: Boolean = true,
    valueColor: Color = Tokens.Colors.secondary,
    divider: Boolean = true,
    onCheckedChange: (Boolean) -> Unit,
) {
    CardRow(divider = divider) {
        Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Text(title, color = Color.White, fontSize = 15.sp)
            if (value != null) {
                Text(value, color = valueColor, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
            }
        }
        Switch(checked = checked, enabled = enabled, onCheckedChange = onCheckedChange)
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
private fun SettingInfoRow(label: String, value: String, divider: Boolean = true) {
    // Справочная строка (версия, транспорт, лицензия) живёт в той же карточке, что и
    // настраиваемое, но без иконки — разделитель у неё идёт во всю ширину.
    CardRow(divider = divider, inset = 0.dp) {
        Text(label, color = Color.White, fontSize = 14.sp, modifier = Modifier.weight(1f))
        Text(value, color = Tokens.Colors.secondary, fontSize = 13.sp)
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

/**
 * Ввод пароля хранилища при холодном старте (уровни PASSWORD/BOTH).
 *
 * Пока пароль не введён, движок не стартует вовсе: данные лежат зашифрованными, и
 * показывать «пустой мессенджер» вместо запроса было бы враньём про состояние.
 */
@Composable
internal fun VaultUnlockScreen(onSubmit: (String, (Boolean) -> Unit) -> Unit) {
    var password by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var wrong by remember { mutableStateOf(false) }

    Column(
        Modifier.fillMaxSize().background(HoloBg).padding(ScreenPad),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(Icons.Filled.Shield, null, tint = HoloAccent, modifier = Modifier.size(48.dp))
        Spacer(Modifier.height(18.dp))
        Text(
            stringResource(R.string.vault_unlock_title),
            color = Color.White,
            fontSize = 19.sp,
            fontWeight = FontWeight.Medium,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.vault_unlock_sub),
            color = HoloSecondary,
            fontSize = 13.sp,
        )
        Spacer(Modifier.height(22.dp))
        TextField(
            password,
            {
                password = it
                wrong = false
            },
            Modifier.fillMaxWidth(),
            colors = inputColors(),
            singleLine = true,
            enabled = !busy,
            visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
        )
        if (wrong) {
            Spacer(Modifier.height(8.dp))
            Text(stringResource(R.string.vault_wrong), color = HoloError, fontSize = 13.sp)
        }
        Spacer(Modifier.height(18.dp))
        if (busy) {
            // Argon2id считается ~секунду: без индикатора это выглядит зависанием.
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
            Spacer(Modifier.height(8.dp))
            Text(stringResource(R.string.vault_deriving), color = HoloSecondary, fontSize = 12.sp)
        } else {
            AccentButton(stringResource(R.string.vault_unlock_btn)) {
                if (password.isEmpty()) return@AccentButton
                busy = true
                onSubmit(password) { ok ->
                    busy = false
                    wrong = !ok
                }
            }
        }
    }
}

/**
 * Выбор уровня шифрования данных (at-rest).
 *
 * Смена уровня пере-оборачивает тот же ключ хранилища, поэтому переписка не
 * перешифровывается и ничего не теряется — кроме случая, когда забыт пароль:
 * об этом сказано прямо, потому что восстановления нет и быть не может.
 */
@Composable
internal fun VaultLevelScreen(model: AppModel, dataDir: String, onDone: () -> Unit) {
    val dir = java.io.File(dataDir)
    var target by remember { mutableStateOf(VaultKeyManager.level(dir)) }
    val current = remember { VaultKeyManager.level(dir) }
    var oldPassword by remember { mutableStateOf("") }
    var newPassword by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        LevelOption(
            VaultLevel.KEYSTORE,
            target,
            stringResource(R.string.vault_lvl_keystore),
            stringResource(R.string.vault_lvl_keystore_desc),
        ) { target = it }
        LevelOption(
            VaultLevel.PASSWORD,
            target,
            stringResource(R.string.vault_lvl_password),
            stringResource(R.string.vault_lvl_password_desc),
        ) { target = it }
        LevelOption(
            VaultLevel.BOTH,
            target,
            stringResource(R.string.vault_lvl_both),
            stringResource(R.string.vault_lvl_both_desc),
        ) { target = it }

        if (current != VaultLevel.KEYSTORE) {
            FieldLabel(stringResource(R.string.vault_current_password))
            TextField(
                oldPassword,
                { oldPassword = it },
                Modifier.fillMaxWidth(),
                colors = inputColors(),
                singleLine = true,
                placeholder = { Text(stringResource(R.string.vault_ph_current), color = HoloSecondary) },
                visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
            )
        }
        if (target != VaultLevel.KEYSTORE) {
            FieldLabel(stringResource(R.string.vault_new_password))
            // Плейсхолдеры обязательны: два одинаковых поля подряд без подписи
            // неразличимы, а пустое выглядит как разделитель, а не как поле.
            TextField(
                newPassword,
                { newPassword = it },
                Modifier.fillMaxWidth(),
                colors = inputColors(),
                singleLine = true,
                placeholder = { Text(stringResource(R.string.vault_ph_password), color = HoloSecondary) },
                visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
            )
            TextField(
                repeat,
                { repeat = it },
                Modifier.fillMaxWidth(),
                colors = inputColors(),
                singleLine = true,
                placeholder = { Text(stringResource(R.string.vault_ph_repeat), color = HoloSecondary) },
                isError = repeat.isNotEmpty() && repeat != newPassword,
                visualTransformation = androidx.compose.ui.text.input.PasswordVisualTransformation(),
            )
            if (repeat.isNotEmpty() && repeat != newPassword) {
                Text(stringResource(R.string.vault_mismatch), color = HoloError, fontSize = 12.sp)
            }
            Text(stringResource(R.string.vault_no_recovery), color = HoloWarn, fontSize = 12.sp)
        }
        if (failed) {
            Text(stringResource(R.string.vault_change_failed), color = HoloError, fontSize = 13.sp)
        }

        val ready = when {
            busy -> false
            target == VaultLevel.KEYSTORE -> current != VaultLevel.KEYSTORE
            else -> newPassword.isNotEmpty() && newPassword == repeat
        }
        if (busy) {
            Text(stringResource(R.string.vault_deriving), color = HoloSecondary, fontSize = 13.sp)
        } else {
            AccentButton(stringResource(R.string.vault_apply)) {
                if (!ready) return@AccentButton
                busy = true
                failed = false
                scope.launch(Dispatchers.IO) {
                    val ok = VaultKeyManager.changeLevel(
                        dir,
                        target,
                        oldPassword.ifEmpty { null },
                        newPassword.ifEmpty { null },
                    )
                    busy = false
                    if (ok) onDone() else failed = true
                }
            }
        }
    }
}

@Composable
private fun LevelOption(
    value: VaultLevel,
    selected: VaultLevel,
    title: String,
    desc: String,
    onPick: (VaultLevel) -> Unit,
) {
    SurfaceCard(onClick = { onPick(value) }) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            RadioButton(
                selected = selected == value,
                onClick = { onPick(value) },
                colors = RadioButtonDefaults.colors(selectedColor = HoloAccent, unselectedColor = HoloSecondary),
            )
            Spacer(Modifier.width(8.dp))
            Column {
                Text(title, color = Color.White, fontSize = 15.sp)
                Text(desc, color = HoloSecondary, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
            }
        }
    }
}
