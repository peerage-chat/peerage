package dev.p2p.messenger

import android.Manifest
import android.content.Context
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.ui.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.reflect.KProperty

class MainActivity : ComponentActivity() {
    // Синглтон на процесс: переживает пересоздание Activity (фон/возврат), иначе
    // утекают второй нативный узел и дублирующие poll/ticker-циклы.
    private val model = AppModel.instance

    @OptIn(ExperimentalMaterial3WindowSizeClassApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model.appContext = applicationContext
        AppLock.load(applicationContext)
        // Разрешения НЕ просим здесь: во время онбординга системный диалог не нужен.
        // Запрашиваются в App() после гейта (onboarded) — см. requestCorePermissions.
        val dataDir = filesDir.absolutePath
        setContent {
            // ADR-005: класс ширины окна читается ЗДЕСЬ (единственная точка), дальше по
            // приложению ходит наш enum WidthClass — experimental-API не течёт в UI-код.
            val width = when (calculateWindowSizeClass(this@MainActivity).widthSizeClass) {
                WindowWidthSizeClass.Expanded -> WidthClass.Expanded
                WindowWidthSizeClass.Medium -> WidthClass.Medium
                else -> WidthClass.Compact
            }
            P2PTheme {
                // Замок рисуется ПОВЕРХ и вместо содержимого: под ним не должно
                // мелькать ни списка чатов, ни превью сообщений.
                if (AppLock.locked) {
                    LockScreen { askCredential() }
                } else {
                    App(model, dataDir, width)
                }
            }
        }
    }

    /** Запрос системного подтверждения личности (биометрия/PIN устройства). */
    private fun askCredential() {
        val intent = AppLock.credentialIntent(this)
        if (intent == null) {
            // Экран блокировки на устройстве сняли, пока приложение было в фоне:
            // подтверждать нечем, держать пользователя взаперти бессмысленно.
            AppLock.unlock()
            return
        }
        credentialLauncher.launch(intent)
    }

    private val credentialLauncher = registerForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.StartActivityForResult(),
    ) { res ->
        if (res.resultCode == RESULT_OK) AppLock.unlock()
    }

    override fun onStart() {
        super.onStart()
        AppLock.onForeground()
        if (AppLock.locked) askCredential()
        model.foreground = true
        // Поднимаем mesh-узел при выходе на передний план (в т.ч. после кнопки
        // «Отключить» в уведомлении): держим процесс/сеть живыми при погасшем экране.
        ConnService.start(applicationContext)
    }

    override fun onStop() {
        super.onStop()
        AppLock.onBackground()
        // При включённой блокировке прячем и снимок в «недавних»: иначе переписка
        // видна любому, кто открыл список задач, — замок в такой паре бесполезен.
        if (AppLock.enabled) {
            window.setFlags(
                android.view.WindowManager.LayoutParams.FLAG_SECURE,
                android.view.WindowManager.LayoutParams.FLAG_SECURE,
            )
        }
        // В фоне троттлим поллинг (см. AppModel) — меньше пробуждений CPU.
        model.foreground = false
    }

    override fun onResume() {
        super.onResume()
        // Снимаем FLAG_SECURE, только когда содержимое уже разблокировано.
        if (!AppLock.locked) {
            window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE)
        }
    }
}

