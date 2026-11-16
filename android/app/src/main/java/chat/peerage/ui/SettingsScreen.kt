@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.CameraFront
import androidx.compose.material.icons.filled.CameraRear
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Description
import androidx.compose.material.icons.filled.GraphicEq
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Smartphone
import androidx.compose.material.icons.filled.SurroundSound
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
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
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

/** Безопасность: копия ключа, блокировка приложения, шифрование хранилища, сверка. */
@Composable
internal fun SettingsSecurityScreen(
    model: AppModel,
    onVaultLevel: () -> Unit = {},
    onContacts: () -> Unit = {},
    onKeyFlow: (wordsFirst: Boolean) -> Unit = {},
    vaultLevel: VaultLevel = VaultLevel.KEYSTORE,
) {
    val ctx = LocalContext.current
    val deviceSecure = remember { AppLock.deviceSecure(ctx) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        Spacer(Modifier.height(Tokens.Space.m))
        CardSection {
            // Копия ключа отмечается сделанной только проверкой трёх слов: «записал» без
            // проверки — обещание, а не копия.
            val facts = model.backupFacts
            SettingRow(
                Icons.Filled.Key,
                stringResource(R.string.settings_key_backup),
                when {
                    facts.keyFailed -> stringResource(R.string.settings_key_failed)
                    facts.keyDone -> stringResource(R.string.settings_key_checked, fmtDay(facts.keyCheckedAt))
                    else -> stringResource(R.string.settings_key_backup_todo)
                },
                valueColor = if (facts.keyDone && !facts.keyFailed) HoloOnline else HoloWarn,
                divider = false,
            ) { onKeyFlow(!facts.keyDone) }
            SettingRow(Icons.Filled.Check, stringResource(R.string.settings_key_check), null) { onKeyFlow(false) }
            SettingRow(Icons.Filled.Visibility, stringResource(R.string.settings_key_show), null) { onKeyFlow(true) }
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
    }
}

/**
 * Приватность (макет `settings/Privacy`): что раскрывается по выбору, что не отправляется
 * вовсе и что видят контакты. Отчёты о прочтении и набор взаимны — подпись говорит, что
 * значит выключенное. «Заблокированные» появятся с их экраном (5.2).
 */
@Composable
internal fun SettingsPrivacyScreen(model: AppModel) {
    val p = model.privacy
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        SectionTitle(stringResource(R.string.privacy_optional))
        CardSection {
            PrivacySwitch(
                stringResource(R.string.privacy_read),
                stringResource(if (p.readReceipts) R.string.privacy_read_on else R.string.privacy_read_off),
                p.readReceipts,
                divider = false,
            ) { model.setReadReceipts(it) }
            PrivacySwitch(
                stringResource(R.string.privacy_typing),
                stringResource(if (p.typing) R.string.privacy_typing_on else R.string.privacy_typing_off),
                p.typing,
            ) { model.setTypingIndicator(it) }
        }
        SectionTitle(stringResource(R.string.privacy_not_sent))
        CardSection {
            PrivacyFact(TablerIcons.Check, Tokens.Colors.online, R.string.privacy_not_sent_what, R.string.privacy_not_sent_sub, divider = false)
        }
        SectionTitle(stringResource(R.string.privacy_seen))
        CardSection {
            PrivacyFact(TablerIcons.InfoCircle, Tokens.Colors.fieldIcon, R.string.privacy_online, R.string.privacy_online_sub, divider = false)
            PrivacyFact(TablerIcons.InfoCircle, Tokens.Colors.fieldIcon, R.string.privacy_profile, R.string.privacy_profile_sub)
        }
    }
}

/** Переключатель «Приватности»: название и что значит текущее положение. */
@Composable
private fun PrivacySwitch(title: String, sub: String, checked: Boolean, divider: Boolean = true, onChange: (Boolean) -> Unit) {
    CardRow(divider = divider, inset = Tokens.Space.l, modifier = Modifier.switchRow(checked, onChange = onChange)) {
        Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
            Text(title, color = Color.White, fontSize = Tokens.Font.name)
            Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, modifier = Modifier.padding(top = Tokens.Space.xxs))
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Switch(checked = checked, onCheckedChange = null)
    }
}

