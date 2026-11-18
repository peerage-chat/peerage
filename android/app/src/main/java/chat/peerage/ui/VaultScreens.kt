package chat.peerage.ui

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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
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
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Tokens

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
internal fun VaultUnlockScreen(model: AppModel, dataDir: String, onSubmit: (String, (Boolean) -> Unit) -> Unit) {
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
internal fun PasswordField(value: String, error: Boolean, enabled: Boolean = true, onValue: (String) -> Unit) {
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
