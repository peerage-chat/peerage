package chat.peerage

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
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.windowsizeclass.ExperimentalMaterial3WindowSizeClassApi
import androidx.compose.material3.windowsizeclass.WindowWidthSizeClass
import androidx.compose.material3.windowsizeclass.calculateWindowSizeClass
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import chat.peerage.ui.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    // Синглтон на процесс: переживает пересоздание Activity (фон/возврат), иначе
    // утекают второй нативный узел и дублирующие poll/ticker-циклы.
    private val model = AppModel.instance

    @OptIn(ExperimentalMaterial3WindowSizeClassApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        LogExport.install(applicationContext)
        model.appContext = applicationContext
        AppLock.load(applicationContext)
        // Разрешения НЕ просим здесь: во время онбординга системный диалог не нужен.
        // Запрашиваются в App() после гейта (onboarded) — см. requestCorePermissions.
        val dataDir = filesDir.absolutePath
        handleCallIntent(intent)
        setContent {
            // ADR-005: класс ширины окна читается ЗДЕСЬ (единственная точка), дальше по
            // приложению ходит наш enum WidthClass — experimental-API не течёт в UI-код.
            val width = when (calculateWindowSizeClass(this@MainActivity).widthSizeClass) {
                WindowWidthSizeClass.Expanded -> WidthClass.Expanded
                WindowWidthSizeClass.Medium -> WidthClass.Medium
                else -> WidthClass.Compact
            }
            AppTheme {
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

    override fun onNewIntent(intent: android.content.Intent) {
        super.onNewIntent(intent)
        handleCallIntent(intent)
    }

    /**
     * «Принять» из уведомления входящего. Без микрофона не принимаем: звонок вышел бы
     * немым, а Android 14 вдобавок не пускает сервис звонка на передний план. Тогда
     * остаётся экран входящего — его «Принять» само спросит разрешение.
     */
    private fun handleCallIntent(i: android.content.Intent?) {
        if (i?.action != IncomingCallReceiver.ACTION_ACCEPT) return
        i.action = null // пересоздание активити не должно принять звонок второй раз
        // Активити экспортирована: ACTION_ACCEPT может прислать кто угодно. Верим
        // только интенту с токеном из нашего уведомления.
        if (!IncomingCallReceiver.consumeToken(i.getStringExtra(IncomingCallReceiver.EXTRA_TOKEN))) return
        IncomingCallReceiver.cancel(this)
        if (hasPermission(Manifest.permission.RECORD_AUDIO)) model.acceptDmCall()
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
        // Отложенное «удалить у себя» выполняем при уходе: снекбара в фоне не видно, а убитый
        // процесс иначе молча вернул бы удалённое.
        model.undo.flush()
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
    // Данные не открылись (R-029): ничего не стёрто, выбор за человеком.
    model.recovery?.let {
        RecoveryScreen(model, dataDir, it)
        return
    }
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

    // Гейт первого запуска (M4). Флаг в SharedPreferences.
    val ctx = LocalContext.current
    val prefs = ctx.getSharedPreferences("settings", Context.MODE_PRIVATE)
    var onboarded by remember { mutableStateOf(prefs.getBoolean("onboarded", false)) }

    // Камеру и микрофон здесь НЕ просим: они запрашиваются контекстно, в момент
    // использования (звонок / включение видео / сканер QR) — см. rememberPermissionRunner.
    // Здесь остаются только уведомления: они нужны приложению целиком, а не отдельному
    // экрану, и просить их можно лишь после гейта (иначе диалог накроет онбординг).
    // Онбординг просит их в своей карточке; здесь — для тех, кто там пропустил.
    val permLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    LaunchedEffect(onboarded) {
        if (onboarded && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            !ctx.hasPermission(Manifest.permission.POST_NOTIFICATIONS)
        ) {
            permLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    when {
        model.error != null -> CenterMessage(stringResource(R.string.app_error, model.error.orEmpty()), Modifier.background(HoloBg))
        model.me == null -> CenterMessage(stringResource(R.string.app_connecting), Modifier.background(HoloBg))
        !onboarded -> OnboardingFlow(model) {
            prefs.edit().putBoolean("onboarded", true).apply()
            onboarded = true
        }
        else -> Root(model, width)
    }
}

@Composable
internal fun CenterMessage(text: String, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text(text, color = HoloSecondary, fontSize = Tokens.Font.body)
    }
}

internal enum class Screen {
    Contacts,
    Chat,
    Add,
    Me,
    Info,
    GroupChat,
    CreateGroup,
    AddMember,
    MyCode,
    Request,
    Invite,
    KeyFlow,
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
        NavState(if (fromNotif) Screen.RotateAddress else debugStartScreen(activity) ?: Screen.Contacts)
    }
    var screen by nav
    // id редактируемой папки (null — создаём новую).
    var editFolder by remember { mutableStateOf<String?>(null) }
    // Открытая карточка запроса (ключ) и приглашения (gid).
    var requestKey by remember { mutableStateOf("") }
    var inviteGid by remember { mutableStateOf("") }
    var keyWordsFirst by remember { mutableStateOf(true) }
    val openKeyFlow: (Boolean) -> Unit = {
        keyWordsFirst = it
        screen = Screen.KeyFlow
    }
    // Ключ контакта, которого пришли СВЕРЯТЬ сканером (null — обычное добавление).
    var verifyingPeer by remember { mutableStateOf<String?>(null) }
    val ctx = LocalContext.current
    // ADR-004: нас отозвали из открытой группы (currentGroup → null) — уходим на список.
    LaunchedEffect(model.currentGroup) {
        if (model.currentGroup == null) nav.leave(setOf(Screen.GroupChat, Screen.AddMember))
    }
    // Вошли в комнату группы по приглашению — показываем её чат (там звонок).
    LaunchedEffect(model.openGroupRequest) {
        val gid = model.openGroupRequest ?: return@LaunchedEffect
        model.openGroupRequest = null
        model.openGroup(gid)
        screen = Screen.GroupChat
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
    val scope = rememberCoroutineScope()
    val selectFolder: (String) -> Unit = { key ->
        val i = folders.indexOf(key)
        if (i >= 0) scope.launch { folderPager.animateScrollToPage(i) }
    }

    // «Назад» — одна функция и для системной кнопки, и для стрелки в шапке.
    val goBack: () -> Unit = {
        if (screen == Screen.Scanner) verifyingPeer = null
        nav.back()
    }
    // Системная «назад» — навигация по приложению, а не выход. Во время звонка/
    // входящего гасим (не выходим); слои внутри экранов (выделение, панели, настройки
    // звонка) перехватывают её своими обработчиками раньше этого. На главной без
    // оверлеев обработчик выключен — back выходит штатно.
    // Обычный «назад» по экранам — предиктивный, в ScreenTransitions; здесь только звонок.
    val inCall = model.dmCallPeer != null || model.incomingCall != null
    BackHandler(enabled = inCall) {}

    val navigate: (Screen) -> Unit = { dest -> screen = dest }

    // Выдвижной панели больше нет: разделы и профиль живут на своём экране «Я»
    // (Screen.Me), который открывается аватаром в шапке. Панель делала две работы
    // сразу — показывала профиль и раздавала разделы, — и профилю в её шапке было
    // тесно. Свайпом «Я» не открывается намеренно: горизонталь на главной отдана
    // переключению папок, третий горизонтальный жест дрался бы с ними.
    run {
        Box(Modifier.fillMaxSize()) {
            // Системная полоса навигации — продолжение того, что над ней: панели листа или
            // фона чата. По экрану-цели: во время перехода смонтированы оба.
            val barColor = (if (screen != Screen.Chat && screen != Screen.GroupChat) Tokens.Colors.panel else Tokens.Colors.bg).toArgb()
            SideEffect { activity?.window?.navigationBarColor = barColor }
            Surface(Modifier.fillMaxSize(), color = HoloBg) {
                ScreenTransitions(nav, backEnabled = !inCall && nav.depth > 1, onBack = goBack) { s ->
                    Box(Modifier.fillMaxSize()) {
                        Column(Modifier.fillMaxSize().background(HoloBg)) {
                            // ── Шапка: конфигурируется по экрану ──────────────────────────────
                            when (s) {
                                Screen.Contacts -> HomeTopBar(
                                    model,
                                    onProfile = { screen = Screen.Me },
                                    onNetwork = { screen = Screen.Network },
                                    onSearch = { screen = Screen.Search },
                                )
                                Screen.Chat -> ChatTopBar(
                                    model,
                                    onBack = goBack,
                                    onProfile = { screen = Screen.Info },
                                )
                                Screen.Add -> TopBar(stringResource(R.string.title_add_contact), onBack = goBack)
                                Screen.Me -> TopBar(stringResource(R.string.me_title), onBack = goBack) {
                                    BarIcon(TablerIcons.Pencil, stringResource(R.string.cd_edit_profile)) { screen = Screen.EditProfile }
                                }
                                Screen.MyCode -> TopBar(stringResource(R.string.my_code_title), onBack = goBack)
                                Screen.Request -> TopBar(stringResource(R.string.req_title), onBack = goBack)
                                Screen.Invite -> TopBar(stringResource(R.string.invite_title), onBack = goBack)
                                Screen.KeyFlow -> TopBar(
                                    stringResource(if (keyWordsFirst) R.string.key_flow_title else R.string.backup_check),
                                    onBack = goBack,
                                )
                                Screen.Info -> TopBar(stringResource(R.string.common_contact), onBack = goBack)
                                Screen.GroupChat -> GroupChatTopBar(
                                    model,
                                    onBack = goBack,
                                    onInfo = { screen = Screen.AddMember },
                                )
                                Screen.CreateGroup -> TopBar(stringResource(R.string.title_new_group), onBack = goBack)
                                Screen.NewChat -> TopBar(stringResource(R.string.newchat_title), onBack = goBack)
                                Screen.AddMember -> TopBar(stringResource(R.string.title_add_member), onBack = goBack)
                                Screen.SettingsSecurity -> TopBar(stringResource(R.string.settings_cat_security), onBack = goBack)
                                Screen.SettingsPrivacy -> TopBar(stringResource(R.string.settings_cat_privacy), onBack = goBack)
                                Screen.SettingsMedia -> TopBar(stringResource(R.string.settings_cat_media), onBack = goBack)
                                Screen.SettingsNotifications -> TopBar(stringResource(R.string.settings_cat_notifications), onBack = goBack)
                                Screen.SettingsAbout -> TopBar(stringResource(R.string.settings_cat_about), onBack = goBack)
                                Screen.Scanner -> TopBar(
                                    stringResource(if (verifyingPeer != null) R.string.title_verify_qr else R.string.title_scan_qr),
                                    onBack = goBack,
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
                                Screen.Search -> TopBar(stringResource(R.string.title_search), onBack = goBack)
                            }
                            // ── Контент экрана (без шапки) ────────────────────────────────────
                            // «Лист» (app-ui.md): под шапкой — панель со скруглённым верхом во всю
                            // высоту, содержимое экрана на ней. Чаты переходят на лист вместе со
                            // своей лентой и полем ввода (фаза 2 плана), до того — на фоне.
                            val sheet = s != Screen.Chat && s != Screen.GroupChat
                            Box(
                                Modifier.weight(1f).then(
                                    if (sheet) {
                                        Modifier.clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                                            .background(Tokens.Colors.panel)
                                    } else {
                                        Modifier
                                    },
                                ),
                            ) {
                                when (s) {
                                    Screen.Contacts -> Column {
                                        // Папки — на панели, над списком: табы едут вместе с ним.
                                        FolderTabs(
                                            model,
                                            activeFilter,
                                            onFilter = selectFolder,
                                            onManage = { screen = Screen.Folders },
                                        )
                                        ChatsPager(
                                            model,
                                            folderPager,
                                            HomeNav(
                                                openDm = {
                                                    model.openChat(it)
                                                    screen = Screen.Chat
                                                },
                                                openGroup = {
                                                    model.openGroup(it)
                                                    screen = Screen.GroupChat
                                                },
                                                openRequest = {
                                                    requestKey = it
                                                    screen = Screen.Request
                                                },
                                                openInvite = {
                                                    inviteGid = it
                                                    screen = Screen.Invite
                                                },
                                                openOutbox = { screen = Screen.Outbox },
                                                myCode = { screen = Screen.MyCode },
                                                scan = { screen = Screen.Scanner },
                                                newGroup = { screen = Screen.CreateGroup },
                                                keyFlow = openKeyFlow,
                                                dataBackup = { screen = Screen.Backup },
                                            ),
                                        )
                                    }
                                    Screen.Chat -> ChatScreen(model)
                                    Screen.Add -> AddScreen(
                                        model,
                                        scannedKey = scanResult,
                                        onScan = { screen = Screen.Scanner },
                                        onDone = goBack,
                                    )
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
                                            // Сверяем только личность: код обмена несёт ещё адрес
                                            // («:…») и самоназвание («#…»), а они меняются.
                                            val identity = key.trim().substringBefore('#').substringBefore(':')
                                            val ok = identity.equals(target, ignoreCase = true)
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
                                    Screen.Me -> ProfileScreen(model, navigate)
                                    Screen.MyCode -> MyCodeScreen(model) { screen = Screen.Scanner }
                                    Screen.Request -> RequestScreen(model, requestKey, onDone = goBack)
                                    Screen.Invite -> InviteScreen(model, inviteGid, onDone = goBack)
                                    Screen.KeyFlow -> KeyFlowScreen(model, keyWordsFirst, onDone = goBack)
                                    Screen.Info -> ContactProfileScreen(
                                        model,
                                        onMessage = { screen = Screen.Chat },
                                        onScanQr = {
                                            verifyingPeer = model.currentChat
                                            screen = Screen.Scanner
                                        },
                                        onDeleted = { screen = Screen.Contacts },
                                    )
                                    Screen.GroupChat -> GroupChatScreen(model) { key ->
                                        // «@я» — свой профиль; контакт — его карточка; с
                                        // незнакомцем из группы открывать нечего.
                                        when {
                                            key == model.me?.pubkey -> screen = Screen.Me
                                            model.contacts.any { it.key == key } -> {
                                                model.openChat(key)
                                                screen = Screen.Info
                                            }
                                        }
                                    }
                                    Screen.CreateGroup -> CreateGroupScreen(model) { screen = Screen.Contacts }
                                    Screen.NewChat -> NewChatScreen(
                                        model,
                                        onNewGroup = { screen = Screen.CreateGroup },
                                        onAddContact = { screen = Screen.Add },
                                        // Выбор собеседника завершает «Новый чат»: назад из
                                        // чата — на главную, а не обратно в выбор.
                                        onOpenDm = {
                                            model.openChat(it)
                                            nav.replace(Screen.Chat)
                                        },
                                    )
                                    Screen.AddMember -> AddMemberScreen(model) { screen = Screen.GroupChat }
                                    Screen.SettingsSecurity -> SettingsSecurityScreen(
                                        model,
                                        onVaultLevel = { screen = Screen.VaultLevel },
                                        onContacts = { screen = Screen.ContactList },
                                        onKeyFlow = openKeyFlow,
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
                                        onMyQr = { screen = Screen.MyCode },
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
                                        RotationProgress(model) { nav.replace(Screen.Network) }
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
                        // FAB «новый чат» — часть главной: уезжает вместе с ней, под оверлеями звонка.
                        if (s == Screen.Contacts && !inCall) {
                            FloatingActionButton(
                                onClick = { screen = Screen.NewChat },
                                modifier = Modifier.align(Alignment.BottomEnd).padding(Tokens.Space.xl),
                                containerColor = HoloAccent,
                                contentColor = HoloAccentDark,
                            ) { Icon(TablerIcons.Pencil, stringResource(R.string.cd_new_chat), modifier = Modifier.size(Tokens.Size.iconM)) }
                        }
                    }
                }
            }
            // «Отменить» — над полем ввода в чате и над FAB на главной, у низа на прочих.
            val overInput = screen == Screen.Contacts || screen == Screen.Chat || screen == Screen.GroupChat || screen == Screen.Saved
            UndoSnackbar(
                model.undo,
                Modifier.align(Alignment.BottomCenter)
                    .padding(horizontal = Tokens.Space.m)
                    .padding(bottom = if (overInput) Tokens.Size.fabClearance else Tokens.Space.xxl),
            )
            if (model.showRingSheet && model.currentRoom != null) {
                RingSheet(model) { model.showRingSheet = false }
                BackHandler { model.showRingSheet = false }
            }
            // Звонок ложится поверх чата, а поле ввода под ним держит фокус — клавиатура
            // оставалась поверх звонка и закрывала кнопки входящего. Снимаем фокус.
            val focus = LocalFocusManager.current
            val callShown = model.incomingCall != null || model.dmCallPeer != null
            LaunchedEffect(callShown) { if (callShown) focus.clearFocus(force = true) }
            model.incomingCall?.let { IncomingCallOverlay(model, it) }
            // Активный DM-звонок (аудио или видео) — полноэкранная комната-сетка.
            if (model.incomingCall == null) {
                model.dmCallPeer?.let { RoomCallScreen(model, it) }
            }
        }
    }
}

/**
 * Рычаг для проверки экранов на эмуляторе: отладочная сборка открывается сразу на экране из
 * `--es debug_screen <Screen>` (`adb shell am start -S -n chat.peerage/.MainActivity …`).
 * Релизная сборка этот параметр не читает.
 */
private fun debugStartScreen(activity: android.app.Activity?): Screen? {
    val app = activity?.applicationInfo ?: return null
    if (app.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE == 0) return null
    val name = activity.intent?.getStringExtra("debug_screen") ?: return null
    return Screen.entries.firstOrNull { it.name == name }
}