/** Строка-факт «Приватности»: значок, что и короткое пояснение. Не нажимается. */
@Composable
private fun PrivacyFact(icon: ImageVector, tint: Color, title: Int, sub: Int, divider: Boolean = true) {
    CardRow(divider = divider, inset = Tokens.Space.l + Tokens.Size.iconRow + Tokens.Space.m) {
        Column(Modifier.fillMaxWidth()) {
            Row(verticalAlignment = Alignment.Top) {
                Icon(icon, null, tint = tint, modifier = Modifier.padding(top = Tokens.Space.xxs).size(Tokens.Size.iconRow))
                Spacer(Modifier.width(Tokens.Space.m))
                Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
                    Text(stringResource(title), color = Color.White, fontSize = Tokens.Font.name)
                    Text(
                        stringResource(sub),
                        color = Tokens.Colors.tertiary,
                        fontSize = Tokens.Font.body,
                        modifier = Modifier.padding(top = Tokens.Space.xxs),
                    )
                }
            }
        }
    }
}

/**
 * Медиа и звонки: значения ПО УМОЛЧАНИЮ для всех звонков. Те же элементы, что на
 * экранах «Показать видео» и «Звук» внутри звонка (call-ui.md, «Настройки по
 * умолчанию»), только без превью и без кнопки «Применить»: выбор сохраняется сразу
 * и действует со следующего звонка.
 *
 * Разрешения и 60 fps — только то, что реально отдаёт камера ([CameraCaps]):
 * раньше здесь был жёсткий список пресетов, и неподдерживаемое разрешение давало
 * чёрный экран вместо видео.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SettingsMediaScreen(model: AppModel) {
    val ctx = LocalContext.current
    val q = model.videoQuality
    val front = model.cameraFront
    val resolutions = remember(front) { CameraCaps.resolutions(ctx, front) }
    val has60 = remember(front) { CameraCaps.supports60(ctx, front) }
    // Сохранённое значение может быть недоступно этой камере (старый пресет 480p,
    // смена камеры) — тогда берём первое честное разрешение и 30 fps.
    LaunchedEffect(resolutions, has60) {
        val resOk = resolutions.isEmpty() || resolutions.any { it.width == q.width && it.height == q.height }
        val fpsOk = q.fps == 30 || has60
        if (!resOk || !fpsOk) {
            // Список бывает пуст (нет камеры, эмулятор): тогда разрешение не трогаем,
            // а `first()` здесь ронял экран, если сохранены 60 fps.
            val r = resolutions.firstOrNull { it.width == q.width && it.height == q.height } ?: resolutions.firstOrNull()
            model.setVideoDefaults(w = r?.width ?: q.width, h = r?.height ?: q.height, fps = if (fpsOk) q.fps else 30)
        }
    }
    val outs = model.availableAudioOuts()
    val ins = model.availableAudioIns()

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        SectionTitle(stringResource(R.string.media_video_section))
        CardSection {
            OptionBlock(stringResource(R.string.media_camera), divider = false) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                    OptionChip(stringResource(R.string.media_camera_front), front, Modifier.weight(1f), Icons.Filled.CameraFront) {
                        model.setVideoDefaults(front = true)
                    }
                    OptionChip(stringResource(R.string.media_camera_back), !front, Modifier.weight(1f), Icons.Filled.CameraRear) {
                        model.setVideoDefaults(front = false)
                    }
                }
            }
            OptionBlock(stringResource(R.string.share_resolution), badge = stringResource(R.string.share_available_on_device)) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                ) {
                    resolutions.forEach { r ->
                        OptionChip(r.label, q.width == r.width && q.height == r.height) {
                            model.setVideoDefaults(w = r.width, h = r.height)
                        }
                    }
                }
            }
            OptionBlock(stringResource(R.string.share_fps)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                    OptionChip(stringResource(R.string.share_fps_value, 30), q.fps == 30) { model.setVideoDefaults(fps = 30) }
                    if (has60) {
                        OptionChip(stringResource(R.string.share_fps_value, 60), q.fps == 60) { model.setVideoDefaults(fps = 60) }
                    }
                }
            }
            OptionBlock(
                stringResource(R.string.share_bitrate),
                trailing = {
                    OptionChip(stringResource(R.string.share_auto), model.videoBitrateAuto, icon = Icons.Filled.Bolt, compact = true) {
                        model.setVideoDefaults(bitrateAuto = true)
                    }
                },
            ) {
                Row(
                    Modifier.alpha(if (model.videoBitrateAuto) 0.45f else 1f),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                ) {
                    model.bitrateOptions(q.width, q.height, q.fps).forEachIndexed { i, bps ->
                        OptionChip(
                            stringResource(R.string.share_bitrate_value, fmtMbps(bps)),
                            !model.videoBitrateAuto && model.videoBitrateIdx == i,
                        ) { model.setVideoDefaults(bitrateAuto = false, bitrateIdx = i) }
                    }
                }
            }
        }

        SectionTitle(stringResource(R.string.media_audio_section))
        CardSection {
            OptionBlock(stringResource(R.string.audio_input), divider = false) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                ) {
                    if ("builtin" in ins) {
                        OptionChip(stringResource(R.string.audio_dev_builtin), model.audioIn == "builtin", icon = Icons.Filled.Smartphone) {
                            model.pickAudioIn("builtin")
                        }
                    }
                    if ("headset" in ins) {
                        OptionChip(stringResource(R.string.audio_dev_headset), model.audioIn == "headset", icon = Icons.Filled.Headset) {
                            model.pickAudioIn("headset")
                        }
                    }
                    if ("bluetooth" in ins) {
                        OptionChip(stringResource(R.string.audio_dev_bluetooth), model.audioIn == "bluetooth", icon = Icons.Filled.Bluetooth) {
                            model.pickAudioIn("bluetooth")
                        }
                    }
                }
            }
            OptionBlock(stringResource(R.string.audio_output)) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                ) {
                    if ("builtin" in outs) {
                        OptionChip(stringResource(R.string.audio_dev_builtin), model.audioOut == "builtin", icon = Icons.Filled.Smartphone) {
                            model.pickAudioOut("builtin")
                        }
                    }
                    if ("speaker" in outs) {
                        OptionChip(stringResource(R.string.audio_dev_speaker), model.audioOut == "speaker", icon = Icons.Filled.VolumeUp) {
                            model.pickAudioOut("speaker")
                        }
                    }
                    if ("bluetooth" in outs) {
                        OptionChip(stringResource(R.string.audio_dev_bluetooth), model.audioOut == "bluetooth", icon = Icons.Filled.Bluetooth) {
                            model.pickAudioOut("bluetooth")
                        }
                    }
                }
            }
            OptionBlock(
                stringResource(R.string.audio_quality),
                trailing = {
                    OptionChip(stringResource(R.string.audio_quality_auto), model.audioQualityAuto, icon = Icons.Filled.Bolt, compact = true) {
                        model.setAudioQuality(!model.audioQualityAuto)
                    }
                },
            ) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text(stringResource(R.string.audio_quality_voice), color = Tokens.Colors.secondary, fontSize = Tokens.Font.caption)
                    Text(stringResource(R.string.audio_quality_audio), color = Tokens.Colors.secondary, fontSize = Tokens.Font.caption)
                }
                Slider(
                    value = model.audioBitrate.toFloat(),
                    onValueChange = { model.setAudioQuality(false, it.roundToInt()) },
                    valueRange = AppModel.AUDIO_BITRATE_MIN.toFloat()..AppModel.AUDIO_BITRATE_MAX.toFloat(),
                    enabled = !model.audioQualityAuto,
                    // Дорожка — tertiary: цвет тайла сливается с фоном карточки, и в
                    // режиме «авто» ползунок висел в пустоте.
                    colors = SliderDefaults.colors(
                        thumbColor = Tokens.Colors.accent,
                        activeTrackColor = Tokens.Colors.accent,
                        inactiveTrackColor = Tokens.Colors.divider,
                        disabledThumbColor = Tokens.Colors.tertiary,
                        disabledActiveTrackColor = Tokens.Colors.tertiary,
                        disabledInactiveTrackColor = Tokens.Colors.divider,
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
                )
            }
        }

        SectionTitle(stringResource(R.string.audio_processing))
        CardSection {
            SettingSwitchRow(Icons.Filled.GraphicEq, stringResource(R.string.audio_ns), null, model.audioFx.ns, divider = false) {
                model.setAudioFx(ns = it)
            }
            SettingSwitchRow(Icons.Filled.SurroundSound, stringResource(R.string.audio_aec), null, model.audioFx.aec) {
                model.setAudioFx(aec = it)
            }
            SettingSwitchRow(Icons.Filled.VolumeUp, stringResource(R.string.audio_agc), null, model.audioFx.agc) {
                model.setAudioFx(agc = it)
            }
        }
        Text(
            stringResource(R.string.media_defaults_hint),
            color = Tokens.Colors.secondary,
            fontSize = Tokens.Font.small,
            modifier = Modifier.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
        )
    }
}

/**
 * Блок внутри карточки: подпись (+ значок «доступно на устройстве», + элемент справа,
 * напр. «авто») и варианты под ней. Разделитель сверху — как у строк [CardRow].
 */
