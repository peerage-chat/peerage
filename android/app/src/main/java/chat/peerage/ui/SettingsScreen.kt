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
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Verified
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
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.launch

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
            // проверки — обещание, а не копия. Строка копии всегда ведёт к словам (оттуда —
            // «Проверить запись»), «Проверить ключ» — проверка по памяти без показа: двух
            // строк на одно действие нет (п. 28).
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
            ) { onKeyFlow(true) }
            SettingRow(Icons.Filled.Check, stringResource(R.string.settings_key_check), null) { onKeyFlow(false) }
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
 * значит выключенное. Последняя группа — вход в «Заблокированные» (5.2).
 */
@Composable
internal fun SettingsPrivacyScreen(model: AppModel, onBlocked: () -> Unit) {
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
            PrivacySwitch(
                stringResource(R.string.privacy_in_app),
                stringResource(if (p.inApp) R.string.privacy_in_app_on else R.string.privacy_in_app_off),
                p.inApp,
            ) { model.setInAppStatus(it) }
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
        // Чаты в системном меню «Поделиться» (5.6): имя и фото чата уходят системе — только по выбору.
        SectionTitle(stringResource(R.string.privacy_system))
        CardSection {
            PrivacySwitch(
                stringResource(R.string.privacy_share_chats),
                stringResource(R.string.privacy_share_chats_sub),
                model.shareChatsInMenu,
                divider = false,
                icon = TablerIcons.Share,
            ) { model.setShareChats(it) }
        }
        Caption(
            if (model.shareChatsInMenu) {
                stringResource(R.string.privacy_share_chats_on)
            } else {
                stringResource(R.string.privacy_share_chats_off, stringResource(R.string.app_name))
            },
        )
        Spacer(Modifier.height(Tokens.Space.l))
        CardSection {
            ValueRow(
                stringResource(R.string.blocked_title),
                model.blocked.size.toString(),
                divider = false,
                chevron = true,
                onClick = onBlocked,
            )
        }
    }
}

/** Переключатель «Приватности»: название и что значит текущее положение. */
@Composable
private fun PrivacySwitch(
    title: String,
    sub: String,
    checked: Boolean,
    divider: Boolean = true,
    icon: ImageVector? = null,
    onChange: (Boolean) -> Unit,
) {
    CardRow(divider = divider, inset = Tokens.Space.l, modifier = Modifier.switchRow(checked, onChange = onChange)) {
        if (icon != null) {
            Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconRow))
            Spacer(Modifier.width(Tokens.Space.m))
        }
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
 * Медиа и звонки (макет `settings/Media`): значения ПО УМОЛЧАНИЮ для всех звонков.
 * Выбор сохраняется сразу и действует со следующего звонка; во время звонка те же
 * настройки — на экранах «Показать видео» и «Звук» (call-ui.md, «Настройки по умолчанию»).
 * Разрешение, fps и битрейт — на подэкране [SettingsVideoQualityScreen].
 */
