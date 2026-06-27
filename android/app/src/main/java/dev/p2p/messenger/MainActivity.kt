package dev.p2p.messenger

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Create
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.ui.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    // Синглтон на процесс: переживает пересоздание Activity (фон/возврат), иначе
    // утекают второй нативный узел и дублирующие poll/ticker-циклы.
    private val model = AppModel.instance
    private val perms =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model.appContext = applicationContext
        val wanted = mutableListOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.CAMERA)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            wanted += Manifest.permission.POST_NOTIFICATIONS // иначе уведомление сервиса не покажется
        }
        val need = wanted.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (need.isNotEmpty()) perms.launch(need.toTypedArray())
        val dataDir = filesDir.absolutePath
        setContent { P2PTheme { App(model, dataDir) } }
    }

    override fun onStart() {
        super.onStart()
        model.foreground = true
        // Поднимаем mesh-узел при выходе на передний план (в т.ч. после кнопки
        // «Отключить» в уведомлении): держим процесс/сеть живыми при погасшем экране.
        ConnService.start(applicationContext)
    }

    override fun onStop() {
        super.onStop()
        // В фоне троттлим поллинг (см. AppModel) — меньше пробуждений CPU.
        model.foreground = false
    }
}

@Composable
fun App(model: AppModel, dataDir: String) {
    // Инициализация ядра на фоне; опрос событий/тикер статуса model запускает сам
    // на выделенных потоках (см. AppModel — UI ничего нативного не вызывает напрямую).
    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) { model.start(dataDir) }
    }

    when {
        model.error != null -> CenterMessage(stringResource(R.string.app_error, model.error.orEmpty()))
        model.me == null -> CenterMessage(stringResource(R.string.app_connecting))
        else -> Root(model)
    }
}

@Composable
internal fun CenterMessage(text: String) {
    Box(Modifier.fillMaxSize().background(HoloBg), contentAlignment = Alignment.Center) {
        Text(text, color = HoloSecondary, fontSize = 14.sp)
    }
}

internal enum class Screen { Contacts, Chat, Add, Me, Info, Groups, GroupChat, CreateGroup, AddMember, Settings, Scanner, ContactList, Calls, Network, Saved, Search }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Root(model: AppModel) {
    var screen by remember { mutableStateOf(Screen.Contacts) }
    var scanResult by remember { mutableStateOf<String?>(null) }
    var activeFilter by remember { mutableStateOf("all") }
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()

    // Системная «назад» — навигация по приложению, а не выход. Во время звонка/
    // входящего гасим (не выходим), открытое меню закрываем, иначе возврат к родителю.
    // На главном (Contacts) без оверлеев — обработчик выключен, back выходит штатно.
    BackHandler(
        enabled = model.dmCallPeer != null || model.incomingCall != null ||
            drawerState.isOpen || screen != Screen.Contacts,
    ) {
        when {
            model.incomingCall != null -> {}
            model.dmCallPeer != null -> {}
            drawerState.isOpen -> scope.launch { drawerState.close() }
            else -> screen = when (screen) {
                Screen.Scanner -> Screen.Add
                Screen.CreateGroup -> Screen.Groups
                Screen.AddMember -> Screen.GroupChat
                else -> Screen.Contacts
            }
        }
    }

