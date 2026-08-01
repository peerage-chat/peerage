@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import android.view.WindowManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

// Онбординг (M4) — реализация по макету docs/design/mockups/onboarding.html.
// Единый скелет с фиксированными зонами: заголовок · описание · контент · короткая
// надпись · Готово · Пропустить — зоны совпадают по высоте, поэтому шаги не «прыгают».
// Строки — в strings.xml (EN/RU), ключи onb_*.

private val WarnBg = WarnColor.copy(alpha = 0.12f)
private val WarnBorder = WarnColor.copy(alpha = 0.40f)
private val WarnText = Color(0xFFE8C98A)

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

/** Пошаговый флоу первого запуска. `onFinish` — онбординг завершён (пометить флаг). */
@Composable
internal fun OnboardingFlow(model: AppModel, onFinish: () -> Unit) {
    var step by rememberSaveable { mutableStateOf(0) }
    var importing by rememberSaveable { mutableStateOf(false) }
    if (importing) {
        ImportKeyStep(model = model, onBack = { importing = false })
        return
    }
    val next: () -> Unit = { if (step < 3) step++ else onFinish() }
    when (step) {
        0 -> WelcomeStep(onStart = next, onImport = { importing = true })
        1 -> ProfileStep(model = model, onDone = next, onSkip = next)
        2 -> KeyBackupStep(model = model, onDone = next, onSkip = next)
        else -> PermissionsStep(onDone = onFinish)
    }
}

/** Восстановление личности из сид-фразы (M4 «У меня уже есть ключ»). Импорт пишет
 *  profile.json и перезапускает процесс — новая личность грузится начисто. */
