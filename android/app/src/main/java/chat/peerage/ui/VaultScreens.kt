package chat.peerage.ui

import android.app.Activity
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import chat.peerage.AppLock
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Tokens
import chat.peerage.VaultKeyManager
import chat.peerage.VaultLevel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/**
 * Замок приложения (макет `security/Lock`): под системным запросом — знак и что именно
 * заперто. Сам запрос рисует система (биометрия, PIN устройства — [chat.peerage.AppLock]);
 * кнопка — повторить его, если запрос закрыли.
 */
@Composable
internal fun LockScreen(onUnlock: () -> Unit) {
    val app = stringResource(R.string.app_name)
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        Column(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = Tokens.Space.xl),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Icon(painterResource(R.drawable.ic_glyph), null, tint = Tokens.Colors.accent, modifier = Modifier.size(Tokens.Size.heroTile))
            Spacer(Modifier.height(Tokens.Space.xl))
            Text(
                stringResource(R.string.lock_title, app),
                color = Color.White,
                fontSize = Tokens.Font.hero,
                lineHeight = Tokens.Font.heroLine,
                fontWeight = FontWeight.Bold,
                textAlign = TextAlign.Center,
                modifier = Modifier.semantics { heading() },
            )
        }
        AccentButton(
            stringResource(R.string.lock_prompt, app),
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m, bottom = Tokens.Space.xxl)
                .heightIn(min = Tokens.Size.primaryButton),
            onClick = onUnlock,
        )
    }
}

/**
 * Пароль хранилища при холодном старте (уровни PASSWORD/BOTH, макет `security/Vault`).
 * Пока пароль не введён, движок не стартует вовсе: данные лежат зашифрованными, и показывать
 * «пустой мессенджер» вместо запроса было бы враньём про состояние. «Пароль забыт» — шторка
 * `security/Forgot`: пароль не восстанавливается, личность — по 24 словам.
 */
@Composable
internal fun VaultUnlockScreen(
    model: AppModel,
    dataDir: String,
    onKey: (ByteArray) -> Unit,
    onSubmit: (String, (Boolean) -> Unit) -> Unit,
) {
    val ctx = LocalContext.current
    val dir = remember { File(dataDir) }
    // «Открывать отпечатком» (`security/VaultBio`): запрос сразу; «Ввести пароль» — экран ниже.
    LaunchedEffect(Unit) {
        val activity = ctx as? Activity ?: return@LaunchedEffect
        val cipher = VaultKeyManager.bioDecryptCipher(dir) ?: return@LaunchedEffect
        AppLock.promptCrypto(
            activity,
            cipher,
            ctx.getString(R.string.vault_bio_prompt),
            ctx.getString(R.string.vault_bio_negative),
        ) { signed -> signed?.let { c -> runCatching { VaultKeyManager.bioUnwrap(dir, c) }.getOrNull()?.let(onKey) } }
    }
    var password by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var wrong by remember { mutableStateOf(false) }
    var forgot by remember { mutableStateOf(false) }
    var restoring by rememberSaveable { mutableStateOf(false) }
    if (restoring) {
        BackHandler { restoring = false }
        PhraseRecoverStep(model, dataDir) { restoring = false }
        return
    }
    val submit = {
        if (password.isNotEmpty() && !busy) {
            busy = true
            onSubmit(password) { ok ->
                busy = false
                wrong = !ok
            }
        }
    }
    OnbPage(
        bar = 0,
        onBack = null,
        icon = TablerIcons.Lock,
        title = stringResource(R.string.vault_unlock_title),
        sub = stringResource(R.string.vault_unlock_sub),
        // Argon2id считается ~секунду: подпись кнопки говорит, что идёт проверка.
        primary = stringResource(if (busy) R.string.vault_deriving else R.string.vault_unlock_btn),
        onPrimary = submit,
        primaryEnabled = password.isNotEmpty() && !busy,
        secondary = stringResource(R.string.vault_forgot),
        onSecondary = { forgot = true },
    ) {
        Spacer(Modifier.height(Tokens.Space.xxl))
        PasswordField(password, wrong, enabled = !busy) {
            password = it
            wrong = false
        }
        if (wrong) OnbError(stringResource(R.string.vault_wrong))
    }
    if (forgot) {
        ForgotSheet(
            onRestore = {
                forgot = false
                restoring = true
            },
            onClose = { forgot = false },
        )
    }
}

/** «Пароль забыт» (`security/Forgot`): пароля не вернуть; личность — по 24 словам. */
@Composable
private fun ForgotSheet(onRestore: () -> Unit, onClose: () -> Unit) {
    ActionSheet(stringResource(R.string.vault_forgot), stringResource(R.string.vault_forgot_text), onClose) {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.field)).background(Tokens.Colors.raised)) {
            OnbRow(TablerIcons.Key, stringResource(R.string.vault_forgot_words), stringResource(R.string.vault_forgot_words_sub), divider = false)
        }
        Text(
            stringResource(R.string.vault_forgot_caption),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            lineHeight = Tokens.Font.lineHeight,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
        PlateButton(stringResource(R.string.recovery_restore), modifier = Modifier.fillMaxWidth(), primary = true, onClick = onRestore)
        PlateButton(stringResource(R.string.common_cancel), modifier = Modifier.fillMaxWidth(), onClick = onClose)
    }
}

