package chat.peerage.ui

import android.app.Activity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import chat.peerage.AppLock
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Tokens

/** Сколько раз можно ошибиться в трёх словах, прежде чем проверка станет «не пройдена». */
private const val CHECK_ATTEMPTS = 3

/**
 * Проверка копии ключа — один экран для первого запуска, полосы на главной и настроек
 * (app-ui.md → «Напоминания о копиях»). Три слова по случайным номерам; ввод — только
 * словами словаря, с подсказкой по первым буквам. Ответ один на все три: какое слово не
 * совпало, не говорится — иначе проверка подсказывает ответ. Номера те же все три попытки.
 *
 * Совпадение и провал записываются моделью; здесь — только куда идти дальше.
 */
@Composable
internal fun KeyCheckContent(
    model: AppModel,
    onPassed: () -> Unit,
    onFailed: () -> Unit,
    onShowWords: () -> Unit,
    modifier: Modifier = Modifier,
    /** Крупный заголовок над пояснением — в первом запуске (`onboard/Check`). */
    heading: String? = null,
) {
    SecureFlag()
    LaunchedEffect(Unit) { model.loadBip39() }
    val picks = rememberSaveable { (1..24).shuffled().take(3).sorted() }
    val words = remember { mutableStateListOf("", "", "") }
    var focused by remember { mutableIntStateOf(0) }
    var attempts by rememberSaveable { mutableIntStateOf(0) }
    var mismatch by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val dict = remember(model.bip39) { model.bip39.toHashSet() }
    val focus = remember { List(3) { FocusRequester() } }
    val focusManager = LocalFocusManager.current
    val allValid = words.all { it in dict }

    val submit: () -> Unit = {
        if (allValid && !busy) {
            busy = true
            val last = attempts + 1 >= CHECK_ATTEMPTS
            model.checkKeyWords(picks.zip(words), final = last) { ok ->
                busy = false
                when {
                    ok -> onPassed()
                    last -> onFailed()
                    else -> {
                        attempts++
                        mismatch = true
                    }
                }
            }
        }
    }
    val fill: (Int, String) -> Unit = { i, w ->
        words[i] = w
        mismatch = false
        if (i < 2) focus[i + 1].requestFocus() else focusManager.clearFocus()
    }

    Column(modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.xl).padding(top = if (heading == null) Tokens.Space.l else Tokens.Space.s),
        ) {
            if (heading != null) {
                Text(
                    heading,
                    color = Color.White,
                    fontSize = Tokens.Font.hero,
                    lineHeight = Tokens.Font.heroLine,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(bottom = Tokens.Space.s),
                )
            }
            Text(
                stringResource(R.string.key_check_sub),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.label,
                lineHeight = Tokens.Font.leadLine,
            )
            Spacer(Modifier.height(Tokens.Space.xl))
            picks.forEachIndexed { i, n ->
                Text(
                    stringResource(R.string.key_check_word, n),
                    color = Tokens.Colors.secondary,
                    fontSize = Tokens.Font.preview,
                    modifier = Modifier.padding(start = Tokens.Space.xs, bottom = Tokens.Space.xs + Tokens.Space.xxs),
                )
                WordField(
                    value = words[i],
                    known = words[i].isEmpty() || words[i] in dict || focused == i,
                    focusRequester = focus[i],
                    last = i == 2,
                    onFocus = { focused = i },
                    onChange = {
                        words[i] = it
                        mismatch = false
                    },
                    onNext = { if (i < 2) focus[i + 1].requestFocus() else submit() },
                )
                if (words[i].isNotEmpty() && words[i] !in dict && focused != i) {
                    Text(
                        stringResource(R.string.key_check_unknown),
                        color = Tokens.Colors.error,
                        fontSize = Tokens.Font.body,
                        modifier = Modifier.padding(start = Tokens.Space.xs, top = Tokens.Space.xs),
                    )
                }
                Spacer(Modifier.height(Tokens.Space.m + Tokens.Space.xxs))
            }
            if (mismatch) {
                // Вежливая живая область: TalkBack зачитает ответ, не перебивая.
                Text(
                    stringResource(R.string.key_check_mismatch),
                    color = Tokens.Colors.error,
                    fontSize = Tokens.Font.preview,
                    modifier = Modifier.padding(horizontal = Tokens.Space.xs).politeLive(),
                )
            }
        }
        Column(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.xl, vertical = Tokens.Space.m),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            AccentButton(
                stringResource(R.string.key_check_btn),
                Modifier.fillMaxWidth().heightIn(min = Tokens.Size.primaryButton),
                enabled = allValid && !busy,
                onClick = submit,
            )
            GhostButton(stringResource(R.string.key_check_show), modifier = Modifier.fillMaxWidth(), onClick = onShowWords)
        }
        WordSuggestions(model.bip39, words.getOrElse(focused) { "" }) { fill(focused, it) }
    }
}

