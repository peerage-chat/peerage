@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Build
import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

// Первый запуск (app-ui.md → «Первый запуск», макеты `onboard/`). Один каркас на все шаги
// ([OnbPage]): «назад» и полоска «шаг N из 3» сверху, крупный заголовок и пояснение,
// содержимое прокручивается (крупный шрифт), кнопки прибиты к низу.

/** FLAG_SECURE на окно, пока композабл в композиции (блокирует скриншот/запись экрана
 *  и превью в «недавних»). Ставим на экранах с секретом — сид-фраза. */
@Composable
internal fun SecureFlag() {
    val ctx = LocalContext.current
    DisposableEffect(Unit) {
        val window = (ctx as? Activity)?.window
        window?.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}

/** Шаги новой личности; [bar] — сколько отрезков полоски закрашено (0 — полоски нет). */
private enum class OnbStep(val bar: Int) { Welcome(0), Profile(1), Key(2), Phrase(2), Check(2), Perms(3) }

/** Пошаговый флоу первого запуска. `onFinish` — онбординг завершён (пометить флаг). */
@Composable
internal fun OnboardingFlow(model: AppModel, onFinish: () -> Unit) {
    var step by rememberSaveable { mutableStateOf(OnbStep.Welcome) }
    var importing by rememberSaveable { mutableStateOf(false) }
    // «Назад» — на шаг назад; с приветствия системная кнопка выходит штатно. С разрешений —
    // к копии ключа: проверка уже позади, к ней не возвращаются.
    val back: () -> Unit = {
        when {
            importing -> importing = false
            step == OnbStep.Profile -> step = OnbStep.Welcome
            step == OnbStep.Key -> step = OnbStep.Profile
            step == OnbStep.Phrase -> step = OnbStep.Key
            step == OnbStep.Check -> step = OnbStep.Phrase
            step == OnbStep.Perms -> step = OnbStep.Key
        }
    }
    BackHandler(enabled = importing || step != OnbStep.Welcome, onBack = back)
    if (importing) {
        ImportKeyStep(model = model, onBack = back)
        return
    }
    when (step) {
        OnbStep.Welcome -> WelcomeStep(onStart = { step = OnbStep.Profile }, onImport = { importing = true })
        OnbStep.Profile -> ProfileStep(model, onBack = back) { step = OnbStep.Key }
        OnbStep.Key -> KeyIntroStep(onBack = back, onShow = { step = OnbStep.Phrase }, onLater = { step = OnbStep.Perms })
        OnbStep.Phrase -> PhraseStep(model, onBack = back) { step = OnbStep.Check }
        OnbStep.Check -> Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
            OnbTop(OnbStep.Check.bar, back)
            // Копия отмечается сделанной только проверкой; провал — тоже итог: на главной
            // встанет полоса «не пройдена» со сверкой.
            KeyCheckContent(
                model,
                onPassed = { step = OnbStep.Perms },
                onFailed = { step = OnbStep.Perms },
                onShowWords = { step = OnbStep.Phrase },
                heading = stringResource(R.string.onb_check_title),
            )
        }
        OnbStep.Perms -> PermissionsStep(onBack = back, onDone = onFinish)
    }
}

/** Восстановление личности по 24 словам при первом запуске. Импорт пишет profile.json и
 *  перезапускает процесс — личность грузится начисто. */
@Composable
private fun ImportKeyStep(model: AppModel, onBack: () -> Unit) {
    val ctx = LocalContext.current
    var busy by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<RecoverResult?>(null) }
    PhraseEntryStep(
        model,
        busy = busy,
        mismatch = result == RecoverResult.BAD_PHRASE,
        error = if (result == RecoverResult.FAILED) stringResource(R.string.onb_restore_failed) else null,
        onBack = onBack,
        onEdit = { result = null },
    ) { phrase ->
        busy = true
        result = null
        model.importIdentity(Profiles.main(ctx).absolutePath, phrase) { r ->
            busy = false
            if (r == RecoverResult.OK) relaunchApp(ctx) else result = r
        }
    }
}

private const val PHRASE_WORDS = 24
private const val MAX_WORD = 8 // самое длинное слово BIP39 — 8 букв

/** С какого масштаба шрифта 24 слова встают в один столбец (×2.0 — app-ui.md). */
private const val ONE_COLUMN_FONT = 1.6f