@Composable
private fun OptionBlock(
    label: String,
    divider: Boolean = true,
    badge: String? = null,
    trailing: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    if (divider) {
        HorizontalDivider(
            thickness = 1.dp,
            color = Tokens.Colors.divider,
            modifier = Modifier.padding(start = Tokens.Space.m),
        )
    }
    Column(Modifier.fillMaxWidth().padding(Tokens.Space.m)) {
        Row(
            Modifier.fillMaxWidth().padding(bottom = Tokens.Space.s),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            Text(label, color = Color.White, fontSize = Tokens.Font.label)
            if (badge != null) TagBadge(badge)
            Spacer(Modifier.weight(1f))
            trailing?.invoke()
        }
        content()
    }
}

/** Уведомления: разрастание очереди — единственное, о чём мы говорим помимо сообщений. */
@Composable
internal fun SettingsNotificationsScreen(model: AppModel) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        Spacer(Modifier.height(Tokens.Space.m))
        CardSection {
            SettingSwitchRow(
                Icons.Filled.Notifications,
                stringResource(R.string.settings_outbox_warn),
                stringResource(R.string.settings_outbox_warn_hint),
                checked = model.warnOutboxGrowth,
                divider = false,
            ) { model.toggleOutboxWarning() }
        }
    }
}