@Composable
private fun ImportKeyStep(model: AppModel, onBack: () -> Unit) {
    val ctx = LocalContext.current
    var phrase by rememberSaveable { mutableStateOf("") }
    var error by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    OnbScaffold(
        title = stringResource(R.string.onb_restore_title),
        desc = stringResource(R.string.onb_restore_sub),
        primary = if (busy) stringResource(R.string.onb_checking) else stringResource(R.string.onb_restore),
        onPrimary = {
            if (!busy && phrase.isNotBlank()) {
                busy = true
                error = false
                model.importIdentity(ctx.filesDir.absolutePath, phrase) { ok ->
                    if (ok) {
                        // commit() (СИНХРОННЫЙ): apply() не успевает записаться на диск до
                        // System.exit в relaunchApp → флаг терялся, онбординг повторялся.
                        ctx.getSharedPreferences("p2p_settings", android.content.Context.MODE_PRIVATE)
                            .edit().putBoolean("onboarded", true).commit()
                        relaunchApp(ctx)
                    } else {
                        busy = false
                        error = true
                    }
                }
            }
        },
        secondary = stringResource(R.string.common_back),
        onSecondary = onBack,
        caption = {
            if (error) Text(stringResource(R.string.onb_bad_phrase), color = HoloError, fontSize = 11.sp)
        },
    ) {
        Box(
            Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile)
                .border(1.dp, if (error) HoloError else HoloBorder, RoundedCornerShape(14.dp)).padding(14.dp),
        ) {
            if (phrase.isEmpty()) Text(stringResource(R.string.onb_phrase_placeholder), color = HoloSecondary, fontSize = 15.sp)
            BasicTextField(
                value = phrase,
                onValueChange = {
                    phrase = it
                    error = false
                },
                textStyle = TextStyle(color = Color.White, fontSize = 15.sp, lineHeight = 24.sp),
                cursorBrush = SolidColor(HoloAccent),
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}

/** Полный перезапуск процесса (после импорта личности): новый profile.json грузится
 *  начисто, без стейл-состояния нативного узла/потоков. */
private fun relaunchApp(ctx: android.content.Context) {
    val intent = ctx.packageManager.getLaunchIntentForPackage(ctx.packageName)
    intent?.addFlags(android.content.Intent.FLAG_ACTIVITY_CLEAR_TOP or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
    ctx.startActivity(intent)
    Runtime.getRuntime().exit(0)
}

/** Единый скелет экрана онбординга (фиксированные зоны из макета). */
@Composable
private fun OnbScaffold(
    title: String,
    desc: String,
    primary: String,
    onPrimary: () -> Unit,
    secondary: String? = null,
    onSecondary: (() -> Unit)? = null,
    contentArrangement: Arrangement.Vertical = Arrangement.Center,
    caption: (@Composable RowScope.() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        Modifier.fillMaxSize().background(HoloBg).padding(horizontal = 24.dp).padding(top = 24.dp, bottom = 22.dp),
    ) {
        Box(Modifier.fillMaxWidth().height(56.dp), Alignment.Center) {
            Text(title, color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center)
        }
        Box(Modifier.fillMaxWidth().height(40.dp), Alignment.Center) {
            Text(desc, color = HoloSecondary, fontSize = 13.sp, textAlign = TextAlign.Center)
        }
        Column(
            Modifier.fillMaxWidth().weight(1f).padding(vertical = 8.dp),
            verticalArrangement = contentArrangement,
            horizontalAlignment = Alignment.CenterHorizontally,
            content = content,
        )
        Row(Modifier.fillMaxWidth().height(20.dp), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
            caption?.invoke(this)
        }
        Spacer(Modifier.height(12.dp))
        OnbButton(primary, filled = true, onClick = onPrimary)
        Spacer(Modifier.height(10.dp))
        // Слот secondary держим всегда (пустой резерв на последнем шаге) — чтобы primary
        // не прыгал по вертикали между шагами.
        if (secondary != null) {
            OnbButton(secondary, filled = false, onClick = onSecondary ?: {})
        } else {
            Spacer(Modifier.height(48.dp))
        }
    }
}

@Composable
private fun OnbButton(text: String, filled: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(Tokens.Radius.button)
    Box(
        Modifier.fillMaxWidth().height(48.dp).clip(shape)
            .then(if (filled) Modifier.background(HoloAccent) else Modifier.border(1.dp, HoloBorder, shape))
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text,
            color = if (filled) HoloAccentDark else Tokens.Colors.iconChrome,
            fontSize = 15.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

@Composable
private fun Bullet(icon: ImageVector, text: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 7.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, color = Color.White, fontSize = 14.sp)
    }
}

@Composable
private fun WelcomeStep(onStart: () -> Unit, onImport: () -> Unit) {
    OnbScaffold(
        title = stringResource(R.string.onb_welcome_title),
        desc = stringResource(R.string.onb_welcome_sub),
        primary = stringResource(R.string.onb_start),
        onPrimary = onStart,
        secondary = stringResource(R.string.onb_have_key),
        onSecondary = onImport,
        caption = { Text(stringResource(R.string.onb_opensource), color = HoloSecondary, fontSize = 11.sp) },
    ) {
        Box(
            Modifier.size(62.dp).clip(RoundedCornerShape(18.dp)).background(HoloAccentDark),
            contentAlignment = Alignment.Center,
        ) { Icon(TablerIcons.ShieldLock, null, tint = HoloAccent, modifier = Modifier.size(32.dp)) }
        Spacer(Modifier.height(8.dp))
        Bullet(TablerIcons.PhoneOff, stringResource(R.string.onb_bullet_nophone))
        Bullet(TablerIcons.AddressBookOff, stringResource(R.string.onb_bullet_nocontacts))
        Bullet(TablerIcons.MapPinOff, stringResource(R.string.onb_bullet_nolocation))
        Bullet(TablerIcons.ServerOff, stringResource(R.string.onb_bullet_noserver))
    }
}

@Composable
private fun ProfileStep(model: AppModel, onDone: () -> Unit, onSkip: () -> Unit) {
    val ctx = LocalContext.current
    // Имя живёт в движке (M2): он его хранит и рассылает контактам. Не в prefs.
    var name by rememberSaveable { mutableStateOf(model.selfName) }
    val save = { if (name.trim() != model.selfName) model.updateSelfName(name) }
    val scope = rememberCoroutineScope()
    // Системный Photo Picker (M2): выбор картинки → чтение байт в IO → set_self_avatar
    // (scrub + блоб на движке). Аватар реактивно показывается из model.selfAvatar.
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let { u ->
            scope.launch(Dispatchers.IO) {
                val bytes = ctx.contentResolver.openInputStream(u)?.use { it.readBytes() } ?: return@launch
                val mime = ctx.contentResolver.getType(u) ?: "image/jpeg"
                model.setSelfAvatar(bytes, mime)
            }
        }
    }
    OnbScaffold(
        title = stringResource(R.string.onb_profile_title),
        desc = stringResource(R.string.onb_profile_sub),
        primary = stringResource(R.string.onb_done),
        onPrimary = {
            save()
            onDone()
        },
        secondary = stringResource(R.string.onb_skip),
        onSecondary = onSkip,
        caption = { Text(stringResource(R.string.onb_profile_hint), color = HoloSecondary, fontSize = 11.sp) },
    ) {
        Box(
            contentAlignment = Alignment.BottomEnd,
            modifier = Modifier.clickable {
                picker.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
            },
        ) {
            Box(
                Modifier.size(96.dp).clip(RoundedCornerShape(26.dp)).background(Tokens.Colors.avatar),
                contentAlignment = Alignment.Center,
            ) {
                val av = model.selfAvatar
                val bmp = av?.let { remember(it) { runCatching { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }.getOrNull() } }
                if (bmp != null) {
                    Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
                } else {
                    Icon(TablerIcons.User, null, tint = Color(0xFF9AA0A6), modifier = Modifier.size(40.dp))
                }
            }
            Box(
                Modifier.size(34.dp).clip(RoundedCornerShape(12.dp)).background(HoloAccent).border(3.dp, HoloBg, RoundedCornerShape(12.dp)),
                contentAlignment = Alignment.Center,
            ) { Icon(TablerIcons.Camera, null, tint = HoloAccentDark, modifier = Modifier.size(17.dp)) }
        }
        Spacer(Modifier.height(14.dp))
        Row(
            Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile).padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(TablerIcons.User, null, tint = HoloSecondary, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(10.dp))
            val hint = stringResource(R.string.onb_name)
            BasicTextField(
                value = name,
                onValueChange = { name = it },
                singleLine = true,
                textStyle = TextStyle(color = Color.White, fontSize = 15.sp),
                cursorBrush = SolidColor(HoloAccent),
                modifier = Modifier.weight(1f),
                decorationBox = { inner ->
                    if (name.isEmpty()) Text(hint, color = HoloSecondary, fontSize = 15.sp)
                    inner()
                },
            )
        }
    }
}

@Composable
private fun KeyBackupStep(model: AppModel, onDone: () -> Unit, onSkip: () -> Unit) {
    var revealed by remember { mutableStateOf(false) }
    if (revealed) {
        SecureFlag() // секрет на экране — блокируем скриншот/запись/превью
        OnbScaffold(
            title = stringResource(R.string.onb_phrase_title),
            desc = stringResource(R.string.onb_phrase_sub),
            primary = stringResource(R.string.onb_wrote_down),
            onPrimary = {
                model.markKeyBackupDone()
                model.clearRevealedPhrase()
                onDone()
            },
            secondary = stringResource(R.string.common_back),
            onSecondary = {
                model.clearRevealedPhrase()
                revealed = false
            },
            caption = {
                Icon(TablerIcons.ShieldCheck, null, tint = HoloOnline, modifier = Modifier.size(13.dp))
                Spacer(Modifier.width(6.dp))
                Text(stringResource(R.string.onb_screen_secure), color = HoloOnline, fontSize = 11.sp)
            },
        ) { PhraseGrid(model.revealedPhrase) }
        return
    }
    // Единственный способ бэкапа — сид-фраза (по паперу/в менеджер паролей). Файловый
    // экспорт намеренно НЕ делаем: секрет в ФС = лишняя поверхность атаки + второй пароль
    // (см. обсуждение). Восстановление — вводом фразы («У меня уже есть ключ»).
    OnbScaffold(
        title = stringResource(R.string.onb_key_title),
        desc = stringResource(R.string.onb_key_sub),
        primary = stringResource(R.string.onb_show_phrase),
        onPrimary = {
            model.revealSeedPhrase()
            revealed = true
        },
        secondary = stringResource(R.string.onb_skip_remind),
        onSecondary = onSkip,
    ) {
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(WarnBg)
                .border(1.dp, WarnBorder, RoundedCornerShape(14.dp)).padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(TablerIcons.AlertTriangle, null, tint = WarnColor, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Text(
                stringResource(R.string.onb_key_warn),
                color = WarnText,
                fontSize = 13.sp,
                lineHeight = 18.sp,
            )
        }
    }
}

/** Сетка 24 слов сид-фразы: два столбца по 12, каждое пронумеровано. `phrase` = null —
 *  ещё грузится (читается через ops). */
@Composable
internal fun ColumnScope.PhraseGrid(phrase: String?) {
    val words = phrase?.split(" ")?.filter { it.isNotBlank() } ?: emptyList()
    if (words.size < 24) {
        Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
            Text("…", color = HoloSecondary, fontSize = 20.sp)
        }
        return
    }
    Row(
        Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile)
            .border(1.dp, HoloBorder, RoundedCornerShape(14.dp)).padding(14.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        for (col in 0 until 2) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.SpaceEvenly) {
                for (row in 0 until 12) {
                    val i = col * 12 + row
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("${i + 1}", color = HoloSecondary, fontSize = 12.sp, modifier = Modifier.width(22.dp))
                        Text(words[i], color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                    }
                }
            }
        }
    }
}

