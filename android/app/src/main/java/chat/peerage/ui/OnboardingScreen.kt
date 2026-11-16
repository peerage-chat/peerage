@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
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

/** Восстановление личности из сид-фразы. Импорт пишет profile.json и перезапускает
 *  процесс — новая личность грузится начисто. */
@Composable
private fun ImportKeyStep(model: AppModel, onBack: () -> Unit) {
    val ctx = LocalContext.current
    var phrase by rememberSaveable { mutableStateOf("") }
    var error by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    SecureFlag()
    OnbPage(
        bar = 0,
        onBack = onBack,
        title = stringResource(R.string.onb_restore_title),
        sub = stringResource(R.string.onb_restore_sub),
        primary = stringResource(if (busy) R.string.onb_checking else R.string.onb_restore),
        onPrimary = {
            if (!busy && phrase.isNotBlank()) {
                busy = true
                error = false
                model.importIdentity(ctx.filesDir.absolutePath, phrase) { ok ->
                    if (ok) {
                        // commit() (СИНХРОННЫЙ): apply() не успевает записаться на диск до
                        // System.exit в relaunchApp → флаг терялся, онбординг повторялся.
                        ctx.getSharedPreferences("settings", Context.MODE_PRIVATE)
                            .edit().putBoolean("onboarded", true).commit()
                        relaunchApp(ctx)
                    } else {
                        busy = false
                        error = true
                    }
                }
            }
        },
        scroll = false,
    ) {
        PhraseInput(phrase, error) {
            phrase = it
            error = false
        }
        if (error) OnbError(stringResource(R.string.onb_bad_phrase))
    }
}

/** Поле ввода 24 слов: импорт при первом запуске и экран восстановления. */
@Composable
private fun ColumnScope.PhraseInput(phrase: String, error: Boolean, onChange: (String) -> Unit) {
    val shape = RoundedCornerShape(Tokens.Radius.field)
    Box(
        Modifier.fillMaxWidth().weight(1f).padding(top = Tokens.Space.l).clip(shape).background(Tokens.Colors.group)
            .then(if (error) Modifier.border(Tokens.Size.focusRing, Tokens.Colors.error, shape) else Modifier)
            .padding(Tokens.Space.l),
    ) {
        if (phrase.isEmpty()) Text(stringResource(R.string.onb_phrase_placeholder), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name)
        BasicTextField(
            value = phrase,
            onValueChange = onChange,
            textStyle = Tokens.Text.base.copy(color = Color.White, fontSize = Tokens.Font.name, lineHeight = Tokens.Font.lineHeightLoose),
            cursorBrush = SolidColor(Tokens.Colors.accent),
            // 24 слова — это ключ целиком. Обычная клавиатура запоминает набранное,
            // подсказывает его потом в других приложениях и может синхронизировать
            // свой словарь в облако. Тип «пароль» это выключает; маски нет — слова
            // надо видеть, чтобы сверить с бумагой.
            keyboardOptions = PasswordKeyboard,
            modifier = Modifier.fillMaxSize(),
        )
    }
}

/** Ошибка под полем: красным, зачитывается без перебивания. */
@Composable
private fun OnbError(text: String) {
    Text(
        text,
        color = Tokens.Colors.error,
        fontSize = Tokens.Font.preview,
        modifier = Modifier.padding(start = Tokens.Space.xs, top = Tokens.Space.s).politeLive(),
    )
}

/**
 * Данные на устройстве не открываются (R-029): ключ хранилища потерян или не тот.
 * Без человека ничего не стирается — он выбирает: вернуть личность по 24 словам или
 * начать заново. Старые данные в обоих случаях откладываются, а не удаляются.
 */
@Composable
internal fun RecoveryScreen(model: AppModel, dataDir: String, why: Recovery) {
    val ctx = LocalContext.current
    var step by rememberSaveable { mutableStateOf(0) } // 0 — выбор, 1 — фраза, 2 — «начать заново?»
    var phrase by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<Int?>(null) }
    val back = {
        step = 0
        error = null
    }
    BackHandler(enabled = step != 0 && !busy) { back() }
    when (step) {
        1 -> {
            SecureFlag()
            OnbPage(
                bar = 0,
                onBack = back,
                title = stringResource(R.string.onb_restore_title),
                sub = stringResource(R.string.onb_restore_sub),
                primary = stringResource(if (busy) R.string.onb_checking else R.string.onb_restore),
                onPrimary = {
                    if (!busy && phrase.isNotBlank()) {
                        busy = true
                        error = null
                        model.recoverFromPhrase(dataDir, phrase) { r ->
                            busy = false
                            when (r) {
                                RecoverResult.OK -> relaunchApp(ctx)
                                RecoverResult.BAD_PHRASE -> error = R.string.onb_bad_phrase
                                RecoverResult.FAILED -> error = R.string.recovery_failed
                            }
                        }
                    }
                },
                scroll = false,
            ) {
                PhraseInput(phrase, error == R.string.onb_bad_phrase) {
                    phrase = it
                    error = null
                }
                error?.let { OnbError(stringResource(it)) }
            }
        }
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
            iconBg = Tokens.Colors.stateWarn,
            title = stringResource(R.string.recovery_title),
            sub = stringResource(R.string.recovery_sub),
            primary = stringResource(R.string.recovery_restore),
            onPrimary = { step = 1 },
            secondary = stringResource(R.string.recovery_start_over),
            onSecondary = { step = 2 },
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            val reason = if (why == Recovery.KEY_LOST) R.string.recovery_reason_key_lost else R.string.recovery_reason_mismatch
            OnbGroup {
                OnbRow(TablerIcons.Lock, stringResource(reason), stringResource(R.string.recovery_body), divider = false)
            }
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
private fun OnbPage(
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
        OnbFooter(primary, onPrimary, secondary, onSecondary, note)
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
) {
    Column(
        Modifier.fillMaxWidth().navigationBarsPadding()
            .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m, bottom = Tokens.Space.xxl),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        note?.invoke()
        AccentButton(primary, Modifier.fillMaxWidth().heightIn(min = Tokens.Size.primaryButton), onClick = onPrimary)
        if (secondary != null) GhostButton(secondary, modifier = Modifier.fillMaxWidth(), onClick = onSecondary)
    }
}