/**
 * Ввод 24 слов (`onboard/Restore`, `Mismatch`) — та же сетка, что показ ([PhraseGrid]):
 * переписанное с бумаги вводится ячейка в ячейку. Слова — только из словаря BIP39, подсказка
 * над клавиатурой по первым буквам. Пробел, вставка в ячейку и «Вставить» раскладывают
 * текст по словам с этой ячейки (с «Вставить» — с №1). Ошибки разные: «№N — нет такого
 * слова» — сразу, как ячейку покинули; [mismatch] (сумма не сошлась) — после «Восстановить».
 */
@Composable
private fun PhraseEntryStep(
    model: AppModel,
    busy: Boolean,
    mismatch: Boolean,
    error: String?,
    onBack: () -> Unit,
    onEdit: () -> Unit,
    onSubmit: (String) -> Unit,
) {
    SecureFlag()
    LaunchedEffect(Unit) { model.loadBip39() }
    // remember, не rememberSaveable: ключ не должен уходить в сохранённое состояние окна
    // (его хранит система). Поворот активность не пересоздаёт (configChanges в манифесте).
    val words = remember { mutableStateListOf(*Array(PHRASE_WORDS) { "" }) }
    val focus = remember { List(PHRASE_WORDS) { FocusRequester() } }
    var focused by remember { mutableIntStateOf(-1) }
    val dict = remember(model.bip39) { model.bip39.toHashSet() }
    val clipboard = LocalClipboardManager.current
    val allValid = dict.isNotEmpty() && words.all { it in dict }
    val unknown = words.indices.firstOrNull { words[it].isNotEmpty() && words[it] !in dict && it != focused }
    val keyboard = LocalSoftwareKeyboardController.current
    // После последней ячейки — спрятать клавиатуру, не снимая фокус: со снятым фокусом
    // аппаратный Enter отпускается уже на «Назад» и уводит с экрана.
    val next: (Int) -> Unit = { i -> if (i + 1 < PHRASE_WORDS) focus[i + 1].requestFocus() else keyboard?.hide() }
    // Текст в ячейку i: одно слово — на место; с разделителями — по словам дальше.
    val put: (Int, String) -> Unit = { i, raw ->
        onEdit()
        val parts = raw.lowercase().split(Regex("[^a-z]+"))
        val ws = parts.filter { it.isNotEmpty() }
        when {
            parts.size == 1 -> words[i] = parts[0].take(MAX_WORD)
            ws.isEmpty() -> words[i] = ""
            else -> {
                ws.forEachIndexed { k, w -> if (i + k < PHRASE_WORDS) words[i + k] = w.take(MAX_WORD) }
                next(i + ws.size - 1)
            }
        }
    }
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        OnbTop(0, onBack)
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = Tokens.Space.xl),
        ) {
            Row(
                Modifier.fillMaxWidth().padding(top = Tokens.Space.s),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
            ) {
                Text(
                    stringResource(R.string.onb_restore_title),
                    color = Color.White,
                    fontSize = Tokens.Font.heroSmall,
                    lineHeight = Tokens.Font.heroSmallLine,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.weight(1f).semantics { heading() },
                )
                PastePill {
                    clipboard.getText()?.text?.takeIf { it.isNotBlank() }?.let { text ->
                        for (k in words.indices) words[k] = ""
                        put(0, "$text ")
                    }
                }
            }
            Text(
                stringResource(R.string.onb_restore_sub),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.label,
                lineHeight = Tokens.Font.leadLine,
                modifier = Modifier.padding(top = Tokens.Space.xs + Tokens.Space.xxs, bottom = Tokens.Space.xs),
            )
            Box(Modifier.heightIn(min = Tokens.Size.phraseNote).politeLive(), contentAlignment = Alignment.CenterStart) {
                if (unknown != null) {
                    Text(stringResource(R.string.onb_restore_unknown, unknown + 1), color = Tokens.Colors.failText, fontSize = Tokens.Font.body)
                }
            }
            PhraseLayout { i ->
                WordSlot(
                    value = words[i],
                    bad = words[i].isNotEmpty() && words[i] !in dict && focused != i,
                    focusRequester = focus[i],
                    last = i == PHRASE_WORDS - 1,
                    onFocus = { has ->
                        if (has) {
                            focused = i
                        } else if (focused == i) {
                            focused = -1
                        }
                    },
                    onChange = { put(i, it) },
                    onNext = { next(i) },
                )
            }
            if (mismatch) {
                Spacer(Modifier.height(Tokens.Space.l))
                OnbWarn(stringResource(R.string.onb_restore_mismatch), bad = true)
            }
            error?.let { OnbError(it) }
            Spacer(Modifier.height(Tokens.Space.l))
        }
        // Пока набирается слово, низ занят клавиатурой и подсказками (макет `onboard/Restore`);
        // кнопка — когда ячейку покинули или все 24 слова из словаря.
        if (focused < 0 || allValid) {
            OnbFooter(
                stringResource(if (busy) R.string.onb_checking else R.string.onb_restore),
                { if (allValid && !busy) onSubmit(words.joinToString(" ")) },
                secondary = null,
                onSecondary = {},
                note = null,
                enabled = allValid && !busy,
            )
        }
        WordSuggestions(model.bip39, if (focused >= 0) words[focused] else "") { w ->
            val i = focused
            if (i >= 0) {
                onEdit()
                words[i] = w
                next(i)
            }
        }
    }
}