/** Поле пароля: точки, глаз — показать; ошибка — красной обводкой, фокус — акцентом. */
@Composable
internal fun PasswordField(value: String, error: Boolean, enabled: Boolean = true, hint: String? = null, onValue: (String) -> Unit) {
    var shown by remember { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Tokens.Radius.field)
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.formField).clip(shape).background(Tokens.Colors.group)
            .then(
                when {
                    error -> Modifier.border(Tokens.Size.focusRing, Tokens.Colors.error, shape)
                    focused -> Modifier.border(Tokens.Size.focusRing, Tokens.Colors.accent, shape)
                    else -> Modifier
                },
            )
            .padding(start = Tokens.Space.l),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicTextField(
            value,
            onValue,
            enabled = enabled,
            singleLine = true,
            textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.name),
            cursorBrush = SolidColor(Tokens.Colors.accent),
            visualTransformation = if (shown) VisualTransformation.None else PasswordVisualTransformation(),
            keyboardOptions = PasswordKeyboard,
            modifier = Modifier.weight(1f).onFocusChanged { focused = it.isFocused },
            decorationBox = { inner ->
                Box {
                    if (value.isEmpty() && hint != null) Text(hint, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name)
                    inner()
                }
            },
        )
        Box(
            Modifier.size(Tokens.Size.touch).clickable(
                role = Role.Button,
                onClickLabel = stringResource(if (shown) R.string.vault_hide_password else R.string.vault_show_password),
            ) { shown = !shown },
            contentAlignment = Alignment.Center,
        ) {
            Icon(if (shown) TablerIcons.Eye else TablerIcons.EyeOff, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
        }
    }
}

/**
 * «Шифрование данных» (макет `security/Level`). Смена уровня пере-оборачивает тот же ключ
 * хранилища — переписка не перешифровывается. У «Ключа устройства» и «Ключа и пароля» —
 * предупреждение о сбросе ключа системой; у «Пароля» — «Открывать отпечатком».
 */