/** Поле одного слова: только строчные латинские буквы, без автоисправления и обучения клавиатуры. */
@Composable
private fun WordField(
    value: String,
    known: Boolean,
    focusRequester: FocusRequester,
    last: Boolean,
    onFocus: () -> Unit,
    onChange: (String) -> Unit,
    onNext: () -> Unit,
) {
    var hasFocus by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Tokens.Radius.field)
    BasicTextField(
        value = value,
        onValueChange = { v -> onChange(v.lowercase().filter { it in 'a'..'z' }.take(8)) },
        singleLine = true,
        textStyle = TextStyle(
            color = if (known) Color.White else Tokens.Colors.error,
            fontSize = Tokens.Font.name,
            fontWeight = FontWeight.Medium,
        ),
        cursorBrush = SolidColor(Tokens.Colors.accent),
        // Пароль — чтобы клавиатура не запоминала слова ключа и не подсказывала свои:
        // подсказки — наши, из словаря.
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.None,
            autoCorrect = false,
            keyboardType = KeyboardType.Password,
            imeAction = if (last) ImeAction.Done else ImeAction.Next,
        ),
        keyboardActions = KeyboardActions(onNext = { onNext() }, onDone = { onNext() }),
        modifier = Modifier.fillMaxWidth().focusRequester(focusRequester)
            .onFocusChanged {
                hasFocus = it.isFocused
                if (it.isFocused) onFocus()
            },
        decorationBox = { inner ->
            Box(
                Modifier.fillMaxWidth().heightIn(min = Tokens.Size.formField).clip(shape).background(Tokens.Colors.group)
                    .then(
                        when {
                            hasFocus -> Modifier.border(Tokens.Size.focusRing, Tokens.Colors.accent, shape)
                            !known -> Modifier.border(Tokens.Size.focusRing, Tokens.Colors.error, shape)
                            else -> Modifier
                        },
                    )
                    .padding(horizontal = Tokens.Space.l),
                contentAlignment = Alignment.CenterStart,
            ) { inner() }
        },
    )
}

/** Подсказки над клавиатурой: до трёх слов словаря по набранным буквам, первое — акцентом. */
@Composable
private fun WordSuggestions(dict: List<String>, prefix: String, onPick: (String) -> Unit) {
    val hits = remember(dict, prefix) {
        if (prefix.isEmpty()) emptyList() else dict.asSequence().filter { it.startsWith(prefix) }.take(3).toList()
    }
    if (hits.isEmpty() || hits == listOf(prefix)) return
    Row(
        Modifier.fillMaxWidth().background(Tokens.Colors.bg)
            .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs + Tokens.Space.xxs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        hits.forEachIndexed { i, w ->
            Box(
                Modifier.weight(1f).heightIn(min = Tokens.Size.suggest)
                    .clip(RoundedCornerShape(Tokens.Radius.chip))
                    .background(if (i == 0) Tokens.Colors.accentMuted else Tokens.Colors.raised)
                    .clickable(role = Role.Button) { onPick(w) },
                contentAlignment = Alignment.Center,
            ) {
                Text(w, color = if (i == 0) Tokens.Colors.accentSoft else Color.White, fontSize = Tokens.Font.label)
            }
        }
    }
}