/** «Вставить» справа от заголовка: пилюля с иконкой буфера. */
@Composable
private fun PastePill(onClick: () -> Unit) {
    Row(
        Modifier.heightIn(min = Tokens.Size.pill).clip(RoundedCornerShape(Tokens.Radius.pill)).background(Tokens.Colors.accentMuted)
            .clickable(role = Role.Button, onClick = onClick).padding(horizontal = Tokens.Space.m + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
    ) {
        Icon(TablerIcons.Clipboard, null, tint = Tokens.Colors.accentSoft, modifier = Modifier.size(Tokens.Size.iconXs))
        Text(stringResource(R.string.onb_restore_paste), color = Tokens.Colors.accentSoft, fontSize = Tokens.Font.preview, fontWeight = FontWeight.Medium)
    }
}

/** Ячейка слова: линия снизу — тихая пустой, акцентом в фокусе, красная у слова не из словаря. */
@Composable
private fun RowScope.WordSlot(
    value: String,
    bad: Boolean,
    focusRequester: FocusRequester,
    last: Boolean,
    onFocus: (Boolean) -> Unit,
    onChange: (String) -> Unit,
    onNext: () -> Unit,
) {
    var has by remember { mutableStateOf(false) }
    // Своё значение с выделением: в ячейку переходят и пробелом, и тапом — стоящее там
    // слово выделяется целиком, и набор его заменяет, а не дописывается к нему.
    var field by remember { mutableStateOf(TextFieldValue(value, TextRange(value.length))) }
    if (field.text != value) field = TextFieldValue(value, TextRange(value.length))
    val line = when {
        has -> Tokens.Colors.accent
        bad -> Tokens.Colors.error
        else -> Tokens.Colors.slotLine
    }
    val stroke = if (has || bad) Tokens.Size.focusRing else 1.dp
    BasicTextField(
        value = field,
        onValueChange = {
            val changed = it.text != field.text
            field = it
            if (changed) onChange(it.text)
        },
        singleLine = true,
        textStyle = Tokens.Text.base.copy(
            color = if (bad) Tokens.Colors.failText else Color.White,
            fontSize = Tokens.Font.name,
            fontWeight = FontWeight.Medium,
        ),
        cursorBrush = SolidColor(Tokens.Colors.accent),
        // Пароль — чтобы клавиатура не запоминала слова ключа и не подсказывала свои.
        keyboardOptions = KeyboardOptions(
            capitalization = KeyboardCapitalization.None,
            autoCorrect = false,
            keyboardType = KeyboardType.Password,
            imeAction = if (last) ImeAction.Done else ImeAction.Next,
        ),
        keyboardActions = KeyboardActions(onNext = { onNext() }, onDone = { onNext() }),
        modifier = Modifier.weight(1f).heightIn(min = Tokens.Size.phraseRow).focusRequester(focusRequester)
            .onFocusChanged {
                if (it.isFocused && !has) field = field.copy(selection = TextRange(0, field.text.length))
                has = it.isFocused
                onFocus(it.isFocused)
            }
            .drawBehind {
                val w = stroke.toPx()
                val y = size.height - w / 2
                drawLine(line, Offset(0f, y), Offset(size.width, y), w)
            },
        decorationBox = { inner ->
            Box(Modifier.fillMaxWidth().heightIn(min = Tokens.Size.phraseRow), contentAlignment = Alignment.CenterStart) { inner() }
        },
    )
}

/** Ошибка под полем: красным, зачитывается без перебивания. */
@Composable
internal fun OnbError(text: String) {
    Text(
        text,
        color = Tokens.Colors.error,
        fontSize = Tokens.Font.preview,
        modifier = Modifier.padding(start = Tokens.Space.xs, top = Tokens.Space.s).politeLive(),
    )
}

/**
 * Данные на устройстве не открываются (R-029, макет `security/Recovery`): ключ хранилища
 * потерян или не тот. Без человека ничего не стирается — он выбирает: вернуть личность по
 * 24 словам или начать заново. Старые данные в обоих случаях откладываются, а не удаляются.
 */
@Composable
internal fun RecoveryScreen(model: AppModel, dataDir: String, why: Recovery) {
    val ctx = LocalContext.current
    var step by rememberSaveable { mutableStateOf(0) } // 0 — выбор, 1 — фраза, 2 — «начать заново?»
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<Int?>(null) }
    val back = {
        step = 0
        error = null
    }
    BackHandler(enabled = step != 0 && !busy) { back() }
    when (step) {
        1 -> PhraseRecoverStep(model, dataDir, onBack = back)
        2 -> OnbPage(
            bar = 0,
            onBack = back,
            title = stringResource(R.string.recovery_new_title),
            sub = stringResource(R.string.recovery_new_sub),
            primary = stringResource(R.string.recovery_start_over),
            onPrimary = {
                if (!busy) {
                    busy = true
                    model.recoverStartOver(dataDir) { ok ->
                        busy = false
                        if (ok) relaunchApp(ctx) else error = R.string.recovery_failed
                    }
                }
            },
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            OnbWarn(stringResource(R.string.recovery_new_body))
            error?.let { OnbError(stringResource(it)) }
        }
        else -> OnbPage(
            bar = 0,
            onBack = null,
            icon = TablerIcons.AlertTriangle,
            iconTint = Tokens.Colors.stateWarnText,
            title = stringResource(R.string.recovery_title),
            sub = stringResource(R.string.recovery_sub),
            primary = stringResource(R.string.recovery_restore),
            onPrimary = { step = 1 },
            secondary = stringResource(R.string.recovery_start_over_long),
            onSecondary = { step = 2 },
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            val reason = if (why == Recovery.KEY_LOST) R.string.recovery_reason_key_lost else R.string.recovery_reason_mismatch
            OnbGroup {
                OnbRow(TablerIcons.ShieldCheck, stringResource(R.string.recovery_reason), stringResource(reason), divider = false)
            }
            OnbCaption(stringResource(R.string.recovery_body), Modifier.padding(top = Tokens.Space.s))
        }
    }
}