@Composable
internal fun App(model: AppModel, dataDir: String, width: WidthClass) {
    // Инициализация ядра на фоне; опрос событий/тикер статуса model запускает сам
    // на выделенных потоках (см. AppModel — UI ничего нативного не вызывает напрямую).
    // Уровень at-rest с паролем: движок не стартует, пока ключ не отперт. Показывать
    // пустой мессенджер вместо запроса — врать про состояние данных.
    val needsPassword = remember { VaultKeyManager.needsPassword(java.io.File(dataDir)) }
    var vaultOpen by remember { mutableStateOf(!needsPassword) }
    if (!vaultOpen) {
        val scope = rememberCoroutineScope()
        VaultUnlockScreen { password, done ->
            scope.launch(Dispatchers.IO) {
                val ok = model.start(dataDir, password)
                done(ok)
                if (ok) vaultOpen = true
            }
        }
        return
    }
    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) { model.start(dataDir) }
    }

    // Гейт первого запуска (M4). Флаг в SharedPreferences; существующим пользователям
    // (уже есть контакты) онбординг НЕ показываем — тихая миграция.
    val ctx = LocalContext.current
    val prefs = ctx.getSharedPreferences("p2p_settings", Context.MODE_PRIVATE)
    var onboarded by remember { mutableStateOf(prefs.getBoolean("onboarded", false)) }
    LaunchedEffect(model.me, model.contacts.size) {
        if (!onboarded && model.me != null && model.contacts.isNotEmpty()) {
            prefs.edit().putBoolean("onboarded", true).apply()
            onboarded = true
        }
    }

    // Камеру и микрофон здесь НЕ просим: они запрашиваются контекстно, в момент
    // использования (звонок / включение видео / сканер QR) — см. rememberPermissionRunner.
    // Здесь остаются только уведомления: они нужны приложению целиком, а не отдельному
    // экрану, и просить их можно лишь после гейта (иначе диалог накроет онбординг).
    // Прошедшие онбординг жмут «Разрешить» в его карточке; сюда попадают те, кому
    // онбординг не показывался (тихая миграция существующих пользователей).
    val permLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    LaunchedEffect(onboarded) {
        if (onboarded && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            !ctx.hasPermission(Manifest.permission.POST_NOTIFICATIONS)
        ) {
            permLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    when {
        model.error != null -> CenterMessage(stringResource(R.string.app_error, model.error.orEmpty()))
        model.me == null -> CenterMessage(stringResource(R.string.app_connecting))
        !onboarded -> OnboardingFlow(model) {
            prefs.edit().putBoolean("onboarded", true).apply()
            onboarded = true
        }
        else -> Root(model, width)
    }
}

@Composable
internal fun CenterMessage(text: String) {
    Box(Modifier.fillMaxSize().background(HoloBg), contentAlignment = Alignment.Center) {
        Text(text, color = HoloSecondary, fontSize = 14.sp)
    }
}

internal enum class Screen {
    Contacts,
    Chat,
    Add,
    Me,
    Info,
    Groups,
    GroupChat,
    CreateGroup,
    AddMember,
    Settings,
    Scanner,
    ContactList,
    Calls,
    Network,
    Saved,
    Search,
    NewChat,
    Storage,
    Outbox,
    Folders,
    FolderEdit,
    Flood,
    RotateAddress,
    EditProfile,
    VaultLevel,
    Backup,

    // Настройки разложены по категориям: корень — оглавление, содержимое живёт здесь.
    SettingsSecurity,
    SettingsPrivacy,
    SettingsMedia,
    SettingsNotifications,
    SettingsAbout,
}

/**
 * Текущий экран + откуда на него пришли. Делегат для `var screen by nav`: каждое
 * `screen = X` запоминает предыдущий экран как источник X, если это переход вперёд.
 *
 * Зачем: часть экранов открывается из нескольких мест («Сеть» — с главной, из
 * настроек и из «Я»; «Контакты» — из «Безопасности»). Зашитый родитель возвращал
 * на главную, откуда бы ни пришли. Полноценный стек не нужен — после «создать группу»
 * назад из группы должно вести на главную, а не в форму создания, поэтому источник
 * учитывают только экраны-разделы ([fromOrigin]), остальные держат своего родителя.
 */
internal class NavState(initial: Screen) {
    var current by mutableStateOf(initial)
        private set
    private val cameFrom = HashMap<Screen, Screen>()

    operator fun getValue(thisRef: Any?, property: KProperty<*>): Screen = current

    operator fun setValue(thisRef: Any?, property: KProperty<*>, value: Screen) {
        if (value == current) return
        when {
            // Главная — корень: дальше начинается новая история.
            value == Screen.Contacts -> cameFrom.clear()
            // Возврат к тому, откуда пришли, — не новый вход: источник не трогаем.
            cameFrom[current] == value -> {}
            else -> cameFrom[value] = current
        }
        current = value
    }

    /** Куда ведёт «назад» с экрана [s] — одно правило и для системной кнопки, и для
     *  стрелки в шапке. */
    fun parentOf(s: Screen): Screen = when (s) {
        Screen.Scanner -> Screen.Add
        Screen.CreateGroup -> Screen.NewChat
        Screen.SettingsSecurity, Screen.SettingsPrivacy, Screen.SettingsMedia,
        Screen.SettingsNotifications, Screen.SettingsAbout,
        -> Screen.Settings
        Screen.AddMember -> Screen.GroupChat
        Screen.Info -> Screen.Chat
        Screen.Flood, Screen.Outbox -> Screen.Network
        Screen.RotateAddress -> Screen.Flood
        Screen.Backup -> Screen.Storage
        Screen.FolderEdit -> Screen.Folders
        Screen.VaultLevel -> cameFrom[s] ?: Screen.SettingsSecurity
        Screen.EditProfile -> cameFrom[s] ?: Screen.Settings
        in fromOrigin -> cameFrom[s] ?: Screen.Contacts
        else -> Screen.Contacts
    }

    private companion object {
        /** Разделы, открываемые из разных мест: «назад» ведёт туда, откуда пришли. */
        val fromOrigin = setOf(
            Screen.Settings, Screen.Network, Screen.Storage, Screen.ContactList,
            Screen.Calls, Screen.Saved, Screen.Folders, Screen.Groups, Screen.Me,
        )
    }
}

/** Класс ширины окна (ADR-005). Свой enum вместо material3 WindowWidthSizeClass, чтобы
 *  experimental-API жил только в MainActivity, а UI зависел от стабильного типа. */
internal enum class WidthClass { Compact, Medium, Expanded }

/** Диспетчер оболочки по ширине окна (ADR-005). v1.0 реализует только Compact; Medium/
 *  Expanded — швы на будущее (пока рендерят Compact-оболочку, без переписывания контента). */
@Composable
private fun Root(model: AppModel, width: WidthClass) {
    when (width) {
        // TODO(ADR-005): MediumShell (nav-rail + одна панель), ExpandedShell (rail+список+детали).
        // Контент-экраны переиспользуются как есть — добавляется только новая оболочка.
        WidthClass.Compact, WidthClass.Medium, WidthClass.Expanded -> CompactShell(model)
    }
}

@OptIn(ExperimentalMaterial3Api::class, androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun CompactShell(model: AppModel) {
    val activity = LocalContext.current as? android.app.Activity
    // Кнопка «Сменить адрес» в уведомлении о флуде открывает сразу нужный экран:
    // человек уже принял решение, незачем заставлять его искать пункт меню.
    // Каталог данных — тот же, что при старте движка; берём из контекста, чтобы не
    // тащить его параметром через оболочку.
    val dataDir = LocalContext.current.filesDir.absolutePath
    val nav = remember {
        val fromNotif = activity?.intent?.getBooleanExtra(FloodNotifier.EXTRA_OPEN_ROTATE, false) == true
        NavState(if (fromNotif) Screen.RotateAddress else Screen.Contacts)
    }
    var screen by nav
    // id редактируемой папки (null — создаём новую).
    var editFolder by remember { mutableStateOf<String?>(null) }
    // Ключ контакта, которого пришли СВЕРЯТЬ сканером (null — обычное добавление).
    var verifyingPeer by remember { mutableStateOf<String?>(null) }
    val ctx = LocalContext.current
    // ADR-004: нас отозвали из открытой группы (currentGroup → null) — уходим на список.
    LaunchedEffect(model.currentGroup) {
        if (model.currentGroup == null && (screen == Screen.GroupChat || screen == Screen.AddMember)) {
            screen = Screen.Groups
        }
    }
    var scanResult by remember { mutableStateOf<String?>(null) }
    // Папки: единственный источник правды — страница пейджера. Таб и заголовок только
    // читают её (targetPage — куда пейджер движется, поэтому таб переключается сразу,
    // а не после остановки), тап по табу только листает. Раньше таб и страница
    // синхронизировались двумя эффектами навстречу друг другу, и при быстрых
    // действиях опоздавший эффект утаскивал страницу назад посреди жеста.
    val folders = folderKeys(model)
    val folderPager = rememberPagerState { folders.size }
    val activeFilter = folders.getOrElse(folderPager.targetPage) { "all" }
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val selectFolder: (String) -> Unit = { key ->
        val i = folders.indexOf(key)
        if (i >= 0) scope.launch { folderPager.animateScrollToPage(i) }
    }

    // Системная «назад» — навигация по приложению, а не выход. Во время звонка/
    // входящего гасим (не выходим), открытое меню закрываем, иначе возврат к родителю.
    // На главном (Contacts) без оверлеев — обработчик выключен, back выходит штатно.
    BackHandler(
        enabled = model.dmCallPeer != null || model.incomingCall != null || screen != Screen.Contacts,
    ) {
        when {
            model.incomingCall != null -> {}
            model.dmCallPeer != null -> {}
            screen == Screen.Scanner && verifyingPeer != null -> {
                verifyingPeer = null
                screen = Screen.Info
            }
            else -> screen = nav.parentOf(screen)
        }
    }
    // Стрелка в шапке ведёт туда же, куда системная «назад».
    val goBack: () -> Unit = { screen = nav.parentOf(screen) }

    val navigate: (Screen) -> Unit = { dest ->
        if (dest == Screen.Groups) model.refreshGroups()
        screen = dest
    }

    // Выдвижной панели больше нет: разделы и профиль живут на своём экране «Я»
    // (Screen.Me), который открывается аватаром в шапке. Панель делала две работы
    // сразу — показывала профиль и раздавала разделы, — и профилю в её шапке было
    // тесно. Свайпом «Я» не открывается намеренно: горизонталь на главной отдана
    // переключению папок, третий горизонтальный жест дрался бы с ними.
    run {
        Box(Modifier.fillMaxSize()) {
            Surface(Modifier.fillMaxSize(), color = HoloBg) {
                Column(Modifier.fillMaxSize()) {
                    // ── Шапка: конфигурируется по текущему экрану ─────────────────────
                    when (screen) {
                        Screen.Contacts -> TopBar(
                            if (activeFilter == "all") stringResource(R.string.nav_chats) else folderTitle(model, activeFilter),
                            leading = {
                                // Аватар как вход в профиль: он же и говорит, под кем
                                // ты сейчас, — гамбургер об этом молчал.
                                // Своего отступа слева НЕТ: 8dp даёт сама шапка, и
                                // аватар встаёт ровно по той же вертикали, что
                                // карточки и табы ниже. Свой добавочный отступ
                                // уводил его вправо, и левый край экрана «ступенькой»
                                // расходился сверху вниз.
                                Box(
                                    Modifier.padding(end = Tokens.Space.m)
                                        .clickable { screen = Screen.Me },
                                ) {
                                    SquareAvatar(
                                        model.selfName.ifBlank { model.me?.pubkey ?: "?" },
                                        size = 38.dp,
                                        avatarBytes = model.selfAvatar,
                                        circle = true,
                                        ring = HoloBg,
                                    )
                                }
                            },
                        ) {
                            PeerIndicator(
                                model.netPeers,
                                onTap = { screen = Screen.Network },
                                onLongPress = { model.startBenchRecv() },
                            )
                            BarIcon(TablerIcons.Search, stringResource(R.string.cd_search)) { screen = Screen.Search }
                        }
                        Screen.Chat -> ChatTopBar(
                            model,
                            onBack = { screen = Screen.Contacts },
                            onProfile = { screen = Screen.Info },
                        )
                        Screen.Add -> TopBar(stringResource(R.string.title_add_contact), onBack = { screen = Screen.Contacts })
                        Screen.Me -> TopBar(stringResource(R.string.me_title), onBack = goBack)
                        Screen.Info -> TopBar(stringResource(R.string.common_contact), onBack = { screen = Screen.Chat })
                        Screen.Groups -> TopBar(stringResource(R.string.title_groups), onBack = goBack) {
                            BarIcon(Icons.Filled.Add, stringResource(R.string.cd_new_group)) { screen = Screen.CreateGroup }
                        }
                        Screen.GroupChat -> GroupChatTopBar(
                            model,
                            onBack = { screen = Screen.Contacts },
                            onInfo = { screen = Screen.AddMember },
                        )
                        Screen.CreateGroup -> TopBar(stringResource(R.string.title_new_group), onBack = { screen = Screen.NewChat })
                        Screen.NewChat -> TopBar(stringResource(R.string.newchat_title), onBack = { screen = Screen.Contacts })
                        Screen.AddMember -> TopBar(stringResource(R.string.title_add_member), onBack = { screen = Screen.GroupChat })
                        Screen.Settings -> TopBar(stringResource(R.string.menu_settings), onBack = goBack)
                        Screen.SettingsSecurity -> TopBar(stringResource(R.string.settings_cat_security), onBack = goBack)
                        Screen.SettingsPrivacy -> TopBar(stringResource(R.string.settings_cat_privacy), onBack = goBack)
                        Screen.SettingsMedia -> TopBar(stringResource(R.string.settings_cat_media), onBack = goBack)
                        Screen.SettingsNotifications -> TopBar(stringResource(R.string.settings_cat_notifications), onBack = goBack)
                        Screen.SettingsAbout -> TopBar(stringResource(R.string.settings_cat_about), onBack = goBack)
                        Screen.Scanner -> TopBar(
                            stringResource(if (verifyingPeer != null) R.string.title_verify_qr else R.string.title_scan_qr),
                            onBack = {
                                screen = if (verifyingPeer != null) Screen.Info else Screen.Add
                                verifyingPeer = null
                            },
                        )
                        Screen.ContactList -> TopBar(stringResource(R.string.menu_contacts), onBack = goBack) {
                            BarIcon(Icons.Filled.PersonAdd, stringResource(R.string.cd_add_contact)) { screen = Screen.Add }
                        }
                        Screen.Calls -> TopBar(stringResource(R.string.menu_calls), onBack = goBack)
                        Screen.Network -> TopBar(stringResource(R.string.menu_network), onBack = goBack) {
                            BarIcon(Icons.Filled.Refresh, stringResource(R.string.cd_refresh)) { /* авто-поллинг; ручной рефреш — TODO */ }
                        }
                        Screen.EditProfile -> TopBar(stringResource(R.string.settings_edit_profile), onBack = goBack)
                        Screen.VaultLevel -> TopBar(stringResource(R.string.settings_at_rest), onBack = goBack)
                        Screen.Flood -> TopBar(stringResource(R.string.flood_screen_title), onBack = goBack)
                        Screen.RotateAddress -> TopBar(stringResource(R.string.rotate_title), onBack = goBack)
                        Screen.Saved -> TopBar(stringResource(R.string.menu_saved), onBack = goBack)
                        Screen.Storage -> TopBar(stringResource(R.string.menu_storage), onBack = goBack)
                        Screen.Backup -> TopBar(stringResource(R.string.backup_title), onBack = goBack)
                        Screen.Outbox -> TopBar(stringResource(R.string.title_outbox), onBack = goBack)
                        Screen.Folders -> TopBar(stringResource(R.string.folders_title), onBack = goBack)
                        Screen.FolderEdit -> TopBar(stringResource(R.string.folder_edit_title), onBack = goBack)
                        Screen.Search -> TopBar(stringResource(R.string.title_search), onBack = { screen = Screen.Contacts })
                    }
                    // ── Папки табами: только на главном экране ──────────────────────
                    if (screen == Screen.Contacts) {
                        FolderTabs(
                            model,
                            activeFilter,
                            onFilter = selectFolder,
                            onManage = { screen = Screen.Folders },
                        )
                    }
                    // ── Контент экрана (без шапки) ────────────────────────────────────
                    // Перехода между экранами НЕ анимируем: Crossfade держал бы оба
                    // экрана смонтированными, а у наших есть эффекты (камера, запись,
                    // поллинг, автоскролл) — они шли бы парой. Анимируем то, что внутри.
                    Box(Modifier.weight(1f)) {
                        when (screen) {
                            Screen.Contacts -> ChatsPager(
                                model,
                                folderPager,
                                onOpenDm = {
                                    model.openChat(it)
                                    screen = Screen.Chat
                                },
                                onOpenGroup = {
                                    model.openGroup(it)
                                    screen = Screen.GroupChat
                                },
                            )
                            Screen.Chat -> ChatScreen(model)
                            Screen.Add -> AddScreen(
                                model,
                                scannedKey = scanResult,
                                onScan = { screen = Screen.Scanner },
                            ) { screen = Screen.Contacts }
                            Screen.Scanner -> QrScannerScreen { key ->
                                val target = verifyingPeer
                                if (target == null) {
                                    scanResult = key
                                    screen = Screen.Add
                                } else {
                                    // Сверка: сканируем QR собеседника и сравниваем с
                                    // ключом, который у нас записан. Совпало — метим
                                    // проверенным; не совпало — ГРОМКО говорим об этом,
                                    // это ровно тот случай, ради которого сверка и нужна.
                                    verifyingPeer = null
                                    val ok = key.equals(target, ignoreCase = true)
                                    if (ok && model.contacts.firstOrNull { it.key == target }?.verified != true) {
                                        model.toggleVerified(target)
                                    }
                                    android.widget.Toast.makeText(
                                        ctx,
                                        ctx.getString(if (ok) R.string.verify_match else R.string.verify_mismatch),
                                        if (ok) android.widget.Toast.LENGTH_SHORT else android.widget.Toast.LENGTH_LONG,
                                    ).show()
                                    screen = Screen.Info
                                }
                            }
                            Screen.Me -> MeScreen(model, navigate)
                            Screen.Info -> ContactProfileScreen(
                                model,
                                onMessage = { screen = Screen.Chat },
                                onScanQr = {
                                    verifyingPeer = model.currentChat
                                    screen = Screen.Scanner
                                },
                                onDeleted = { screen = Screen.Contacts },
                            )
                            Screen.Groups -> GroupsScreen(model) {
                                model.openGroup(it)
                                screen = Screen.GroupChat
                            }
                            Screen.GroupChat -> GroupChatScreen(model)
                            Screen.CreateGroup -> CreateGroupScreen(model) { screen = Screen.Contacts }
                            Screen.NewChat -> NewChatScreen(
                                model,
                                onNewGroup = { screen = Screen.CreateGroup },
                                onAddContact = { screen = Screen.Add },
                                onOpenDm = {
                                    model.openChat(it)
                                    screen = Screen.Chat
                                },
                            )
                            Screen.AddMember -> AddMemberScreen(model) { screen = Screen.GroupChat }
                            Screen.Settings -> SettingsScreen(
                                onSecurity = { screen = Screen.SettingsSecurity },
                                onPrivacy = { screen = Screen.SettingsPrivacy },
                                onMedia = { screen = Screen.SettingsMedia },
                                onNotifications = { screen = Screen.SettingsNotifications },
                                onAbout = { screen = Screen.SettingsAbout },
                                onEditProfile = { screen = Screen.EditProfile },
                                onNetwork = { screen = Screen.Network },
                                onStorage = { screen = Screen.Storage },
                            )
                            Screen.SettingsSecurity -> SettingsSecurityScreen(
                                model,
                                onVaultLevel = { screen = Screen.VaultLevel },
                                onContacts = { screen = Screen.ContactList },
                                vaultLevel = VaultKeyManager.level(java.io.File(dataDir)),
                            )
                            Screen.SettingsPrivacy -> SettingsPrivacyScreen()
                            Screen.SettingsMedia -> SettingsMediaScreen(model)
                            Screen.SettingsNotifications -> SettingsNotificationsScreen(model)
                            Screen.SettingsAbout -> SettingsAboutScreen()
                            Screen.ContactList -> ContactListScreen(model) {
                                model.openChat(it)
                                screen = Screen.Chat
                            }
                            Screen.Calls -> CallsScreen(
                                model,
                                onCallPeer = { model.callDm(it) },
                                onJoinRoom = {
                                    model.openGroup(it)
                                    model.joinRoom(it)
                                    screen = Screen.GroupChat
                                },
                            )
                            Screen.Network -> NetworkScreen(
                                model,
                                onMyQr = { screen = Screen.Me },
                                onOpenOutbox = { screen = Screen.Outbox },
                                onFlood = { screen = Screen.Flood },
                                onRotate = { screen = Screen.RotateAddress },
                            )
                            Screen.Flood -> FloodScreen(model) { screen = Screen.RotateAddress }
                            // Пока идёт смена адреса, экран выбора подменяется ходом
                            // операции: пользователь минуту офлайн и должен видеть, почему.
                            Screen.RotateAddress -> if (model.rotation != null) {
                                // Ход операции живёт на том же экране: уводить человека
                                // в другое место в момент, когда он на минуту офлайн,
                                // — ровно тогда, когда он хочет видеть, что происходит.
                                RotationProgress(model) { screen = Screen.Network }
                            } else {
                                RotateAddressScreen(model) {}
                            }
                            Screen.EditProfile -> EditProfileScreen(model) { goBack() }
                            Screen.VaultLevel -> VaultLevelScreen(model, dataDir) { goBack() }
                            Screen.Saved -> SavedScreen(model)
                            Screen.Storage -> StorageScreen(model) { screen = Screen.Backup }
                            Screen.Backup -> BackupScreen(model) { screen = Screen.Storage }
                            Screen.Folders -> FoldersScreen(model) { id ->
                                editFolder = id
                                screen = Screen.FolderEdit
                            }
                            Screen.FolderEdit -> FolderEditorScreen(model, editFolder) { screen = Screen.Folders }
                            Screen.Outbox -> OutboxScreen(model) { peer ->
                                model.openChat(peer)
                                screen = Screen.Chat
                            }
                            Screen.Search -> SearchScreen(
                                model,
                                onOpenDm = {
                                    model.openChat(it)
                                    screen = Screen.Chat
                                },
                                onOpenGroup = {
                                    model.openGroup(it)
                                    screen = Screen.GroupChat
                                },
                            )
                        }
                    }
                }
            }
            // FAB «новый чат» — только на Главной, под оверлеями звонка.
            if (screen == Screen.Contacts && model.dmCallPeer == null && model.incomingCall == null) {
                FloatingActionButton(
                    onClick = { screen = Screen.NewChat },
                    modifier = Modifier.align(Alignment.BottomEnd).padding(20.dp),
                    containerColor = HoloAccent,
                    contentColor = HoloAccentDark,
                ) { Icon(TablerIcons.Pencil, stringResource(R.string.cd_new_chat), modifier = Modifier.size(22.dp)) }
            }
            model.incomingCall?.let { IncomingCallOverlay(model, it) }
            // Активный DM-звонок (аудио или видео) — полноэкранная комната-сетка.
            if (model.incomingCall == null) {
                model.dmCallPeer?.let { RoomCallScreen(model, it) }
            }
        }
    }
}