@Composable
internal fun SettingsMediaScreen(model: AppModel, onVideoQuality: () -> Unit) {
    val ctx = LocalContext.current
    VideoCapsGuard(model)
    var picker by remember { mutableStateOf<AudioPicker?>(null) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        SectionTitle(stringResource(R.string.media_video_section))
        CardSection {
            SegmentLabel(stringResource(R.string.media_camera))
            Segmented(
                listOf(stringResource(R.string.media_camera_front), stringResource(R.string.media_camera_back)),
                if (model.cameraFront) 0 else 1,
                Modifier.padding(start = Tokens.Space.m, end = Tokens.Space.m, bottom = Tokens.Space.m),
            ) { model.setVideoDefaults(front = it == 0) }
            ValueRow(stringResource(R.string.media_video_quality), videoQualityValue(model), chevron = true, onClick = onVideoQuality)
        }

        SectionTitle(stringResource(R.string.media_audio_section))
        CardSection {
            val ins = model.availableAudioIns()
            val outs = model.availableAudioOuts()
            ValueRow(
                stringResource(R.string.audio_input),
                ctx.getString(audioDevLabel(model.audioIn)),
                divider = false,
                onClick = if (ins.size > 1) ({ picker = AudioPicker.IN }) else null,
            )
            ValueRow(
                stringResource(R.string.audio_output),
                ctx.getString(audioDevLabel(model.audioOut)),
                onClick = if (outs.size > 1) ({ picker = AudioPicker.OUT }) else null,
            )
            HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = Tokens.Space.l))
            SegmentLabel(stringResource(R.string.audio_quality))
            AudioModePicker(model, Modifier.padding(horizontal = Tokens.Space.m))
            Spacer(Modifier.height(Tokens.Space.m))
        }

        SectionTitle(stringResource(R.string.audio_processing))
        CardSection {
            PlainSwitchRow(stringResource(R.string.audio_ns), model.audioFx.ns, divider = false) { model.setAudioFx(ns = it) }
            PlainSwitchRow(stringResource(R.string.audio_aec), model.audioFx.aec) { model.setAudioFx(aec = it) }
            PlainSwitchRow(stringResource(R.string.audio_agc), model.audioFx.agc) { model.setAudioFx(agc = it) }
        }
        Text(
            stringResource(R.string.media_defaults_hint),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(start = Tokens.Space.panelText, end = Tokens.Space.panelText, top = Tokens.Space.m),
        )
    }
    picker?.let { p ->
        val inPick = p == AudioPicker.IN
        val devs = if (inPick) model.availableAudioIns() else model.availableAudioOuts()
        val current = if (inPick) model.audioIn else model.audioOut
        SheetFrame(onClose = { picker = null }) {
            Text(
                stringResource(if (inPick) R.string.audio_input else R.string.audio_output),
                color = Color.White,
                fontSize = Tokens.Font.sheet,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs),
            )
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.raised)) {
                devs.forEachIndexed { i, dev ->
                    CardRow(
                        onClick = {
                            if (inPick) model.pickAudioIn(dev) else model.pickAudioOut(dev)
                            picker = null
                        },
                        divider = i > 0,
                        inset = Tokens.Space.l,
                    ) {
                        Text(stringResource(audioDevLabel(dev)), color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f))
                        if (dev == current) Icon(TablerIcons.Check, null, tint = Tokens.Colors.accent, modifier = Modifier.size(Tokens.Size.iconRow))
                    }
                }
            }
        }
    }
}

private enum class AudioPicker { IN, OUT }

/** Подпись устройства звука по его коду. */
private fun audioDevLabel(dev: String): Int = when (dev) {
    "headset" -> R.string.audio_dev_headset
    "bluetooth" -> R.string.audio_dev_bluetooth
    "speaker" -> R.string.audio_dev_speaker
    else -> R.string.audio_dev_builtin
}

/** «Авто · до 1080p 30» или «720p 30 · 2 Мбит/с» — как стоит качество видео. */
@Composable
private fun videoQualityValue(model: AppModel): String {
    val q = model.videoQuality
    val res = "${q.height}p"
    return if (model.videoBitrateAuto) {
        stringResource(R.string.media_video_auto, res, q.fps)
    } else {
        val bps = model.bitrateOptions(q.width, q.height, q.fps).getOrNull(model.videoBitrateIdx) ?: q.bitrate
        stringResource(R.string.media_video_fixed, res, q.fps, fmtMbps(bps))
    }
}

/**
 * Сохранённое качество может быть недоступно этой камере (старый пресет, смена камеры) —
 * тогда первое честное разрешение и 30 fps. Список бывает пуст (нет камеры, эмулятор):
 * тогда разрешение не трогаем.
 */
@Composable
private fun VideoCapsGuard(model: AppModel) {
    val ctx = LocalContext.current
    val q = model.videoQuality
    val front = model.cameraFront
    val resolutions = remember(front) { CameraCaps.resolutions(ctx, front) }
    val has60 = remember(front) { CameraCaps.supports60(ctx, front) }
    LaunchedEffect(resolutions, has60) {
        val resOk = resolutions.isEmpty() || resolutions.any { it.width == q.width && it.height == q.height }
        val fpsOk = q.fps == 30 || has60
        if (!resOk || !fpsOk) {
            val r = resolutions.firstOrNull { it.width == q.width && it.height == q.height } ?: resolutions.firstOrNull()
            model.setVideoDefaults(w = r?.width ?: q.width, h = r?.height ?: q.height, fps = if (fpsOk) q.fps else 30)
        }
    }
}