/**
 * Личность по 24 словам на месте данных, которые не открыть (R-029, забытый пароль):
 * данные откладываются ([AppModel.recoverFromPhrase]), уровень шифрования после — «ключ
 * устройства». Успех — перезапуск процесса.
 */
@Composable
internal fun PhraseRecoverStep(model: AppModel, dataDir: String, onBack: () -> Unit) {
    val ctx = LocalContext.current
    var busy by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<RecoverResult?>(null) }
    PhraseEntryStep(
        model,
        busy = busy,
        mismatch = result == RecoverResult.BAD_PHRASE,
        error = if (result == RecoverResult.FAILED) stringResource(R.string.recovery_failed) else null,
        onBack = onBack,
        onEdit = { result = null },
    ) { phrase ->
        busy = true
        result = null
        model.recoverFromPhrase(dataDir, phrase) { r ->
            busy = false
            if (r == RecoverResult.OK) relaunchApp(ctx) else result = r
        }
    }
}

/** Полный перезапуск процесса (после импорта личности): новый profile.json грузится
 *  начисто, без стейл-состояния нативного узла/потоков. */
private fun relaunchApp(ctx: Context) {
    val intent = ctx.packageManager.getLaunchIntentForPackage(ctx.packageName)
    intent?.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_NEW_TASK)
    ctx.startActivity(intent)
    Runtime.getRuntime().exit(0)
}

/**
 * Каркас шага (макеты `onboard/`): сверху «назад» и полоска [bar] из 3 (0 — без полоски),
 * плитка-знак [icon], заголовок 28, пояснение; содержимое; снизу [note], главная кнопка и
 * [secondary] текстом. [scroll] = false — содержимое само делит высоту (поле на весь экран).
 */
@Composable
internal fun OnbPage(
    bar: Int,
    onBack: (() -> Unit)?,
    title: String,
    sub: String?,
    primary: String,
    onPrimary: () -> Unit,
    icon: ImageVector? = null,
    iconTint: Color = Tokens.Colors.accent,
    iconBg: Color = Tokens.Colors.accentMuted,
    secondary: String? = null,
    onSecondary: () -> Unit = {},
    note: (@Composable () -> Unit)? = null,
    scroll: Boolean = true,
    primaryEnabled: Boolean = true,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        OnbTop(bar, onBack)
        Column(
            Modifier.weight(1f).fillMaxWidth()
                .then(if (scroll) Modifier.verticalScroll(rememberScrollState()) else Modifier)
                .padding(horizontal = Tokens.Space.xl),
        ) {
            if (icon != null) {
                Box(
                    Modifier.padding(top = Tokens.Space.s, bottom = Tokens.Space.m).size(Tokens.Size.heroTile)
                        .clip(RoundedCornerShape(Tokens.Radius.heroTile)).background(iconBg),
                    contentAlignment = Alignment.Center,
                ) { Icon(icon, null, tint = iconTint, modifier = Modifier.size(Tokens.Size.iconTile)) }
            }
            OnbTitle(title)
            if (sub != null) OnbLead(sub)
            content()
        }
        OnbFooter(primary, onPrimary, secondary, onSecondary, note, enabled = primaryEnabled)
    }
}