@Composable
internal fun VaultLevelScreen(dataDir: String, onDone: () -> Unit) {
    val ctx = LocalContext.current
    val dir = remember { File(dataDir) }
    val current = remember { VaultKeyManager.level(dir) }
    val bioWas = remember { VaultKeyManager.bioEnabled(dir) }
    val bioAvailable = remember { VaultKeyManager.bioSupported() && AppLock.strongBiometrics(ctx) }
    var target by remember { mutableStateOf(current) }
    var bio by remember { mutableStateOf(bioWas) }
    var oldPassword by remember { mutableStateOf("") }
    var newPassword by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val needOld = current != VaultLevel.KEYSTORE
    // Тот же «Пароль» без нового пароля — меняется только отпечаток.
    val bioOnly = target == current && target == VaultLevel.PASSWORD && newPassword.isEmpty()
    val ready = !busy && (!needOld || oldPassword.isNotEmpty()) && when {
        target == VaultLevel.KEYSTORE -> current != VaultLevel.KEYSTORE
        bioOnly -> bio != bioWas
        else -> newPassword.isNotEmpty() && newPassword == repeat
    }
    val apply: () -> Unit = {
        busy = true
        failed = false
        scope.launch {
            val old = oldPassword.ifEmpty { null }
            // Argon2id — на IO, а итог — на главном: он уводит с экрана (инвариант 4).
            val raw = withContext(Dispatchers.IO) {
                // Только отпечаток — пароль всё равно сверяется: выключать и включать его
                // вправе тот, кто знает пароль.
                val ok = if (bioOnly) {
                    runCatching { VaultKeyManager.getOrCreate(dir, old) }.isSuccess
                } else {
                    VaultKeyManager.changeLevel(dir, target, old, newPassword.ifEmpty { null })
                }
                if (!ok) return@withContext null
                if (target == VaultLevel.PASSWORD && bio) {
                    runCatching { VaultKeyManager.getOrCreate(dir, newPassword.ifEmpty { old }) }.getOrNull()
                } else {
                    if (!bio) VaultKeyManager.bioDisable(dir)
                    ByteArray(0)
                }
            }
            if (raw == null) {
                busy = false
                failed = true
                return@launch
            }
            val activity = ctx as? Activity
            if (raw.isEmpty() || activity == null) {
                onDone()
                return@launch
            }
            val cipher = VaultKeyManager.bioEncryptCipher()
            AppLock.promptCrypto(activity, cipher, ctx.getString(R.string.vault_bio), ctx.getString(R.string.common_cancel)) { signed ->
                if (signed != null) VaultKeyManager.bioEnable(dir, signed, raw) else VaultKeyManager.bioDisable(dir)
                raw.fill(0)
                onDone()
            }
        }
    }
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        TopBar(stringResource(R.string.settings_at_rest), onBack = onDone)
        Column(
            Modifier.weight(1f).fillMaxWidth()
                .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                .background(Tokens.Colors.panel)
                .verticalScroll(rememberScrollState())
                .padding(top = Tokens.Space.panel),
        ) {
            CardSection {
                LevelRow(VaultLevel.KEYSTORE, target, R.string.vault_lvl_keystore, R.string.vault_lvl_keystore_desc, divider = false) { target = it }
                LevelRow(VaultLevel.PASSWORD, target, R.string.vault_lvl_password, R.string.vault_lvl_password_desc) { target = it }
                if (target == VaultLevel.PASSWORD && bioAvailable) {
                    BioRow(bio) { bio = it }
                }
                LevelRow(VaultLevel.BOTH, target, R.string.vault_lvl_both, R.string.vault_lvl_both_desc) { target = it }
            }
            if (needOld) {
                SectionTitle(stringResource(R.string.vault_current_password))
                Box(Modifier.padding(horizontal = Tokens.Space.panel)) {
                    PasswordField(oldPassword, failed) {
                        oldPassword = it
                        failed = false
                    }
                }
                if (failed) Box(Modifier.padding(horizontal = Tokens.Space.panel)) { OnbError(stringResource(R.string.vault_change_failed)) }
            }
            if (target != VaultLevel.KEYSTORE) {
                SectionTitle(stringResource(R.string.vault_new_password))
                Column(Modifier.padding(horizontal = Tokens.Space.panel), verticalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                    PasswordField(newPassword, false) { newPassword = it }
                    PasswordField(repeat, repeat.isNotEmpty() && repeat != newPassword, hint = stringResource(R.string.vault_ph_repeat)) { repeat = it }
                    if (repeat.isNotEmpty() && repeat != newPassword) OnbError(stringResource(R.string.vault_mismatch))
                }
                Box(Modifier.padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.l)) {
                    OnbWarn(stringResource(R.string.vault_no_recovery))
                }
            }
            Spacer(Modifier.height(Tokens.Space.l))
        }
        ScreenFooter(stringResource(if (busy) R.string.vault_deriving else R.string.vault_apply), enabled = ready, onClick = apply)
    }
}

/** Вариант уровня: кружок выбора, название, пояснение; выбранному «ключу» — предупреждение. */
@Composable
private fun LevelRow(value: VaultLevel, selected: VaultLevel, title: Int, desc: Int, divider: Boolean = true, onPick: (VaultLevel) -> Unit) {
    val on = value == selected
    if (divider) HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider)
    Row(
        Modifier.fillMaxWidth().clickable(role = Role.RadioButton) { onPick(value) }
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Box(
            Modifier.padding(top = Tokens.Space.xxs).size(Tokens.Size.radio).clip(CircleShape)
                .then(
                    if (on) {
                        Modifier.background(Tokens.Colors.accent)
                    } else {
                        Modifier.border(Tokens.Size.radioRing, Tokens.Colors.tertiary, CircleShape)
                    },
                ),
            contentAlignment = Alignment.Center,
        ) {
            if (on) Icon(TablerIcons.Check, null, tint = Tokens.Colors.accentDark, modifier = Modifier.size(Tokens.Size.iconXs))
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs)) {
            Text(stringResource(title), color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            Text(stringResource(desc), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, lineHeight = Tokens.Font.lineHeight)
            if (on && value != VaultLevel.PASSWORD) {
                Row(Modifier.padding(top = Tokens.Space.s), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                    Icon(TablerIcons.AlertTriangle, null, tint = Tokens.Colors.stateWarnText, modifier = Modifier.size(Tokens.Size.iconXs))
                    Text(
                        stringResource(R.string.vault_lvl_keystore_warn),
                        color = Tokens.Colors.stateWarnText,
                        fontSize = Tokens.Font.body,
                        lineHeight = Tokens.Font.lineHeight,
                    )
                }
            }
        }
    }
}

/** «Открывать отпечатком» под «Паролем»: плашка ступенью выше группы. */
@Composable
private fun BioRow(checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .padding(start = Tokens.Space.l + Tokens.Size.radio + Tokens.Space.m + Tokens.Space.xxs, end = Tokens.Space.l, bottom = Tokens.Space.m)
            .clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.raised)
            .switchRow(checked, onChange = onChange)
            .heightIn(min = Tokens.Size.touch).padding(horizontal = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(TablerIcons.Fingerprint, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconS))
        Text(stringResource(R.string.vault_bio), color = Color.White, fontSize = Tokens.Font.preview, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = null)
    }
}