    val navigate: (Screen) -> Unit = { dest ->
        if (dest == Screen.Groups) model.refreshGroups()
        screen = dest
        scope.launch { drawerState.close() }
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        // Свайп-открытие меню — только с Главной (внутри чатов жест мешал бы прокрутке/назад).
        gesturesEnabled = screen == Screen.Contacts && model.dmCallPeer == null && model.incomingCall == null,
        drawerContent = { AppDrawer(model, navigate) },
    ) {
        Box(Modifier.fillMaxSize()) {
            Surface(Modifier.fillMaxSize(), color = HoloBg) {
                Column(Modifier.fillMaxSize()) {
                    // ── Шапка: конфигурируется по текущему экрану ─────────────────────
                    when (screen) {
                        Screen.Contacts -> TopBar(
                            if (activeFilter == "all") stringResource(R.string.nav_chats) else stringResource(folderLabel(activeFilter)),
                            onMenu = { scope.launch { drawerState.open() } },
                        ) {
                            PeerIndicator(model.netPeers) { model.startBenchRecv() }
                            BarIcon(Icons.Filled.Search, stringResource(R.string.cd_search)) { screen = Screen.Search }
                        }
                        Screen.Chat -> ChatTopBar(
                            model,
                            onBack = { screen = Screen.Contacts },
                            onProfile = { screen = Screen.Info },
                        )
                        Screen.Add -> TopBar(stringResource(R.string.title_add_contact), onBack = { screen = Screen.Contacts })
                        Screen.Me -> TopBar(stringResource(R.string.title_my_key), onBack = { screen = Screen.Contacts })
                        Screen.Info -> TopBar(stringResource(R.string.common_contact), onBack = { screen = Screen.Chat })
                        Screen.Groups -> TopBar(stringResource(R.string.title_groups), onBack = { screen = Screen.Contacts }) {
                            BarIcon(Icons.Filled.Add, stringResource(R.string.cd_new_group)) { screen = Screen.CreateGroup }
                        }
                        Screen.GroupChat -> GroupChatTopBar(
                            model,
                            onBack = { screen = Screen.Contacts },
                            onInfo = { screen = Screen.AddMember },
                        )
                        Screen.CreateGroup -> TopBar(stringResource(R.string.title_new_group), onBack = { screen = Screen.Groups })
                        Screen.AddMember -> TopBar(stringResource(R.string.title_add_member), onBack = { screen = Screen.GroupChat })
                        Screen.Settings -> TopBar(stringResource(R.string.menu_settings), onBack = { screen = Screen.Contacts })
                        Screen.Scanner -> TopBar(stringResource(R.string.title_scan_qr), onBack = { screen = Screen.Add })
                        Screen.ContactList -> TopBar(stringResource(R.string.menu_contacts), onBack = { screen = Screen.Contacts }) {
                            BarIcon(Icons.Filled.PersonAdd, stringResource(R.string.cd_add_contact)) { screen = Screen.Add }
                        }
                        Screen.Calls -> TopBar(stringResource(R.string.menu_calls), onBack = { screen = Screen.Contacts })
                        Screen.Network -> TopBar(stringResource(R.string.menu_network), onBack = { screen = Screen.Contacts }) {
                            BarIcon(Icons.Filled.Refresh, stringResource(R.string.cd_refresh)) { /* авто-поллинг; ручной рефреш — TODO */ }
                        }
                        Screen.Saved -> TopBar(stringResource(R.string.menu_saved), onBack = { screen = Screen.Contacts })
                        Screen.Search -> TopBar(stringResource(R.string.title_search), onBack = { screen = Screen.Contacts })
                    }
                    // ── Рейл папок: только на главном экране ────────────────────────
                    if (screen == Screen.Contacts) {
                        FolderRail(model, activeFilter) { activeFilter = it }
                    }
                    // ── Контент экрана (без шапки) ────────────────────────────────────
                    Box(Modifier.weight(1f)) {
                        when (screen) {
                            Screen.Contacts -> ContactsScreen(
                                model,
                                activeFilter,
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
                                scanResult = key
                                screen = Screen.Add
                            }
                            Screen.Me -> MeScreen(model)
                            Screen.Info -> InfoScreen(model) { screen = Screen.Contacts }
                            Screen.Groups -> GroupsScreen(model) {
                                model.openGroup(it)
                                screen = Screen.GroupChat
                            }
                            Screen.GroupChat -> GroupChatScreen(model)
                            Screen.CreateGroup -> CreateGroupScreen(model) { screen = Screen.Groups }
                            Screen.AddMember -> AddMemberScreen(model) { screen = Screen.GroupChat }
                            Screen.Settings -> SettingsScreen(model)
                            Screen.ContactList -> ContactListScreen(model) {
                                model.openChat(it)
                                screen = Screen.Chat
                            }
                            Screen.Calls -> CallsScreen()
                            Screen.Network -> NetworkScreen(model) { screen = Screen.Me }
                            Screen.Saved -> SavedScreen()
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
                    onClick = { screen = Screen.Add },
                    modifier = Modifier.align(Alignment.BottomEnd).padding(20.dp),
                    containerColor = HoloAccent,
                    contentColor = HoloAccentDark,
                ) { Icon(Icons.Filled.Create, stringResource(R.string.cd_new_chat), modifier = Modifier.size(22.dp)) }
            }
            model.incomingCall?.let { IncomingCallOverlay(model, it) }
            // Активный DM-звонок (аудио или видео) — полноэкранная комната-сетка.
            if (model.incomingCall == null) {
                model.dmCallPeer?.let { RoomCallScreen(model, it) }
            }
        }
    }
}