/** Верх шага: «назад» без плашки и полоска «шаг N из 3». Без «назад» место держится. */
@Composable
private fun OnbTop(bar: Int, onBack: (() -> Unit)?) {
    val stepDesc = stringResource(R.string.onb_step_of, bar, ONB_STEPS)
    Row(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.xs, end = Tokens.Space.xl, top = Tokens.Space.xs)
            .heightIn(min = Tokens.Size.touch),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        if (onBack != null) {
            BarIcon(TablerIcons.ChevronLeft, stringResource(R.string.common_back), bg = Tokens.Colors.bg, onClick = onBack)
        } else {
            Spacer(Modifier.size(Tokens.Size.touch))
        }
        if (bar > 0) {
            Row(
                Modifier.weight(1f).semantics { contentDescription = stepDesc },
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
            ) {
                repeat(ONB_STEPS) { i ->
                    Box(
                        Modifier.weight(1f).height(Tokens.Size.onbProgress).clip(RoundedCornerShape(percent = 50))
                            .background(if (i < bar) Tokens.Colors.accent else Tokens.Colors.raised),
                    )
                }
            }
        }
    }
}

private const val ONB_STEPS = 3

@Composable
private fun OnbTitle(text: String, welcome: Boolean = false) {
    Text(
        text,
        color = Color.White,
        fontSize = if (welcome) Tokens.Font.welcome else Tokens.Font.hero,
        lineHeight = if (welcome) Tokens.Font.welcomeLine else Tokens.Font.heroLine,
        fontWeight = FontWeight.Bold,
        modifier = Modifier.padding(top = Tokens.Space.s, bottom = Tokens.Space.s).semantics { heading() },
    )
}

@Composable
private fun OnbLead(text: String) {
    Text(text, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label, lineHeight = Tokens.Font.leadLine)
}

/** Низ шага: пометка над кнопкой, главная кнопка 52, второе действие текстом акцента. */
@Composable
private fun OnbFooter(
    primary: String,
    onPrimary: () -> Unit,
    secondary: String?,
    onSecondary: () -> Unit,
    note: (@Composable () -> Unit)?,
    enabled: Boolean = true,
) {
    Column(
        Modifier.fillMaxWidth().navigationBarsPadding()
            .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m, bottom = Tokens.Space.xxl),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        note?.invoke()
        AccentButton(primary, Modifier.fillMaxWidth().heightIn(min = Tokens.Size.primaryButton), enabled = enabled, onClick = onPrimary)
        if (secondary != null) GhostButton(secondary, modifier = Modifier.fillMaxWidth(), onClick = onSecondary)
    }
}

/** Группа строк шага (макет `.grp`): ступень group, скругление 18. */
@Composable
internal fun OnbGroup(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.group), content = content)
}

/** Строка группы (макет `.gr`): иконка без плитки, название, пояснение, справа — действие. */
@Composable
internal fun OnbRow(
    icon: ImageVector,
    title: String,
    sub: String? = null,
    divider: Boolean = true,
    trailing: (@Composable () -> Unit)? = null,
) {
    if (divider) HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.raised)
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.navRow)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Icon(icon, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconM))
        Column(Modifier.weight(1f)) {
            Text(title, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            if (sub != null) {
                Text(
                    sub,
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                    lineHeight = Tokens.Font.lineHeight,
                    modifier = Modifier.padding(top = Tokens.Space.xxs),
                )
            }
        }
        trailing?.invoke()
    }
}

/** Плашка-последствие, которое нельзя пропустить глазами: янтарная, [bad] — красная. */
@Composable
internal fun OnbWarn(text: String, bad: Boolean = false) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group))
            .background(if (bad) Tokens.Colors.stateBad else Tokens.Colors.stateWarn)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m + Tokens.Space.xxs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(
            TablerIcons.AlertTriangle,
            null,
            tint = if (bad) Tokens.Colors.failText else Tokens.Colors.stateWarnText,
            modifier = Modifier.size(Tokens.Size.iconRow),
        )
        Text(
            text,
            color = if (bad) Tokens.Colors.errorSoft else Tokens.Colors.stateWarnText,
            fontSize = Tokens.Font.preview,
            lineHeight = Tokens.Font.lineHeight,
        )
    }
}