/**
 * Качество видео по умолчанию: разрешения и 60 fps — только то, что реально отдаёт камера
 * ([CameraCaps]); жёсткий список пресетов давал чёрный экран на неподдерживаемом разрешении.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SettingsVideoQualityScreen(model: AppModel) {
    val ctx = LocalContext.current
    VideoCapsGuard(model)
    val q = model.videoQuality
    val front = model.cameraFront
    val resolutions = remember(front) { CameraCaps.resolutions(ctx, front) }
    val has60 = remember(front) { CameraCaps.supports60(ctx, front) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = Tokens.Space.m, bottom = Tokens.Space.xxl),
    ) {
        CardSection {
            OptionBlock(stringResource(R.string.share_resolution), divider = false) {
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
    }
}

/** Качество звука: Авто / Голос / Аудио и что они значат. Один на настройки и звонок. */
@Composable
internal fun AudioModePicker(model: AppModel, modifier: Modifier = Modifier) {
    Column(modifier) {
        Segmented(
            listOf(
                stringResource(R.string.audio_quality_auto),
                stringResource(R.string.audio_quality_voice),
                stringResource(R.string.audio_quality_audio),
            ),
            model.audioMode.ordinal,
        ) { model.pickAudioMode(AudioMode.entries[it]) }
        Text(
            stringResource(
                R.string.audio_mode_caption,
                AppModel.AUDIO_BITRATE_MIN / 1000,
                AudioMode.AUTO.bitrate / 1000,
                AudioMode.VOICE.bitrate / 1000,
                AudioMode.MUSIC.bitrate / 1000,
            ),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(top = Tokens.Space.s, start = Tokens.Space.xxs, end = Tokens.Space.xxs),
        )
    }
}

/** Подпись над сегментами внутри группы. */
@Composable
private fun SegmentLabel(text: String) {
    Text(
        text,
        color = Color.White,
        fontSize = Tokens.Font.option,
        modifier = Modifier.padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.m, bottom = Tokens.Space.s),
    )
}

/** Строка «название — значение» (+ шеврон, если ведёт на экран). Без [onClick] не нажимается. */
@Composable
private fun ValueRow(title: String, value: String, divider: Boolean = true, chevron: Boolean = false, onClick: (() -> Unit)? = null) {
    CardRow(onClick = onClick, divider = divider, inset = Tokens.Space.l) {
        Text(title, color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f))
        Spacer(Modifier.width(Tokens.Space.m))
        Text(value, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.option, maxLines = 1)
        if (chevron) {
            Spacer(Modifier.width(Tokens.Space.s))
            Icon(TablerIcons.ChevronRight, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconS))
        }
    }
}