/** Группа строк шага (макет `.grp`): ступень group, скругление 18. */
@Composable
private fun OnbGroup(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.group), content = content)
}

/** Строка группы (макет `.gr`): иконка без плитки, название, пояснение, справа — действие. */
@Composable
private fun OnbRow(
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

/** Янтарная плашка: последствие, которое нельзя пропустить глазами. */
@Composable
private fun OnbWarn(text: String) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.stateWarn)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m + Tokens.Space.xxs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(TablerIcons.AlertTriangle, null, tint = Tokens.Colors.stateWarnText, modifier = Modifier.size(Tokens.Size.iconRow))
        Text(text, color = Tokens.Colors.stateWarnText, fontSize = Tokens.Font.preview, lineHeight = Tokens.Font.lineHeight)
    }
}

/** Пояснение под группой. */
@Composable
private fun OnbCaption(text: String, modifier: Modifier = Modifier) {
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
                OnbRow(TablerIcons.ServerOff, stringResource(R.string.onb_bullet_noserver), stringResource(R.string.onb_bullet_noserver_sub))
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
private fun ProfileStep(model: AppModel, onBack: () -> Unit, onNext: () -> Unit) {
    // Имя живёт в движке (M2): он его хранит и рассылает контактам. Не в prefs.
    var name by rememberSaveable { mutableStateOf(model.selfName) }
    // Системный Photo Picker (M2) → квадрат 512 → set_self_avatar; фото — из model.selfAvatar.
    val pickAvatar = rememberAvatarPicker(model)
    OnbPage(
        bar = OnbStep.Profile.bar,
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

/** Сетка 24 слов сид-фразы (макет `onboard/Phrase`): два столбца по 12 — 1–12 слева,
 *  13–24 справа, — каждое пронумеровано. По высоте — по содержимому, без растяжения.
 *  `phrase` = null — ещё грузится (читается через ops). */
@Composable
internal fun PhraseGrid(phrase: String?) {
    val words = phrase?.split(" ")?.filter { it.isNotBlank() } ?: emptyList()
    val shape = RoundedCornerShape(Tokens.Radius.group)
    if (words.size < 24) {
        Box(Modifier.fillMaxWidth().clip(shape).background(Tokens.Colors.group).padding(Tokens.Space.xl), Alignment.Center) {
            Text("…", color = Tokens.Colors.tertiary, fontSize = Tokens.Font.title)
        }
        return
    }
    Row(
        Modifier.fillMaxWidth().clip(shape).background(Tokens.Colors.group)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s + Tokens.Space.xxs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        for (col in 0 until 2) {
            Column(Modifier.weight(1f)) {
                for (row in 0 until 12) {
                    val i = col * 12 + row
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
                        Text(words[i], color = Color.White, fontSize = Tokens.Font.name, fontWeight = FontWeight.Medium)
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
    var bgGranted by remember { mutableStateOf(ctx.ignoresBatteryOptimizations()) }
    val bgLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        bgGranted = ctx.ignoresBatteryOptimizations()
    }
    val autostart = remember { Autostart.intent(ctx) }
    OnbPage(
        bar = OnbStep.Perms.bar,
        onBack = onBack,
        title = stringResource(R.string.onb_perms_title),
        sub = stringResource(R.string.onb_perms_sub),
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
                bgLauncher.launch(batteryOptimizationIntent(ctx))
            }
            if (autostart != null) {
                // Включён ли автозапуск, прошивка не говорит — кнопка остаётся.
                PermRow(TablerIcons.Rocket, R.string.onb_perm_auto, R.string.onb_perm_auto_desc, first = false, granted = false, R.string.onb_open) {
                    runCatching { ctx.startActivity(autostart) }
                }
            }
        }
        OnbCaption(stringResource(R.string.onb_perms_caption), Modifier.padding(top = Tokens.Space.s))
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

/** Приложение уже выведено из-под оптимизации батареи? */
private fun Context.ignoresBatteryOptimizations(): Boolean {
    val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
    return pm.isIgnoringBatteryOptimizations(packageName)
}

/**
 * Прямой диалог «не оптимизировать» (требует REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).
 *
 * На части прошивок (проверено на MIUI) этот экшен ловят ДВА обработчика, и вместо
 * диалога система показывает выбор «Что использовать?». Поэтому целимся явно в
 * системные настройки; если их нет — отдаём неявный интент, а в крайнем случае
 * общий список батарейных исключений (он есть всегда).
 */
private fun batteryOptimizationIntent(ctx: Context): Intent {
    val direct = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${ctx.packageName}"))
    val settings = Intent(direct).setPackage("com.android.settings")
    return when {
        settings.resolveActivity(ctx.packageManager) != null -> settings
        direct.resolveActivity(ctx.packageManager) != null -> direct
        else -> Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
    }
}