/** Пояснение под группой. */
@Composable
internal fun OnbCaption(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        lineHeight = Tokens.Font.lineHeight,
        modifier = modifier.padding(horizontal = Tokens.Space.xs + Tokens.Space.xxs),
    )
}

/** Приветствие (`onboard/Main`): знак, имя, обещания без «мы», два входа. */
@Composable
private fun WelcomeStep(onStart: () -> Unit, onImport: () -> Unit) {
    Column(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.xl).padding(top = Tokens.Space.hero + Tokens.Space.s),
        ) {
            Icon(
                painterResource(R.drawable.ic_glyph),
                null,
                tint = Tokens.Colors.accent,
                modifier = Modifier.padding(bottom = Tokens.Space.xl).size(Tokens.Size.heroTile),
            )
            OnbTitle(stringResource(R.string.app_name), welcome = true)
            OnbLead(stringResource(R.string.onb_welcome_sub))
            Spacer(Modifier.height(Tokens.Space.xxl))
            OnbGroup {
                OnbRow(TablerIcons.PhoneOff, stringResource(R.string.onb_bullet_nophone), divider = false)
                OnbRow(TablerIcons.AddressBookOff, stringResource(R.string.onb_bullet_nocontacts))
                OnbRow(TablerIcons.MapPinOff, stringResource(R.string.onb_bullet_nolocation))
                OnbRow(TablerIcons.ServerOff, stringResource(R.string.onb_bullet_noserver))
            }
            Spacer(Modifier.height(Tokens.Space.xxl))
        }
        OnbFooter(
            stringResource(R.string.onb_start),
            onStart,
            stringResource(R.string.onb_have_key),
            onImport,
            note = {
                Text(
                    stringResource(R.string.onb_opensource),
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(bottom = Tokens.Space.s),
                )
            },
        )
    }
}

/** «Как подписаться» (`onboard/Profile`): фото и имя, можно пропустить. */
@Composable
internal fun ProfileStep(model: AppModel, onBack: (() -> Unit)?, bar: Int = OnbStep.Profile.bar, onNext: () -> Unit) {
    // Имя живёт в движке (M2): он его хранит и рассылает контактам. Не в prefs.
    var name by rememberSaveable { mutableStateOf(model.selfName) }
    // Системный Photo Picker (M2) → квадрат 512 → set_self_avatar; фото — из model.selfAvatar.
    val pickAvatar = rememberAvatarPicker(model)
    OnbPage(
        bar = bar,
        onBack = onBack,
        title = stringResource(R.string.onb_profile_title),
        sub = stringResource(R.string.onb_profile_sub),
        primary = stringResource(R.string.onb_next),
        onPrimary = {
            if (name.trim() != model.selfName) model.updateSelfName(name)
            onNext()
        },
        secondary = stringResource(R.string.onb_skip),
        onSecondary = onNext,
    ) {
        OwnPhoto(model.selfAvatar, stringResource(R.string.onb_pick_photo), pickAvatar)
        FormInput(name, stringResource(R.string.onb_name), Modifier.fillMaxWidth()) { name = it }
        OnbCaption(stringResource(R.string.onb_profile_hint), Modifier.padding(top = Tokens.Space.s))
    }
}

/** Своё фото 96 кругом со значком камеры; пустое — фигурка. Тап — выбрать фото. */
@Composable
private fun OwnPhoto(photo: ByteArray?, label: String, onPick: () -> Unit) {
    Box(
        Modifier.padding(top = Tokens.Space.xxl + Tokens.Space.xs, bottom = Tokens.Space.xl)
            .clickable(onClickLabel = label, role = Role.Button, onClick = onPick),
    ) {
        val bmp = photo?.let { remember(it) { runCatching { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }.getOrNull() } }
        Box(
            // Обводка — только у фото: у пустого круга она читалась ободком.
            Modifier.size(Tokens.Size.avatarHero).clip(CircleShape).background(Tokens.Colors.raised)
                .then(if (bmp != null) Modifier.imageOutline(CircleShape) else Modifier),
            contentAlignment = Alignment.Center,
        ) {
            if (bmp != null) {
                Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
            } else {
                Icon(TablerIcons.User, null, tint = Tokens.Colors.onbAvatarIcon, modifier = Modifier.size(Tokens.Size.iconAvatar))
            }
        }
        Box(
            Modifier.align(Alignment.BottomEnd).offset(x = Tokens.Space.xxs, y = Tokens.Space.xxs)
                .size(Tokens.Size.onbBadge).clip(CircleShape).background(Tokens.Colors.accent)
                .border(Tokens.Size.badgeRing, Tokens.Colors.bg, CircleShape),
            contentAlignment = Alignment.Center,
        ) { Icon(TablerIcons.Camera, null, tint = Tokens.Colors.accentDark, modifier = Modifier.size(Tokens.Size.iconS)) }
    }
}

