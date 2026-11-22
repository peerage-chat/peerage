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
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalConfiguration
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        LogExport.install(applicationContext)
        model.appContext = applicationContext
        AppLock.load(applicationContext)
        // Разрешения НЕ просим здесь: во время онбординга системный диалог не нужен.
        // Запрашиваются в App() после гейта (onboarded) — см. requestCorePermissions.
        val dataDir = filesDir.absolutePath
        handleCallIntent(intent)
        model.acceptIntent(this, intent)
        setContent {
            // ADR-005 (поправка 2): раскладку решает ширина окна, и решается она ЗДЕСЬ, в одной
            // точке. Порог 720 dp — свой, не классы 600/840: телефон в ландшафте уже шире него.
            // Окно, а не экран: в разделённом режиме половина планшета — это телефон.
            val width = if (LocalConfiguration.current.screenWidthDp >= Tokens.Size.twoPaneMin.value) {
                WidthClass.TwoPane
            } else {
                WidthClass.OnePane
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
        model.acceptIntent(this, intent)
    }

    /**
     * «Принять» из уведомления входящего. Без микрофона не принимаем: звонок вышел бы
     * немым, а Android 14 вдобавок не пускает сервис звонка на передний план. Тогда
     * остаётся экран входящего — его «Принять» само спросит разрешение.
     */
    private fun handleCallIntent(i: android.content.Intent?) {
        if (i?.action == MessageNotifier.ACTION_CALL_BACK) return callBack(i)
        if (i?.action != IncomingCallReceiver.ACTION_ACCEPT) return
        i.action = null // пересоздание активити не должно принять звонок второй раз
        // Активити экспортирована: ACTION_ACCEPT может прислать кто угодно. Верим
        // только интенту с токеном из нашего уведомления.
        if (!IncomingCallReceiver.consumeToken(i.getStringExtra(IncomingCallReceiver.EXTRA_TOKEN))) return
        IncomingCallReceiver.cancel(this)
        // Камеру — только если звали с видео, нажато «Ответить» и разрешение уже есть:
        // спрашивать его из шторки некому, звонок тогда начнётся голосом.
        val video = i.getBooleanExtra(IncomingCallReceiver.EXTRA_VIDEO, false) && hasPermission(Manifest.permission.CAMERA)
        if (hasPermission(Manifest.permission.RECORD_AUDIO)) model.acceptDmCall(video = video)
    }

    /**
     * «Перезвонить» (пропущенный) или «Присоединиться» (комната группы ожила) из уведомления: чат
     * открывается всегда, звонок — только с токеном из нашего уведомления и с микрофоном (иначе
     * остаётся чат с кнопкой звонка).
     */
    private fun callBack(i: android.content.Intent) {
        val tag = i.getStringExtra(EXTRA_SHORTCUT_CHAT).orEmpty()
        val ok = MessageNotifier.consumeCallToken(tag, i.getStringExtra(MessageNotifier.EXTRA_CALL_TOKEN))
        i.action = ACTION_OPEN_CHAT // дальше — как тап по уведомлению: открыть чат
        val chat = chatOfShortcut(tag) ?: return
        MessageNotifier.cancel(this, chat)
        if (!ok || !hasPermission(Manifest.permission.RECORD_AUDIO)) return
        if (chat.group) model.joinRoom(chat.id) else model.callDm(chat.id)
    }

    /** Запрос уже на экране: onStart и кнопка замка не должны поднять второй. */
    private var asking = false

    /** Запрос системного подтверждения личности (биометрия/PIN устройства). */
    private fun askCredential() {
        if (asking) return
        asking = true
        val shown = AppLock.prompt(this) { ok ->
            asking = false
            if (ok) AppLock.unlock()
        }
        if (shown) return
        val intent = AppLock.credentialIntent(this)
        if (intent == null) {
            // Экран блокировки на устройстве сняли, пока приложение было в фоне:
            // подтверждать нечем, держать пользователя взаперти бессмысленно.
            asking = false
            AppLock.unlock()
            return
        }
        credentialLauncher.launch(intent)
    }

    private val credentialLauncher = registerForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.StartActivityForResult(),
    ) { res ->
        asking = false
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
        VaultUnlockScreen(model, dataDir, onKey = { key ->
            scope.launch(Dispatchers.IO) {
                // Ключ развёрнут отпечатком; не подошёл — остаётся экран пароля.
                if (model.start(dataDir, key = key)) vaultOpen = true
                key.fill(0)
            }
        }) { password, done ->
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
    // Личность восстановлена по 24 словам — предложить перенести переписку (шаг 7k).
    var transfer by remember { mutableStateOf(prefs.getBoolean("offer_transfer", false)) }

    // Камеру и микрофон здесь НЕ просим: они запрашиваются контекстно, в момент
    // использования (звонок / включение видео / сканер QR) — см. rememberPermissionRunner.
    // Здесь остаются только уведомления: они нужны приложению целиком, а не отдельному
    // экрану, и просить их можно лишь после гейта (иначе диалог накроет онбординг).
    // Онбординг просит их в своей карточке; здесь — для тех, кто там пропустил.
    val permLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    LaunchedEffect(onboarded, transfer) {
        if (onboarded && !transfer && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
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
        transfer -> TransferFlow(model) {
            prefs.edit().remove("offer_transfer").apply()
            transfer = false
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
    /** «Медиа и файлы» чата (3.2). */
    Media,

    /** Профиль группы — «витрина» (3.1). */
    GroupInfo,

    /** Правка группы: название и фото, только админ (3.5). */
    GroupEdit,

    /** Сверка ключа контакта (из его профиля). */
    Verify,
    Contacts,
    Chat,
    Add,
    Me,
    Info,
    GroupChat,

    /** Новая группа, шаг 1 — участники (3.4). */
    CreateGroup,

    /** Новая группа, шаг 2 — название. */
    CreateGroupName,
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

    /** «Отправить в Peerage»: выбор чатов для присланного другой программой (5.6). */
    Share,
    NewChat,
    Storage,
    Autodel,
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
    SettingsBlocked,
    SettingsMedia,
    VideoQuality,
    SettingsNotifications,
    SettingsAbout,
}

/** Раскладка по ширине окна (ADR-005, поправка 2): уже 720 dp — один экран, как на телефоне;
 *  шире — главная слева и стек экранов справа. Экраны о ширине не знают — меняется оболочка. */
internal enum class WidthClass { OnePane, TwoPane }

@Composable
private fun Root(model: AppModel, width: WidthClass) = Shell(model, twoPane = width == WidthClass.TwoPane)

@OptIn(ExperimentalMaterial3Api::class, androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun Shell(model: AppModel, twoPane: Boolean) {
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
    // Поиск на «Контактах»: null — поле закрыто.
    var contactQuery by remember { mutableStateOf<String?>(null) }
    var inviteGid by remember { mutableStateOf("") }
    // Черновик мастера новой группы и выбор на «Пригласить»: живут между шагами и шапкой.
    var groupDraft by remember { mutableStateOf(GroupDraft()) }
    val invitePick = remember { mutableStateListOf<String>() }
    var keyWordsFirst by remember { mutableStateOf(true) }
    val openKeyFlow: (Boolean) -> Unit = {
        keyWordsFirst = it
        screen = Screen.KeyFlow
    }
    // Ключ контакта, которого пришли СВЕРЯТЬ сканером (null — обычное добавление).
    var verifyingPeer by remember { mutableStateOf<String?>(null) }

    /** Итог сканирования при сверке ("match" | "mismatch" | "invalid") — шторка на экране сверки. */
    var verifyResult by remember { mutableStateOf<String?>(null) }
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
    // Открыть чат по цели: группа, «Сохранённое» или личный.
    val openTarget: (ChatTarget) -> Unit = { t ->
        when {
            t.group -> {
                model.openGroup(t.id)
                screen = Screen.GroupChat
            }
            model.isSaved(t.id) -> {
                model.openSaved()
                screen = Screen.Saved
            }
            else -> {
                model.openChat(t.id)
                screen = Screen.Chat
            }
        }
    }
    // «Поделиться» (5.6): без выбранного чата — экран выбора; с чатом (ярлык в системном меню
    // или один отмеченный) — сам чат, вложение ждёт в поле. Ключ — что и куда, без текста:
    // перенос текста в поле не должен открывать чат ещё раз.
    LaunchedEffect(model.shareDraft?.let { it.items to it.target }) {
        val d = model.shareDraft ?: return@LaunchedEffect
        val t = d.target?.takeIf { model.hasChat(it) }
        if (t == null) {
            if (d.target != null) model.shareDraft = d.copy(target = null)
            screen = Screen.Share
        } else {
            if (screen == Screen.Share) nav.back()
            openTarget(t)
        }
    }
    // Ушли с «Отправить в Peerage», не выбрав чат («назад», жест) — присланное отменено, как
    // крестиком. Не onDispose экрана: оболочку пересоздаёт старт движка, и холодный запуск
    // из «Поделиться» терял бы присланное, не успев его показать.
    val lastScreen = remember { arrayOf(screen) }
    LaunchedEffect(screen) {
        if (lastScreen[0] == Screen.Share && screen != Screen.Share && model.shareDraft?.target == null) model.shareDraft = null
        lastScreen[0] = screen
    }
    // Ярлыки на значке и ярлыки чатов. Замок их не обходит: App() не рисуется, пока он закрыт.
    LaunchedEffect(model.launchTarget) {
        val t = model.launchTarget ?: return@LaunchedEffect
        model.launchTarget = null
        when (t) {
            LaunchTarget.AddContact -> screen = Screen.Add
            LaunchTarget.MyCode -> screen = Screen.MyCode
            is LaunchTarget.Chat -> if (model.hasChat(t.chat)) openTarget(t.chat)
        }
    }
    // Папки: единственный источник правды — страница пейджера. Таб и заголовок только
    // читают её (targetPage — куда пейджер движется, поэтому таб переключается сразу,
    // а не после остановки), тап по табу только листает. Раньше таб и страница
    // синхронизировались двумя эффектами навстречу друг другу, и при быстрых
    // действиях опоздавший эффект утаскивал страницу назад посреди жеста.
    val folders = folderKeys(model)
    val folderPager = rememberPagerState { folders.size }
    // Вкладка «Запросы» появляется и пропадает, а номера страниц за ней сдвигаются. Пейджер
    // помнит номер, человек — папку: при смене набора страница остаётся на той же папке,
    // а пропавшая уступает место «Все» (иначе после принятия запроса открывались «Группы»).
    // SideEffect — до измерения кадра, так что currentPage здесь ещё прежний.
    val seenFolders = remember { arrayOf(folders) }
    SideEffect {
        if (seenFolders[0] != folders) {
            val was = seenFolders[0].getOrNull(folderPager.currentPage)
            seenFolders[0] = folders
            folderPager.requestScrollToPage(folders.indexOf(was).coerceAtLeast(0))
        }
    }
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
    // Поиск «Контактов»: «назад» сначала закрывает поле; ушли с экрана — поле закрыто.
    BackHandler(enabled = contactQuery != null && screen == Screen.ContactList) { contactQuery = null }
    LaunchedEffect(screen) { if (screen != Screen.ContactList) contactQuery = null }

    val navigate: (Screen) -> Unit = { dest -> screen = dest }

    // Переход с главной. В две панели главная не уходит с экрана, а выбранное в ней
    // открывается справа заново ([NavState.resetTo]); на телефоне — обычный шаг вглубь.
    val home: (Screen) -> Unit = { dest -> if (twoPane) nav.resetTo(dest) else screen = dest }

    // «Медиа и файлы» открываются из «⋮» чата, профиля и пустого поиска — сразу на нужной вкладке.
    val openMedia: (ChatTarget, MediaTab) -> Unit = { chat, tab ->
        model.openMedia(chat, tab)
        screen = Screen.Media
    }

    // Выдвижной панели больше нет: разделы и профиль живут на своём экране «Я»
    // (Screen.Me), который открывается аватаром в шапке. Панель делала две работы
    // сразу — показывала профиль и раздавала разделы, — и профилю в её шапке было
    // тесно. Свайпом «Я» не открывается намеренно: горизонталь на главной отдана
    // переключению папок, третий горизонтальный жест дрался бы с ними.
    run {
        Box(Modifier.fillMaxSize()) {
            // Системная полоса навигации — продолжение листа над ней: иначе внизу экрана
            // остаётся чужая тёмная полоса. Под меню у пузыря обе полосы темнеют вместе с
            // экраном — иначе светлые края выдают, что затемнение «не до конца».
            val dimmed = model.bubbleActions != null
            fun bar(c: androidx.compose.ui.graphics.Color) =
                (if (dimmed) Tokens.Colors.dim.compositeOver(c) else c).toArgb()
            val navColor = bar(Tokens.Colors.panel)
            val statusColor = bar(Tokens.Colors.bg)
            SideEffect {
                activity?.window?.navigationBarColor = navColor
                activity?.window?.statusBarColor = statusColor
            }
            // Экран целиком — шапка, лист с содержимым и FAB главной. Одна функция на обе
            // раскладки: в две панели главная рисуется слева ею же, остальное — справа.
            val pane: @Composable (Screen) -> Unit = { s ->
                Box(Modifier.fillMaxSize()) {
                    Column(Modifier.fillMaxSize().background(HoloBg)) {
                        // ── Шапка: конфигурируется по экрану ──────────────────────────────
                        when (s) {
                            Screen.Contacts -> HomeTopBar(
                                model,
                                onProfile = { home(Screen.Me) },
                                onNetwork = { home(Screen.Network) },
                                onSearch = {
                                    model.globalQuery = ""
                                    home(Screen.Search)
                                },
                            )
                            Screen.Chat -> ChatTopBar(
                                model,
                                onBack = goBack,
                                onProfile = { screen = Screen.Info },
                                onMedia = { model.currentChat?.let { openMedia(ChatTarget(it, group = false), MediaTab.Media) } },
                            )
                            Screen.Media -> MediaFilesTopBar(model, onBack = goBack)
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
                            // «Витрина» рисует свою шапку на фоне и едет вместе со списком.
                            Screen.Info, Screen.GroupInfo -> {}
                            // Правка рисует свою шапку: «Готово» в ней — часть формы.
                            Screen.GroupEdit -> {}
                            Screen.Verify -> TopBar(stringResource(R.string.verify_title), onBack = goBack)
                            Screen.GroupChat -> GroupChatTopBar(
                                model,
                                onBack = goBack,
                                onInfo = { screen = Screen.GroupInfo },
                                onInvite = {
                                    invitePick.clear()
                                    screen = Screen.AddMember
                                },
                                onMedia = { model.currentGroup?.let { openMedia(ChatTarget(it, group = true), MediaTab.Media) } },
                            )
                            Screen.CreateGroup -> TopBar(stringResource(R.string.title_new_group), onBack = goBack) {
                                PickedCount(groupDraft.picked.size)
                            }
                            Screen.CreateGroupName -> TopBar(stringResource(R.string.title_new_group), onBack = goBack)
                            Screen.NewChat -> TopBar(stringResource(R.string.newchat_title), onBack = goBack)
                            Screen.AddMember -> TopBar(stringResource(R.string.title_add_member), onBack = goBack) {
                                PickedCount(invitePick.size)
                            }
                            Screen.SettingsSecurity -> TopBar(stringResource(R.string.settings_cat_security), onBack = goBack)
                            Screen.SettingsPrivacy -> TopBar(stringResource(R.string.settings_cat_privacy), onBack = goBack)
                            Screen.SettingsBlocked -> TopBar(stringResource(R.string.blocked_title), onBack = goBack)
                            Screen.SettingsMedia -> TopBar(stringResource(R.string.settings_cat_media), onBack = goBack)
                            Screen.VideoQuality -> TopBar(stringResource(R.string.media_video_quality), onBack = goBack)
                            Screen.SettingsNotifications -> TopBar(stringResource(R.string.settings_cat_notifications), onBack = goBack)
                            Screen.SettingsAbout -> {}
                            Screen.Scanner -> TopBar(stringResource(R.string.title_verify_qr), onBack = goBack)
                            Screen.ContactList -> TopBar(stringResource(R.string.menu_contacts), onBack = goBack) {
                                BarIcon(TablerIcons.Search, stringResource(R.string.cd_search)) {
                                    contactQuery = if (contactQuery == null) "" else null
                                }
                                BarIcon(
                                    TablerIcons.Plus,
                                    stringResource(R.string.cd_add_contact),
                                    tint = Tokens.Colors.accentDark,
                                    bg = Tokens.Colors.accent,
                                ) { screen = Screen.Add }
                            }
                            Screen.Calls -> TopBar(stringResource(R.string.menu_calls), onBack = goBack)
                            Screen.Network -> NetworkHeader(model, onBack = goBack)
                            Screen.EditProfile -> {}
                            Screen.VaultLevel -> {}
                            Screen.Flood -> TopBar(stringResource(R.string.flood_screen_title), onBack = goBack)
                            Screen.RotateAddress -> TopBar(stringResource(R.string.rotate_title), onBack = goBack)
                            Screen.Saved -> SavedTopBar(model, onBack = goBack)
                            Screen.Storage -> StorageHeader(model, onBack = goBack)
                            Screen.Autodel -> TopBar(stringResource(R.string.autodel_title), onBack = goBack)
                            Screen.Backup -> TopBar(stringResource(R.string.backup_title), onBack = goBack)
                            Screen.Outbox -> TopBar(stringResource(R.string.title_outbox), onBack = goBack)
                            Screen.Folders -> TopBar(stringResource(R.string.folders_title), onBack = goBack)
                            Screen.FolderEdit -> FolderEditTopBar(model, onBack = goBack)
                            Screen.Search -> SearchTopBar(model, onBack = goBack)
                            Screen.Share -> ShareTopBar {
                                model.shareDraft = null
                                goBack()
                            }
                        }
                        // ── Контент экрана (без шапки) ────────────────────────────────────
                        // «Лист» (app-ui.md): под шапкой — панель со скруглённым верхом во всю
                        // высоту, содержимое экрана на ней; у чата на листе — закреплённое,
                        // лента и поле ввода.
                        val showcase = s == Screen.Info || s == Screen.GroupInfo
                        Box(
                            if (showcase) {
                                Modifier.weight(1f)
                            } else {
                                Modifier.weight(1f)
                                    .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                                    .background(Tokens.Colors.panel)
                            },
                        ) {
                            when (s) {
                                Screen.Contacts -> Column {
                                    // Папки — на панели, над списком: табы едут вместе с ним.
                                    FolderTabs(
                                        model,
                                        activeFilter,
                                        onFilter = selectFolder,
                                        onManage = { home(Screen.Folders) },
                                    )
                                    ChatsPager(
                                        model,
                                        folderPager,
                                        HomeNav(
                                            openDm = {
                                                model.openChat(it)
                                                home(Screen.Chat)
                                            },
                                            openGroup = {
                                                model.openGroup(it)
                                                home(Screen.GroupChat)
                                            },
                                            openRequest = {
                                                requestKey = it
                                                home(Screen.Request)
                                            },
                                            openInvite = {
                                                inviteGid = it
                                                home(Screen.Invite)
                                            },
                                            openOutbox = { home(Screen.Outbox) },
                                            myCode = { home(Screen.MyCode) },
                                            scan = { home(Screen.Scanner) },
                                            newGroup = {
                                                groupDraft = GroupDraft()
                                                home(Screen.CreateGroup)
                                            },
                                            keyFlow = openKeyFlow,
                                            dataBackup = { home(Screen.Backup) },
                                        ),
                                    )
                                }
                                Screen.Chat -> ChatScreen(model) { tab ->
                                    model.currentChat?.let { openMedia(ChatTarget(it, group = false), tab) }
                                }
                                Screen.Media -> MediaFilesScreen(model) { ref ->
                                    val chat = model.mediaBrowse?.chat ?: return@MediaFilesScreen
                                    if (chat.group) {
                                        model.openGroup(chat.id)
                                        screen = Screen.GroupChat
                                    } else {
                                        model.openChat(chat.id)
                                        screen = Screen.Chat
                                    }
                                    model.jumpTo = ref
                                }
                                Screen.Add -> AddContactScreen(
                                    model,
                                    onMyCode = { screen = Screen.MyCode },
                                    // Запрос ушёл или контакт уже есть — в чат вместо экрана
                                    // добавления: «назад» из чата ведёт в список, а не к камере.
                                    onOpenChat = { key ->
                                        model.openChat(key)
                                        nav.replace(Screen.Chat)
                                    },
                                )
                                // Отдельный сканер — только для сверки ключа: добавление
                                // сканирует камерой прямо на своём экране.
                                Screen.Scanner -> QrScannerScreen { key ->
                                    val target = verifyingPeer ?: return@QrScannerScreen
                                    // Личность из кода сравнивает движок; итог — шторкой на
                                    // экране сверки. Не совпало — ровно тот случай, ради
                                    // которого сверка и нужна.
                                    verifyingPeer = null
                                    model.verifyByCode(target, key) { verifyResult = it }
                                    nav.back() // сканер открыт с экрана сверки — туда и вернуться
                                }
                                Screen.Me -> ProfileScreen(model, navigate)
                                Screen.MyCode -> MyCodeScreen(model) { nav.replace(Screen.Add) }
                                Screen.Request -> {
                                    // Сверка ключа незнакомца — тем же сканером и шторкой, что у контакта.
                                    val scan = {
                                        verifyingPeer = requestKey
                                        screen = Screen.Scanner
                                    }
                                    RequestScreen(model, requestKey, onDone = goBack, onVerify = scan)
                                    verifyResult?.let { r ->
                                        VerifyResultSheet(
                                            r,
                                            onMyCode = { screen = Screen.MyCode },
                                            onScanAgain = scan,
                                            onClose = { verifyResult = null },
                                        )
                                    }
                                }
                                Screen.Invite -> InviteScreen(model, inviteGid, onDone = goBack)
                                Screen.KeyFlow -> KeyFlowScreen(model, keyWordsFirst, onDone = goBack)
                                Screen.Info -> ContactProfileScreen(
                                    model,
                                    onBack = goBack,
                                    onMessage = { screen = Screen.Chat },
                                    onVerify = { screen = Screen.Verify },
                                    onOpenGroup = {
                                        model.openGroup(it)
                                        screen = Screen.GroupChat
                                    },
                                    onMedia = { model.currentChat?.let { openMedia(ChatTarget(it, group = false), MediaTab.Media) } },
                                    onDeleted = { screen = Screen.Contacts },
                                )
                                Screen.Verify -> {
                                    val scan = {
                                        verifyingPeer = model.currentChat
                                        screen = Screen.Scanner
                                    }
                                    KeyVerifyScreen(model, onScan = scan, onMyCode = { screen = Screen.MyCode })
                                    verifyResult?.let { r ->
                                        VerifyResultSheet(
                                            r,
                                            onMyCode = { screen = Screen.MyCode },
                                            onScanAgain = scan,
                                            onClose = { verifyResult = null },
                                        )
                                    }
                                }
                                Screen.GroupEdit -> GroupEditScreen(model, onBack = goBack)
                                Screen.GroupInfo -> GroupProfileScreen(
                                    model,
                                    onBack = goBack,
                                    onEdit = { screen = Screen.GroupEdit },
                                    onAdd = {
                                        invitePick.clear()
                                        screen = Screen.AddMember
                                    },
                                    onMedia = { model.currentGroup?.let { openMedia(ChatTarget(it, group = true), MediaTab.Media) } },
                                    onSearch = {
                                        model.currentGroup?.let { model.openChatSearch(ChatTarget(it, group = true)) }
                                        screen = Screen.GroupChat
                                    },
                                    onMessage = {
                                        model.openChat(it)
                                        screen = Screen.Chat
                                    },
                                    onProfile = {
                                        model.openChat(it)
                                        screen = Screen.Info
                                    },
                                    onLeft = { screen = Screen.Contacts },
                                )
                                Screen.GroupChat -> GroupChatScreen(model, onMedia = { tab ->
                                    model.currentGroup?.let { openMedia(ChatTarget(it, group = true), tab) }
                                }) { key ->
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
                                Screen.CreateGroup -> GroupPickScreen(model, groupDraft) { screen = Screen.CreateGroupName }
                                Screen.CreateGroupName -> GroupNameScreen(model, groupDraft) { screen = Screen.Contacts }
                                Screen.NewChat -> NewChatScreen(
                                    model,
                                    onNewGroup = {
                                        groupDraft = GroupDraft()
                                        screen = Screen.CreateGroup
                                    },
                                    onAddContact = { screen = Screen.Add },
                                    // Выбор собеседника завершает «Новый чат»: назад из
                                    // чата — на главную, а не обратно в выбор.
                                    onOpenDm = {
                                        model.openChat(it)
                                        nav.replace(Screen.Chat)
                                    },
                                )
                                Screen.AddMember -> GroupInviteScreen(model, invitePick, onDone = goBack)
                                Screen.SettingsSecurity -> SettingsSecurityScreen(
                                    model,
                                    onVaultLevel = { screen = Screen.VaultLevel },
                                    onContacts = { screen = Screen.ContactList },
                                    onKeyFlow = openKeyFlow,
                                    vaultLevel = VaultKeyManager.level(java.io.File(dataDir)),
                                )
                                Screen.SettingsPrivacy -> SettingsPrivacyScreen(model, onBlocked = { screen = Screen.SettingsBlocked })
                                Screen.SettingsBlocked -> SettingsBlockedScreen(model)
                                Screen.SettingsMedia -> SettingsMediaScreen(model) { screen = Screen.VideoQuality }
                                Screen.VideoQuality -> SettingsVideoQualityScreen(model)
                                Screen.SettingsNotifications -> SettingsNotificationsScreen(model)
                                Screen.SettingsAbout -> SettingsAboutScreen(onBack = goBack)
                                Screen.ContactList -> ContactListScreen(
                                    model,
                                    query = contactQuery,
                                    onQuery = { contactQuery = it },
                                    // Запросы живут вкладкой главной — туда и ведёт полоса.
                                    onRequests = {
                                        screen = Screen.Contacts
                                        selectFolder(RequestsFolder)
                                    },
                                ) {
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
                                Screen.VaultLevel -> VaultLevelScreen(dataDir) { goBack() }
                                Screen.Saved -> ChatScreen(model) { tab ->
                                    model.currentChat?.let { openMedia(ChatTarget(it, group = false), tab) }
                                }
                                Screen.Storage -> StorageScreen(
                                    model,
                                    onBackup = { screen = Screen.Backup },
                                    onAutodel = { screen = Screen.Autodel },
                                )
                                Screen.Autodel -> AutodelScreen(model)
                                Screen.Backup -> BackupScreen(model) { screen = Screen.Storage }
                                Screen.Folders -> FoldersScreen(model) { id ->
                                    editFolder = id
                                    screen = Screen.FolderEdit
                                }
                                Screen.FolderEdit -> FolderEditorScreen(model, editFolder, onDone = goBack)
                                Screen.Outbox -> OutboxScreen(model) { peer ->
                                    model.openChat(peer)
                                    screen = Screen.Chat
                                }
                                Screen.Search -> SearchScreen(model, onOpen = openTarget)
                                // Один чат открывает эффект «Поделиться» (цель уже в черновике).
                                Screen.Share -> ShareScreen(model, onOpenOne = {}, onSent = { nav.leave(setOf(Screen.Share)) })
                            }
                        }
                    }
                    // FAB «новый чат» — часть главной: уезжает вместе с ней, под оверлеями звонка.
                    if (s == Screen.Contacts && !inCall) {
                        FloatingActionButton(
                            onClick = { home(Screen.NewChat) },
                            modifier = Modifier.align(Alignment.BottomEnd).padding(Tokens.Space.xl),
                            containerColor = HoloAccent,
                            contentColor = HoloAccentDark,
                        ) { Icon(TablerIcons.Pencil, stringResource(R.string.cd_new_chat), modifier = Modifier.size(Tokens.Size.iconM)) }
                    }
                }
            }
            Surface(Modifier.fillMaxSize(), color = HoloBg) {
                val backOn = !inCall && nav.depth > 1
                if (!twoPane) {
                    ScreenTransitions(nav, backEnabled = backOn, onBack = goBack) { s -> pane(s) }
                } else {
                    TwoPanes(
                        list = { CompositionLocalProvider(LocalPaneChat provides model.viewing) { pane(Screen.Contacts) } },
                        detail = {
                            ScreenTransitions(nav, backEnabled = backOn, onBack = goBack, rootFade = true) { s ->
                                when (s) {
                                    Screen.Contacts -> NoChatPane()
                                    // Другой чат из списка — тот же экран с другим чатом: без
                                    // ключа он сменился бы мгновенно, а лист должен проявиться.
                                    Screen.Chat, Screen.GroupChat, Screen.Saved -> FadeIn(model.openChatTarget()) { pane(s) }
                                    else -> pane(s)
                                }
                            }
                        },
                    )
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
            // Меню у пузыря — над всем экраном: затемнение и меню рядом с пузырём.
            BubbleMenuHost(model)
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