/** О приложении: версия, транспорт, шифрование, лицензия. */
@Composable
internal fun SettingsAboutScreen() {
    val ctx = LocalContext.current
    // Сохранить лог: человек выбирает, куда положить файл, и сам его пересылает.
    val saveLog = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/plain")) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        Thread {
            val ok = runCatching {
                val text = chat.peerage.LogExport.collect(ctx)
                ctx.contentResolver.openOutputStream(uri)?.use { it.write(text.toByteArray()) } ?: error("no stream")
            }.isSuccess
            android.os.Handler(android.os.Looper.getMainLooper()).post {
                android.widget.Toast.makeText(
                    ctx,
                    ctx.getString(if (ok) R.string.settings_log_saved else R.string.settings_log_failed),
                    android.widget.Toast.LENGTH_SHORT,
                ).show()
            }
        }.apply { isDaemon = true }.start()
    }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        Spacer(Modifier.height(Tokens.Space.m))
        CardSection {
            SettingInfoRow(
                stringResource(R.string.settings_version_label),
                appVersion(),
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
        Spacer(Modifier.height(Tokens.Space.m))
        CardSection {
            SettingRow(
                Icons.Filled.Description,
                stringResource(R.string.settings_log_save),
                stringResource(R.string.settings_log_hint),
                divider = false,
            ) { saveLog.launch(chat.peerage.LogExport.fileName()) }
        }
        Text(
            stringResource(R.string.settings_about_future),
            color = Tokens.Colors.secondary,
            fontSize = Tokens.Font.small,
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
            Text(title, color = Color.White, fontSize = Tokens.Font.label)
            if (value != null) {
                Text(value, color = valueColor, fontSize = Tokens.Font.small, modifier = Modifier.padding(top = Tokens.Space.xxs))
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
    CardRow(divider = divider, modifier = Modifier.switchRow(checked, enabled, onCheckedChange)) {
        Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Column(Modifier.weight(1f)) {
            Text(title, color = Color.White, fontSize = Tokens.Font.label)
            if (value != null) {
                Text(value, color = valueColor, fontSize = Tokens.Font.small, modifier = Modifier.padding(top = Tokens.Space.xxs))
            }
        }
        Switch(checked = checked, enabled = enabled, onCheckedChange = null)
    }
}

/** Строка «параметр — значение» в футере (только чтение). */
@Composable
private fun SettingInfoRow(label: String, value: String, divider: Boolean = true) {
    // Справочная строка (версия, транспорт, лицензия) живёт в той же карточке, что и
    // настраиваемое, но без иконки — разделитель у неё идёт во всю ширину.
    CardRow(divider = divider, inset = 0.dp) {
        Text(label, color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
        Text(value, color = Tokens.Colors.secondary, fontSize = Tokens.Font.body)
    }
}

/**
 * Свой профиль (макет `people/MeEdit`): фото, имя, «Убрать фото»; «Готово» в шапке. Новое фото
 * и снятие ждут «Готово» — до него контакты видят прежнее, как у правки группы.
 */
@Composable
internal fun EditProfileScreen(model: AppModel, onDone: () -> Unit) {
    var name by remember { mutableStateOf(model.selfName) }
    var photo by remember { mutableStateOf<ByteArray?>(null) }
    var removed by remember { mutableStateOf(false) }
    val pick = rememberImagePicker {
        photo = it
        removed = false
    }
    val shown = if (removed) null else photo ?: model.selfAvatar
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        TopBar(stringResource(R.string.settings_edit_profile), onBack = onDone) {
            PillButton(stringResource(R.string.common_done), modifier = Modifier.padding(end = Tokens.Space.xs)) {
                if (name.trim() != model.selfName) model.updateSelfName(name.trim())
                when {
                    photo != null -> model.setSelfAvatar(photo!!, "image/jpeg")
                    removed -> model.clearSelfAvatar()
                }
                onDone()
            }
        }
        Column(
            Modifier.weight(1f).fillMaxWidth()
                .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                .background(Tokens.Colors.panel)
                .verticalScroll(rememberScrollState()),
        ) {
            PhotoEditAvatar(
                name.ifBlank { "?" },
                shown,
                circle = true,
                label = stringResource(R.string.settings_tap_avatar),
                onPick = pick,
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
            SectionTitle(stringResource(R.string.me_edit_name), Modifier.padding(top = 0.dp))
            FormInput(name, "", Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth()) { name = it }
            Caption(stringResource(R.string.me_edit_caption))
            if (shown != null) {
                Spacer(Modifier.height(Tokens.Space.l))
                CardSection {
                    NavRow(TablerIcons.Trash, stringResource(R.string.me_remove_photo), divider = false) {
                        photo = null
                        removed = true
                    }
                }
            }
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
        Icon(Icons.Filled.Lock, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconHero))
        Spacer(Modifier.height(Tokens.Space.xl))
        Text(
            stringResource(R.string.lock_title),
            color = Color.White,
            fontSize = Tokens.Font.title,
            fontWeight = FontWeight.Medium,
        )
        Spacer(Modifier.height(Tokens.Space.s))
        Text(stringResource(R.string.lock_subtitle), color = HoloSecondary, fontSize = Tokens.Font.body)
        Spacer(Modifier.height(Tokens.Space.xxl))
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
        Icon(Icons.Filled.Shield, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconHero))
        Spacer(Modifier.height(Tokens.Space.l))
        Text(
            stringResource(R.string.vault_unlock_title),
            color = Color.White,
            fontSize = Tokens.Font.title,
            fontWeight = FontWeight.Medium,
        )
        Spacer(Modifier.height(Tokens.Space.s))
        Text(
            stringResource(R.string.vault_unlock_sub),
            color = HoloSecondary,
            fontSize = Tokens.Font.body,
        )
        Spacer(Modifier.height(Tokens.Space.xl))
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
            keyboardOptions = PasswordKeyboard,
        )
        if (wrong) {
            Spacer(Modifier.height(Tokens.Space.s))
            Text(stringResource(R.string.vault_wrong), color = HoloError, fontSize = Tokens.Font.body)
        }
        Spacer(Modifier.height(Tokens.Space.l))
        if (busy) {
            // Argon2id считается ~секунду: без индикатора это выглядит зависанием.
            CircularProgressIndicator(color = HoloAccent, modifier = Modifier.size(28.dp), strokeWidth = 2.dp)
            Spacer(Modifier.height(Tokens.Space.s))
            Text(stringResource(R.string.vault_deriving), color = HoloSecondary, fontSize = Tokens.Font.small)
        } else {
            AccentButton(stringResource(R.string.vault_unlock_btn), enabled = password.isNotEmpty()) {
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
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
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
                keyboardOptions = PasswordKeyboard,
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
                keyboardOptions = PasswordKeyboard,
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
                keyboardOptions = PasswordKeyboard,
            )
            if (repeat.isNotEmpty() && repeat != newPassword) {
                Text(stringResource(R.string.vault_mismatch), color = HoloError, fontSize = Tokens.Font.small)
            }
            Text(stringResource(R.string.vault_no_recovery), color = HoloWarn, fontSize = Tokens.Font.small)
        }
        if (failed) {
            Text(stringResource(R.string.vault_change_failed), color = HoloError, fontSize = Tokens.Font.body)
        }

        val ready = when {
            busy -> false
            target == VaultLevel.KEYSTORE -> current != VaultLevel.KEYSTORE
            else -> newPassword.isNotEmpty() && newPassword == repeat
        }
        if (busy) {
            Text(stringResource(R.string.vault_deriving), color = HoloSecondary, fontSize = Tokens.Font.body)
        } else {
            // Неготовая кнопка — серая, а не молча глухая: иначе нажатие ничего не
            // делало и не объясняло почему.
            AccentButton(stringResource(R.string.vault_apply), enabled = ready) {
                busy = true
                failed = false
                scope.launch {
                    // Argon2id — на IO, а итог — на главном: он уводит с экрана, а
                    // навигацию из фонового потока трогать нельзя (инвариант 4).
                    val ok = withContext(Dispatchers.IO) {
                        VaultKeyManager.changeLevel(
                            dir,
                            target,
                            oldPassword.ifEmpty { null },
                            newPassword.ifEmpty { null },
                        )
                    }
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
            Spacer(Modifier.width(Tokens.Space.s))
            Column {
                Text(title, color = Color.White, fontSize = Tokens.Font.label)
                Text(desc, color = HoloSecondary, fontSize = Tokens.Font.small, modifier = Modifier.padding(top = Tokens.Space.xxs))
            }
        }
    }
}