/** «Копия ключа» (`onboard/KeyIntro`): зачем и как хранить; «Позже» — напоминание на главной. */
@Composable
private fun KeyIntroStep(onBack: () -> Unit, onShow: () -> Unit, onLater: () -> Unit) {
    // Единственный способ бэкапа — сид-фраза (бумага / менеджер паролей). Файловый экспорт
    // намеренно НЕ делаем: секрет в ФС = лишняя поверхность атаки + второй пароль.
    OnbPage(
        bar = OnbStep.Key.bar,
        onBack = onBack,
        icon = TablerIcons.Key,
        title = stringResource(R.string.onb_key_title),
        sub = stringResource(R.string.onb_key_sub),
        primary = stringResource(R.string.onb_show_phrase),
        onPrimary = onShow,
        secondary = stringResource(R.string.onb_skip_remind),
        onSecondary = onLater,
    ) {
        Spacer(Modifier.height(Tokens.Space.xxl))
        OnbGroup {
            OnbRow(TablerIcons.EyeOff, stringResource(R.string.onb_key_paper), stringResource(R.string.onb_key_paper_sub), divider = false)
            OnbRow(TablerIcons.ShieldCheck, stringResource(R.string.onb_key_secret), stringResource(R.string.onb_key_secret_sub))
        }
        Spacer(Modifier.height(Tokens.Space.l))
        OnbWarn(stringResource(R.string.onb_key_warn))
        Spacer(Modifier.height(Tokens.Space.xxl))
    }
}

/** «24 слова» (`onboard/Phrase`): сетка под FLAG_SECURE; слова живут, пока шаг на экране. */
@Composable
private fun PhraseStep(model: AppModel, onBack: () -> Unit, onWritten: () -> Unit) {
    SecureFlag()
    LaunchedEffect(Unit) { model.revealSeedPhrase() }
    DisposableEffect(Unit) { onDispose { model.clearRevealedPhrase() } }
    OnbPage(
        bar = OnbStep.Phrase.bar,
        onBack = onBack,
        title = stringResource(R.string.onb_phrase_title),
        sub = stringResource(R.string.onb_phrase_sub),
        primary = stringResource(R.string.onb_wrote_down),
        onPrimary = onWritten,
        note = {
            Row(
                Modifier.padding(bottom = Tokens.Space.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
            ) {
                Icon(TablerIcons.ShieldCheck, null, tint = Tokens.Colors.online, modifier = Modifier.size(Tokens.Size.iconXs))
                Text(stringResource(R.string.onb_screen_secure), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
            }
        },
    ) {
        Spacer(Modifier.height(Tokens.Space.l))
        PhraseGrid(model.revealedPhrase)
        Spacer(Modifier.height(Tokens.Space.l))
    }
}

/** Сетка 24 слов сид-фразы (макет `onboard/Phrase`). `phrase` = null — ещё грузится
 *  (читается через ops). */
@Composable
internal fun PhraseGrid(phrase: String?) {
    val words = phrase?.split(" ")?.filter { it.isNotBlank() } ?: emptyList()
    if (words.size < PHRASE_WORDS) {
        Box(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.group).padding(Tokens.Space.xl),
            Alignment.Center,
        ) {
            Text("…", color = Tokens.Colors.tertiary, fontSize = Tokens.Font.title)
        }
        return
    }
    PhraseLayout { i -> Text(words[i], color = Color.White, fontSize = Tokens.Font.name, fontWeight = FontWeight.Medium) }
}

/**
 * Раскладка 24 слов — одна на показ и ввод, чтобы запись на бумаге переносилась ячейка в
 * ячейку: два столбца по 12 (1–12 слева, 13–24 справа), номер у каждого; при крупном шрифте —
 * один столбец. По высоте — по содержимому.
 */
@Composable
private fun PhraseLayout(cell: @Composable RowScope.(Int) -> Unit) {
    val cols = if (LocalDensity.current.fontScale >= ONE_COLUMN_FONT) 1 else 2
    val perCol = PHRASE_WORDS / cols
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.group)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s + Tokens.Space.xxs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        for (col in 0 until cols) {
            Column(Modifier.weight(1f)) {
                for (row in 0 until perCol) {
                    val i = col * perCol + row
                    Row(
                        Modifier.heightIn(min = Tokens.Size.phraseRow),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s + Tokens.Space.xxs),
                    ) {
                        Text(
                            "${i + 1}",
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.small,
                            textAlign = TextAlign.End,
                            style = Tokens.Text.meta,
                            modifier = Modifier.width(Tokens.Size.phraseNum),
                        )
                        cell(i)
                    }
                }
            }
        }
    }
}