/** Тумблер без значка: название и переключатель, нажимается вся строка. */
@Composable
private fun PlainSwitchRow(title: String, checked: Boolean, divider: Boolean = true, onChange: (Boolean) -> Unit) {
    CardRow(divider = divider, inset = Tokens.Space.l, modifier = Modifier.switchRow(checked, onChange = onChange)) {
        Text(title, color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = null)
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

/**
 * Уведомления (макет `settings/Notifications`). Строки каналов ведут в системные настройки
 * канала (звуки системные, решение 9) — поэтому стрелка «наружу», а не шеврон макета; подпись —
 * то, что в канале сейчас. «Звуки в чате» — решение 2026-11-22, после макета. Строки «Звонки»,
 * «Запросы и приглашения», «Передача файлов» — вместе с самими уведомлениями (шаг 7b).
 */
@Composable
internal fun SettingsNotificationsScreen(model: AppModel) {
    val ctx = LocalContext.current
    // Каналы меняются в системных настройках — перечитать, когда экран снова виден.
    var resumed by remember { mutableIntStateOf(0) }
    val owner = androidx.compose.ui.platform.LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val obs = androidx.lifecycle.LifecycleEventObserver { _, e ->
            if (e == androidx.lifecycle.Lifecycle.Event.ON_RESUME) resumed++
        }
        owner.lifecycle.addObserver(obs)
        onDispose { owner.lifecycle.removeObserver(obs) }
    }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.xxl),
    ) {
        SectionTitle(stringResource(R.string.notif_sec_messages))
        CardSection {
            listOf(
                MessageNotifier.CH_DM to R.string.notif_ch_dm,
                MessageNotifier.CH_GROUP to R.string.notif_ch_group,
                MessageNotifier.CH_MENTION to R.string.notif_ch_mention,
            ).forEachIndexed { i, (id, name) ->
                ExternalRow(stringResource(name), remember(resumed) { channelState(ctx, id) }, divider = i > 0) {
                    MessageNotifier.ensureChannels(ctx)
                    ctx.startActivity(ConnService.channelSettings(ctx, id))
                }
            }
            PrivacySwitch(stringResource(R.string.notif_set_text), stringResource(R.string.notif_set_text_sub), model.notifyText) {
                model.setNotifyTextOn(it)
            }
            PlainSwitchRow(stringResource(R.string.notif_set_chat_sounds), model.chatSounds) { model.setChatSoundsOn(it) }
        }
        SectionTitle(stringResource(R.string.notif_sec_other))
        CardSection {
            PlainSwitchRow(stringResource(R.string.notif_ch_requests), model.notifyRequests, divider = false) { model.setNotifyRequestsOn(it) }
            PlainSwitchRow(stringResource(R.string.notif_set_flood), model.notifyFlood) { model.setFloodNotify(it) }
            PlainSwitchRow(stringResource(R.string.settings_outbox_warn), model.warnOutboxGrowth) { model.toggleOutboxWarning() }
        }
        Spacer(Modifier.height(Tokens.Space.l))
        CardSection {
            ExternalRow(stringResource(R.string.notif_set_conn), stringResource(R.string.notif_set_conn_sub), divider = false) {
                ctx.startActivity(ConnService.channelSettings(ctx))
            }
        }
    }
}

/** Строка, уводящая в системные настройки: название, подпись, стрелка «наружу». */
@Composable
private fun ExternalRow(title: String, sub: String, divider: Boolean, onClick: () -> Unit) {
    CardRow(onClick = onClick, divider = divider, inset = Tokens.Space.l) {
        Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
            Text(title, color = Color.White, fontSize = Tokens.Font.name)
            Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, modifier = Modifier.padding(top = Tokens.Space.xxs))
        }
        Spacer(Modifier.width(Tokens.Space.m))
        Icon(TablerIcons.ArrowUpRight, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconS))
    }
}

/** Что в канале сейчас: «Звук · вибрация», «Без звука · без вибрации», «Выключено». */
private fun channelState(ctx: android.content.Context, id: String): String {
    if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.O) return ctx.getString(R.string.notif_ch_sound)
    MessageNotifier.ensureChannels(ctx)
    val ch = ctx.getSystemService(android.app.NotificationManager::class.java).getNotificationChannel(id)
        ?: return ctx.getString(R.string.notif_ch_sound)
    if (ch.importance == android.app.NotificationManager.IMPORTANCE_NONE) return ctx.getString(R.string.notif_ch_off)
    val sound = ctx.getString(if (ch.sound != null && ch.importance >= android.app.NotificationManager.IMPORTANCE_DEFAULT) R.string.notif_ch_sound else R.string.notif_ch_silent)
    val vibe = ctx.getString(if (ch.shouldVibrate()) R.string.notif_ch_vibrate else R.string.notif_ch_novibrate)
    return "$sound · $vibe"
}

/**
 * О приложении (макет `settings/About`): знак, имя и версия над панелью; транспорт,
 * шифрование, лицензия; «Сохранить лог». Шапка своя: знак стоит на фоне, не на панели.
 */