/**
 * Разрешения, нужные приложению ЦЕЛИКОМ, а не отдельному экрану: уведомления
 * (иначе входящие не видны) и работа в фоне (mesh-узел при погасшем экране).
 * Камеру/микрофон здесь НЕ просим — они запрашиваются в момент звонка/сканера
 * (см. [rememberPermissionRunner]).
 *
 * Карточка уведомлений показывается только на Android 13+: до неё разрешение
 * не существовало и выдавалось автоматически.
 */
@Composable
private fun PermissionsStep(onDone: () -> Unit) {
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

    OnbScaffold(
        title = stringResource(R.string.onb_perms_title),
        desc = stringResource(R.string.onb_perms_sub),
        primary = stringResource(R.string.onb_done),
        onPrimary = onDone,
        // Две карточки делят зону поровну; одна — не растягивается на весь экран,
        // а стоит по центру (иначе внутри карточки зияет пустота).
        contentArrangement = if (showNotif) Arrangement.spacedBy(12.dp) else Arrangement.Center,
    ) {
        if (showNotif) {
            PermCard(
                TablerIcons.Bell,
                stringResource(R.string.onb_perm_notif),
                stringResource(R.string.onb_perm_notif_desc),
                granted = notifGranted,
                fill = true,
            ) { notifLauncher.launch(Manifest.permission.POST_NOTIFICATIONS) }
        }
        PermCard(
            TablerIcons.BatteryCharging,
            stringResource(R.string.onb_perm_bg),
            stringResource(R.string.onb_perm_bg_desc),
            granted = bgGranted,
            fill = showNotif,
        ) { bgLauncher.launch(batteryOptimizationIntent(ctx)) }
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

/** Карточка разрешения. Выдано — вместо кнопок статичная отметка (жать больше нечего).
 *  `fill` — занять свою долю зоны (когда карточек несколько). */
@Composable
private fun ColumnScope.PermCard(
    icon: ImageVector,
    title: String,
    desc: String,
    granted: Boolean,
    fill: Boolean,
    onGrant: () -> Unit,
) {
    var dismissed by remember { mutableStateOf(false) }
    Column(
        Modifier.fillMaxWidth().then(if (fill) Modifier.weight(1f) else Modifier)
            .clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile)
            .border(1.dp, HoloBorder, RoundedCornerShape(14.dp)).padding(14.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(10.dp))
            Text(title, color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        }
        Spacer(Modifier.height(9.dp))
        Text(desc, color = HoloSecondary, fontSize = 12.5.sp, lineHeight = 17.sp)
        Spacer(Modifier.height(11.dp))
        // Высота зоны кнопок постоянна (38.dp) — карточки не «прыгают» после гранта.
        Box(Modifier.fillMaxWidth().height(38.dp), contentAlignment = Alignment.CenterStart) {
            when {
                granted -> Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(TablerIcons.Check, null, tint = HoloOnline, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.onb_granted), color = HoloOnline, fontSize = 13.sp)
                }
                dismissed -> Text(stringResource(R.string.onb_later_hint), color = HoloSecondary, fontSize = 12.5.sp)
                else -> Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box(
                        Modifier.weight(1f).height(38.dp).clip(RoundedCornerShape(11.dp)).background(HoloAccent)
                            .clickable(onClick = onGrant),
                        contentAlignment = Alignment.Center,
                    ) { Text(stringResource(R.string.onb_allow), color = HoloAccentDark, fontSize = 13.sp, fontWeight = FontWeight.Medium) }
                    Box(
                        Modifier.weight(1f).height(38.dp).clip(RoundedCornerShape(11.dp))
                            .border(1.dp, HoloBorder, RoundedCornerShape(11.dp)).clickable { dismissed = true },
                        contentAlignment = Alignment.Center,
                    ) { Text(stringResource(R.string.onb_later), color = HoloSecondary, fontSize = 13.sp) }
                }
            }
        }
    }
}