/**
 * Разрешения одним экраном (`onboard/Perms`) — нужные приложению ЦЕЛИКОМ, а не отдельному
 * экрану: уведомления (Android 13+; раньше разрешения не было), работа в фоне (узел сети при
 * погасшем экране) и автозапуск — только на прошивках, где такой экран есть ([Autostart]).
 * Камеру, микрофон и фото здесь НЕ просим — в момент нужды ([rememberPermissionRunner]).
 */
@Composable
private fun PermissionsStep(onBack: () -> Unit, onDone: () -> Unit) {
    val ctx = LocalContext.current
    val showNotif = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
    var notifGranted by remember {
        mutableStateOf(!showNotif || ctx.hasPermission(Manifest.permission.POST_NOTIFICATIONS))
    }
    val notifLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        notifGranted = it
    }
    // Состояние «в фоне» перечитываем после возврата из системных настроек.
    var bgGranted by remember { mutableStateOf(Background.ignoresBatteryOptimizations(ctx)) }
    val bgLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        bgGranted = Background.ignoresBatteryOptimizations(ctx)
    }
    val autostart = remember { Autostart.intent(ctx) }
    val vendor = remember { Background.vendor(ctx) }
    OnbPage(
        bar = OnbStep.Perms.bar,
        onBack = onBack,
        title = stringResource(R.string.onb_perms_title),
        sub = null,
        primary = stringResource(R.string.onb_done),
        onPrimary = onDone,
    ) {
        Spacer(Modifier.height(Tokens.Space.xxl))
        OnbGroup {
            if (showNotif) {
                PermRow(TablerIcons.Bell, R.string.onb_perm_notif, R.string.onb_perm_notif_desc, first = true, notifGranted, R.string.onb_allow) {
                    notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
            PermRow(TablerIcons.BatteryCharging, R.string.onb_perm_bg, R.string.onb_perm_bg_desc, first = !showNotif, bgGranted, R.string.onb_allow) {
                bgLauncher.launch(Background.batteryOptimizationIntent(ctx))
            }
            if (vendor != null) {
                // Системное «не оптимизировать» прошивку не убеждает (п. 50): на Xiaomi
                // останавливает «Контроль активности», пока там не «Нет ограничений».
                val (title, desc) = vendorStrings(vendor)
                PermRow(TablerIcons.BatteryCharging, title, desc, first = false, granted = false, R.string.onb_open) {
                    Background.openVendor(ctx, vendor)
                }
            }
            if (autostart != null) {
                // Включён ли автозапуск, прошивка не говорит — кнопка остаётся.
                PermRow(TablerIcons.Rocket, R.string.onb_perm_auto, R.string.onb_perm_auto_desc, first = false, granted = false, R.string.onb_open) {
                    Background.openAutostart(ctx, autostart)
                }
            }
        }
        Spacer(Modifier.height(Tokens.Space.xxl))
    }
}

/** Строка разрешения: выдано — «✓ Разрешено» (жать больше нечего), иначе пилюля действия. */
@Composable
private fun PermRow(icon: ImageVector, title: Int, desc: Int, first: Boolean, granted: Boolean, action: Int, onGrant: () -> Unit) {
    OnbRow(icon, stringResource(title), stringResource(desc), divider = !first) {
        if (granted) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs)) {
                Icon(TablerIcons.Check, null, tint = Tokens.Colors.online, modifier = Modifier.size(Tokens.Size.iconS))
                Text(stringResource(R.string.onb_granted), color = Tokens.Colors.online, fontSize = Tokens.Font.preview)
            }
        } else {
            PillButton(stringResource(action), onClick = onGrant)
        }
    }
}

/** Подписи строки батареи прошивки: на MIUI — её название экрана, иначе — общее. */
internal fun vendorStrings(v: Background.Vendor): Pair<Int, Int> = when (v) {
    Background.Vendor.MIUI -> R.string.bg_vendor_miui to R.string.bg_vendor_miui_desc
    Background.Vendor.APP_INFO -> R.string.bg_vendor_app to R.string.bg_vendor_app_desc
}