@Composable
internal fun SettingsAboutScreen(onBack: () -> Unit, onWhatsNew: () -> Unit, onPolicy: () -> Unit) {
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
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        TopBar(stringResource(R.string.settings_cat_about), onBack = onBack)
        Column(
            Modifier.fillMaxWidth().padding(top = Tokens.Space.l, bottom = Tokens.Space.xl),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Box(
                Modifier.size(Tokens.Size.aboutTile).clip(RoundedCornerShape(Tokens.Radius.aboutTile)).background(Tokens.Colors.panel),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    androidx.compose.ui.res.painterResource(R.drawable.ic_glyph),
                    null,
                    tint = Tokens.Colors.accent,
                    modifier = Modifier.size(Tokens.Size.aboutGlyph),
                )
            }
            Spacer(Modifier.height(Tokens.Space.m))
            Text(
                stringResource(R.string.app_name),
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
            )
            Text(
                stringResource(R.string.settings_version, appVersion()),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = Tokens.Space.xs),
            )
        }
        Column(
            Modifier.weight(1f).fillMaxWidth()
                .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                .background(Tokens.Colors.panel)
                .verticalScroll(rememberScrollState())
                .padding(top = Tokens.Space.panel, bottom = Tokens.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            CardSection {
                ValueRow(stringResource(R.string.settings_transport_label), stringResource(R.string.settings_transport_value), divider = false)
                ValueRow(stringResource(R.string.settings_encryption_label), stringResource(R.string.settings_encryption_value))
                ValueRow(stringResource(R.string.settings_license_label), "GPL-3.0")
            }
            // Отказ от гарантий — сама лицензия (GPL-3.0, разделы 15–16); отдельных условий
            // использования нет: сервиса, который ими регулировать, тоже нет.
            Text(
                stringResource(R.string.about_no_warranty),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(horizontal = Tokens.Space.xl),
            )
            CardSection {
                ValueRow(stringResource(R.string.whatsnew_title), appVersionName(), divider = false, chevron = true, onClick = onWhatsNew)
                ValueRow(stringResource(R.string.policy_title), "", chevron = true, onClick = onPolicy)
            }
            UpdatesCard()
            CardSection {
                CardRow(onClick = { saveLog.launch(chat.peerage.LogExport.fileName()) }, divider = false, inset = Tokens.Space.l) {
                    Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
                        Text(stringResource(R.string.settings_log_save), color = Color.White, fontSize = Tokens.Font.name)
                        Text(
                            stringResource(R.string.settings_log_hint),
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.body,
                            modifier = Modifier.padding(top = Tokens.Space.xxs),
                        )
                    }
                }
            }
        }
    }
}

/**
 * Обновления в «О приложении» (app-ui.md → «Обновления»): тумблер ежедневной проверки и
 * «Проверить сейчас» с итогом. Ручная проверка — сама по себе согласие на этот запрос.
 */
@Composable
private fun UpdatesCard() {
    val s = Updates.state
    CardSection {
        PrivacySwitch(
            title = stringResource(R.string.update_toggle),
            sub = stringResource(R.string.update_consent_sub),
            checked = Updates.facts.consent == true,
            divider = false,
            onChange = Updates::setConsent,
        )
        val result = when (s) {
            UpdateState.Checking -> stringResource(R.string.update_checking)
            UpdateState.UpToDate -> stringResource(R.string.update_latest)
            UpdateState.CheckFailed -> stringResource(R.string.update_offline)
            is UpdateState.Available -> stringResource(R.string.update_available, s.info.versionName)
            is UpdateState.Downloading -> stringResource(R.string.update_downloading)
            is UpdateState.Ready -> stringResource(R.string.update_ready)
            is UpdateState.DownloadFailed -> stringResource(R.string.update_failed)
            UpdateState.Rejected -> stringResource(R.string.update_rejected)
            UpdateState.Idle -> null
        }
        CardRow(onClick = Updates::check, inset = Tokens.Space.l) {
            Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
                Text(stringResource(R.string.update_check_now), color = Color.White, fontSize = Tokens.Font.name)
                if (result != null) {
                    Text(
                        result,
                        color = Tokens.Colors.tertiary,
                        fontSize = Tokens.Font.body,
                        modifier = Modifier.padding(top = Tokens.Space.xxs),
                    )
                }
            }
        }
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

/** Только versionName — для строки «Что нового» (номер сборки там лишний). */
@Composable
private fun appVersionName(): String {
    val ctx = LocalContext.current
    return remember { runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull().orEmpty() }
}