/**
 * Ключ целиком — 24 слова под FLAG_SECURE. Здесь записывают копию и сверяют её после
 * проваленной проверки. «Слова записаны» ведёт к проверке: копия считается сделанной
 * только после неё.
 */
@Composable
private fun KeyWordsContent(model: AppModel, deviceSecure: Boolean, onWritten: () -> Unit) {
    SecureFlag()
    LaunchedEffect(Unit) { model.revealSeedPhrase() }
    Column(Modifier.fillMaxSize().padding(horizontal = Tokens.Space.xl)) {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(top = Tokens.Space.l)) {
            Text(stringResource(R.string.onb_phrase_sub), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
            Spacer(Modifier.height(Tokens.Space.l))
            PhraseGrid(model.revealedPhrase)
            Row(
                Modifier.fillMaxWidth().padding(vertical = Tokens.Space.m),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.Center,
            ) {
                val color = if (deviceSecure) Tokens.Colors.online else Tokens.Colors.warn
                Icon(TablerIcons.ShieldCheck, null, tint = color, modifier = Modifier.size(Tokens.Size.iconXs))
                Spacer(Modifier.width(Tokens.Space.s))
                Text(
                    stringResource(if (deviceSecure) R.string.onb_screen_secure else R.string.key_no_device_lock),
                    color = color,
                    fontSize = Tokens.Font.body,
                )
            }
        }
        AccentButton(
            stringResource(R.string.onb_wrote_down),
            Modifier.fillMaxWidth().padding(vertical = Tokens.Space.m).navigationBarsPadding()
                .heightIn(min = Tokens.Size.primaryButton),
        ) {
            model.clearRevealedPhrase()
            onWritten()
        }
    }
}

/**
 * Копия ключа вне первого запуска: с главной («Сделать», «Проверить», «Сверить», «Проверить
 * ещё раз») и из настроек. [wordsFirst] — начать с 24 слов (сделать или сверить копию),
 * иначе — сразу с проверки. Слова показываются только после разблокировки устройства:
 * в чужих руках разблокированное приложение не должно отдавать ключ.
 */
@Composable
internal fun KeyFlowScreen(model: AppModel, wordsFirst: Boolean, onDone: () -> Unit) {
    val ctx = LocalContext.current
    val deviceSecure = remember { AppLock.deviceSecure(ctx) }
    // Шаг: слова или проверка. Слова — только после разблокировки (unlocked).
    var words by rememberSaveable { mutableStateOf(wordsFirst) }
    var unlocked by rememberSaveable { mutableStateOf(false) }
    val unlock = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { res ->
        if (res.resultCode == Activity.RESULT_OK) {
            unlocked = true
        } else if (wordsFirst && !unlocked) {
            onDone()
        } else {
            words = false
        }
    }
    LaunchedEffect(words, unlocked) {
        if (words && !unlocked) {
            val intent = AppLock.credentialIntent(ctx)
            if (intent == null) unlocked = true else unlock.launch(intent)
        }
    }
    LaunchedEffect(words, unlocked) { if (words && unlocked) model.keyReconciled() }
    BackHandler(enabled = words && !wordsFirst) {
        model.clearRevealedPhrase()
        words = false
    }
    when {
        words && unlocked -> KeyWordsContent(model, deviceSecure) { words = false }
        words -> Box(Modifier.fillMaxSize())
        else -> KeyCheckContent(model, onPassed = onDone, onFailed = onDone, onShowWords = { words = true })
    }
}

/** Вежливая живая область: изменение текста зачитывается, не перебивая. */
internal fun Modifier.politeLive(): Modifier = semantics {
    liveRegion = androidx.compose.ui.semantics.LiveRegionMode.Polite
}
