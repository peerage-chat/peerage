package dev.p2p.messenger

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.BatteryStd
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.EmojiEmotions
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Create
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Forum
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material3.ExperimentalMaterial3Api
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.abs

// ── Единая сетка размеров/отступов ──────────────────────────────────────────
private val ScreenPad = 16.dp        // отступ контента экрана
private val FormGap = 12.dp          // вертикальный зазор в формах
private val RowPadV = 14.dp          // вертикальный паддинг ряда списка
private val OfflineDot = Color(0xFF555555)
private val WarnColor = Color(0xFFE6A23C)
private val BubbleIn = Color(0xFF24292F)   // входящий пузырь (нейтральный)
private val BubbleOut = Color(0xFF245266)  // исходящий пузырь (приглушённый акцент)
private val FolderSelected = Color(0xFF1C3A47) // фон активной папки (приглушённый, не неон)

/** Время сообщения «ЧЧ:ММ» (time — unix-секунды). */
private fun fmtMsgTime(ts: Long): String =
    java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))

/** Метка дня для чипа-разделителя: Сегодня / Вчера / «d MMMM». */
private fun dayLabel(ts: Long): String {
    val msgDay = java.util.Calendar.getInstance().apply { timeInMillis = ts * 1000 }
    val today = java.util.Calendar.getInstance()
    fun sameDay(a: java.util.Calendar, b: java.util.Calendar) =
        a.get(java.util.Calendar.YEAR) == b.get(java.util.Calendar.YEAR) &&
        a.get(java.util.Calendar.DAY_OF_YEAR) == b.get(java.util.Calendar.DAY_OF_YEAR)
    val yesterday = (today.clone() as java.util.Calendar).apply { add(java.util.Calendar.DAY_OF_YEAR, -1) }
    return when {
        sameDay(msgDay, today) -> "Сегодня"
        sameDay(msgDay, yesterday) -> "Вчера"
        else -> java.text.SimpleDateFormat("d MMMM", java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))
    }
}

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
        model.error != null -> CenterMessage("Ошибка: ${model.error}")
        model.me == null -> CenterMessage("Подключение к сети…")
        else -> Root(model)
    }
}

@Composable
private fun CenterMessage(text: String) {
    Box(Modifier.fillMaxSize().background(HoloBg), contentAlignment = Alignment.Center) {
        Text(text, color = HoloSecondary, fontSize = 14.sp)
    }
}

private enum class Screen { Contacts, Chat, Add, Me, Info, Groups, GroupChat, CreateGroup, AddMember, Settings, Scanner, ContactList, Calls, Network, Saved }

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
            drawerState.isOpen || screen != Screen.Contacts
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
                        if (activeFilter == "all") "Чаты" else folderLabel(activeFilter),
                        onMenu = { scope.launch { drawerState.open() } },
                    ) {
                        PeerIndicator(model.netPeers) { model.startBenchRecv() }
                        BarIcon(Icons.Filled.Search, "Поиск") { /* TODO: глобальный поиск */ }
                    }
                    Screen.Chat -> ChatTopBar(
                        model,
                        onBack = { screen = Screen.Contacts },
                        onProfile = { screen = Screen.Info },
                    )
                    Screen.Add -> TopBar("Добавить контакт", onBack = { screen = Screen.Contacts })
                    Screen.Me -> TopBar("Мой ключ", onBack = { screen = Screen.Contacts })
                    Screen.Info -> TopBar("Контакт", onBack = { screen = Screen.Chat })
                    Screen.Groups -> TopBar("Группы", onBack = { screen = Screen.Contacts }) {
                        BarIcon(Icons.Filled.Add, "Новая группа") { screen = Screen.CreateGroup }
                    }
                    Screen.GroupChat -> GroupChatTopBar(
                        model,
                        onBack = { screen = Screen.Contacts },
                        onInfo = { screen = Screen.AddMember },
                    )
                    Screen.CreateGroup -> TopBar("Новая группа", onBack = { screen = Screen.Groups })
                    Screen.AddMember -> TopBar("Добавить участника", onBack = { screen = Screen.GroupChat })
                    Screen.Settings -> TopBar("Настройки", onBack = { screen = Screen.Contacts })
                    Screen.Scanner -> TopBar("Сканировать QR", onBack = { screen = Screen.Add })
                    Screen.ContactList -> TopBar("Контакты", onBack = { screen = Screen.Contacts }) {
                        BarIcon(Icons.Filled.PersonAdd, "Добавить контакт") { screen = Screen.Add }
                    }
                    Screen.Calls -> TopBar("Звонки", onBack = { screen = Screen.Contacts })
                    Screen.Network -> TopBar("Сеть", onBack = { screen = Screen.Contacts }) {
                        BarIcon(Icons.Filled.Refresh, "Обновить") { /* авто-поллинг; ручной рефреш — TODO */ }
                    }
                    Screen.Saved -> TopBar("Сохранённое", onBack = { screen = Screen.Contacts })
                }
                // ── Рейл папок: только на главном экране ────────────────────────
                if (screen == Screen.Contacts) {
                    FolderRail(model, activeFilter) { activeFilter = it }
                }
                // ── Контент экрана (без шапки) ────────────────────────────────────
                Box(Modifier.weight(1f)) {
                    when (screen) {
                        Screen.Contacts -> ContactsScreen(
                            model, activeFilter,
                            onOpenDm = { model.openChat(it); screen = Screen.Chat },
                            onOpenGroup = { model.openGroup(it); screen = Screen.GroupChat },
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
                        Screen.Groups -> GroupsScreen(model) { model.openGroup(it); screen = Screen.GroupChat }
                        Screen.GroupChat -> GroupChatScreen(model)
                        Screen.CreateGroup -> CreateGroupScreen(model) { screen = Screen.Groups }
                        Screen.AddMember -> AddMemberScreen(model) { screen = Screen.GroupChat }
                        Screen.Settings -> SettingsScreen(model)
                        Screen.ContactList -> ComingSoon(Icons.Filled.Group, "Контакты")
                        Screen.Calls -> ComingSoon(Icons.Filled.Call, "История звонков")
                        Screen.Network -> NetworkScreen(model) { screen = Screen.Me }
                        Screen.Saved -> ComingSoon(Icons.Filled.Lock, "Сохранённое")
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
            ) { Icon(Icons.Filled.Create, "Новый чат", modifier = Modifier.size(22.dp)) }
        }
        model.incomingCall?.let { IncomingCallOverlay(model, it) }
        // Активный DM-звонок (аудио или видео) — полноэкранная комната-сетка.
        if (model.incomingCall == null) {
            model.dmCallPeer?.let { RoomCallScreen(model, it) }
        }
    }
    }
}

// ── Общие элементы ──────────────────────────────────────────────────────────

/** Иконка-кнопка топ-бара: единый размер (48dp касание / 24dp иконка). */
@Composable
private fun BarIcon(icon: ImageVector, desc: String, tint: Color = HoloAccent, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(icon, contentDescription = desc, tint = tint, modifier = Modifier.size(24.dp))
    }
}

@Composable
private fun TopBar(
    title: String,
    onBack: (() -> Unit)? = null,
    onMenu: (() -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when {
            onMenu != null -> BarIcon(Icons.Filled.Menu, "Меню", onClick = onMenu)
            onBack != null -> BarIcon(Icons.AutoMirrored.Filled.ArrowBack, "Назад", onClick = onBack)
            else -> Spacer(Modifier.width(12.dp))
        }
        Text(title, color = Color.White, fontSize = 19.sp, fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        actions()
    }
}

/** Индикатор присутствия: маленький кружок (онлайн/офлайн). */
@Composable
private fun StatusDot(online: Boolean) {
    Box(Modifier.size(10.dp).clip(CircleShape).background(if (online) HoloOnline else OfflineDot))
}

/** Индикатор пиров меша в шапке Главной (⊹ N). Тап → «Сеть» (TODO),
 *  долгий тап — диагностический бенч приёма. */
@Composable
private fun PeerIndicator(peers: Int, onLongPress: () -> Unit = {}) {
    Row(
        Modifier.clip(RoundedCornerShape(8.dp))
            .pointerInput(Unit) {
                detectTapGestures(
                    onTap = { /* TODO: экран Сеть */ },
                    onLongPress = { onLongPress() },
                )
            }
            .padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Icon(Icons.Filled.Share, "Сеть", tint = if (peers > 0) HoloOnline else HoloSecondary,
            modifier = Modifier.size(18.dp))
        Text("$peers", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Medium)
    }
}

/** Квадратная аватарка со скруглением — единый визуальный язык приложения
 *  (НЕ круг). Буква имени на детерминированном цвете; опционально presence-точка
 *  в правом-нижнем углу; [bg]/[content] переопределяют фон и содержимое (комнаты). */
@Composable
private fun SquareAvatar(
    name: String,
    size: Dp = 44.dp,
    online: Boolean? = null,
    bg: Color? = null,
    content: (@Composable () -> Unit)? = null,
) {
    Box(Modifier.size(size), contentAlignment = Alignment.BottomEnd) {
        Box(
            Modifier.size(size).clip(RoundedCornerShape(size * 0.3f)).background(bg ?: avatarColor(name)),
            contentAlignment = Alignment.Center,
        ) {
            if (content != null) content()
            else Text(name.take(1).uppercase(), color = Color.White,
                fontSize = (size.value * 0.4f).sp, fontWeight = FontWeight.Medium)
        }
        if (online != null) {
            Box(
                Modifier.size(size * 0.28f).clip(CircleShape)
                    .background(if (online) HoloOnline else OfflineDot)
                    .border(2.dp, HoloBg, CircleShape),
            )
        }
    }
}

@Composable
private fun ListRow(onClick: () -> Unit, content: @Composable RowScope.() -> Unit) {
    // Без разделителей — мягкий «телеграмный» список (по макету).
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = ScreenPad, vertical = RowPadV),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

@Composable
private fun FieldLabel(text: String) {
    Text(text, color = HoloSecondary, fontSize = 11.sp, fontWeight = FontWeight.Medium)
}

@Composable
private fun AccentButton(text: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Button(onClick = onClick, modifier = modifier,
        colors = ButtonDefaults.buttonColors(containerColor = HoloAccent)) {
        Text(text, color = HoloAccentDark, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun SendButton(onClick: () -> Unit) {
    Box(
        Modifier.clip(CircleShape).background(HoloAccent).size(44.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(Icons.AutoMirrored.Filled.Send, "Отправить", tint = HoloAccentDark, modifier = Modifier.size(20.dp)) }
}

// ── Экраны ──────────────────────────────────────────────────────────────────

@Composable
private fun ContactsScreen(
    model: AppModel,
    activeFilter: String,
    onOpenDm: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    val showDm = activeFilter == "all" || activeFilter == "dm"
    val showRoom = activeFilter == "all" || activeFilter == "room"

    Column(Modifier.fillMaxSize()) {
        // Счётчик пиров переехал в шапку (PeerIndicator). Здесь — только бенч-замер
        // приёма, пока он активен (диагностика; запуск — долгим тапом по индикатору).
        if (model.benchRecv) {
            Text("⏱ приём: %.1f Мбит/с".format(model.benchRecvMbps),
                color = HoloAccent, fontSize = 11.sp,
                modifier = Modifier.padding(horizontal = ScreenPad, vertical = 6.dp))
        }
        when {
            activeFilter == "channel" -> CenterMessage("Каналы — скоро")
            model.contacts.isEmpty() && model.groups.isEmpty() ->
                CenterMessage("Нет контактов. Добавьте по ключу.")
            else -> LazyColumn(Modifier.fillMaxSize()) {
                // Комнаты — наверху, если фильтр позволяет
                if (showRoom) {
                    items(model.groups, key = { "g_${it.gid}" }) { g ->
                        val presenceCount = model.groupPresence[g.gid]?.size ?: 0
                        val inRoom = model.currentRoom == g.gid
                        ListRow(onClick = { onOpenGroup(g.gid) }) {
                            SquareAvatar(g.name, size = 44.dp, bg = Color(0xFF0D2A0D)) {
                                Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
                            }
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(g.name, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    if (inRoom) "В эфире"
                                    else if (presenceCount > 0) "$presenceCount в комнате"
                                    else "${g.members} уч.",
                                    color = if (inRoom || presenceCount > 0) HoloOnline else HoloSecondary,
                                    fontSize = 11.sp,
                                )
                            }
                        }
                    }
                }
                // DM-контакты
                if (showDm) {
                    items(model.contacts, key = { "c_${it.key}" }) { c ->
                        val online = model.online[c.key] == true
                        ListRow(onClick = { onOpenDm(c.key) }) {
                            SquareAvatar(c.name, size = 44.dp, online = online)
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(c.name, color = Color.White, fontSize = 15.sp, maxLines = 1,
                                        overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                                    if (c.verified) {
                                        Spacer(Modifier.width(4.dp))
                                        Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(13.dp))
                                    }
                                }
                                Text(
                                    if (online) "онлайн" else "не в сети",
                                    color = if (online) HoloOnline else HoloSecondary,
                                    fontSize = 11.sp,
                                )
                            }
                            val unread = model.unread[c.key] ?: 0
                            if (unread > 0) {
                                Box(Modifier.clip(CircleShape).background(HoloAccent).padding(horizontal = 7.dp, vertical = 2.dp)) {
                                    Text("$unread", color = HoloAccentDark, fontSize = 11.sp)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Шапка личного чата: назад · аватар+presence · имя+verified · статус (тап →
 *  профиль) · аудиозвонок · меню. Видеозвонка нет — камеру включают внутри звонка. */
@Composable
private fun ChatTopBar(model: AppModel, onBack: () -> Unit, onProfile: () -> Unit) {
    val key = model.currentChat
    val c = key?.let { k -> model.contacts.firstOrNull { it.key == k } }
    val name = c?.name ?: "Чат"
    val online = key?.let { model.online[it] == true } ?: false
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, "Назад", onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).clickable { onProfile() }
                .padding(horizontal = 4.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(name, size = 38.dp, online = online)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(name, color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Medium,
                        maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                    if (c?.verified == true) {
                        Spacer(Modifier.width(4.dp))
                        Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(14.dp))
                    }
                }
                Text(if (online) "в сети" else "не в сети",
                    color = if (online) HoloOnline else HoloSecondary, fontSize = 11.sp)
            }
        }
        if (key != null) BarIcon(Icons.Filled.Call, "Аудио-звонок") { model.callDm(key) }
        BarIcon(Icons.Filled.MoreVert, "Ещё", tint = Color.White, onClick = onProfile)
    }
}

@Composable
private fun ChatScreen(model: AppModel) {
    val key = model.currentChat ?: return
    val msgs = model.messages[key] ?: emptyList()
    var input by remember(key) { mutableStateOf("") }
    val listState = rememberLazyListState()
    // Держим прокрутку у последнего сообщения при появлении новых.
    LaunchedEffect(msgs.size) { if (msgs.isNotEmpty()) listState.animateScrollToItem(msgs.size) }

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
        ) {
            item { E2EBanner() }
            itemsIndexed(msgs) { i, m ->
                val prev = msgs.getOrNull(i - 1)
                if (prev == null || dayLabel(prev.time) != dayLabel(m.time)) DateChip(dayLabel(m.time))
                MessageBubble(m)
            }
        }
        MessageInput(input, { input = it }) {
            if (input.isNotBlank()) { model.send(key, input); input = "" }
        }
    }
}

/** Системная строка «E2E» в начале переписки. */
@Composable
private fun E2EBanner() {
    Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalArrangement = Arrangement.Center) {
        Row(
            Modifier.clip(RoundedCornerShape(8.dp)).background(HoloSurface)
                .padding(horizontal = 10.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Lock, null, tint = HoloSecondary, modifier = Modifier.size(12.dp))
            Spacer(Modifier.width(5.dp))
            Text("Сообщения защищены сквозным шифрованием", color = HoloSecondary, fontSize = 11.sp)
        }
    }
}

/** Чип-разделитель даты. */
@Composable
private fun DateChip(label: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
        Box(
            Modifier.clip(RoundedCornerShape(10.dp)).background(HoloSurface)
                .padding(horizontal = 10.dp, vertical = 3.dp),
        ) { Text(label, color = HoloSecondary, fontSize = 11.sp) }
    }
}

@Composable
private fun MessageInput(value: String, onChange: (String) -> Unit, onSend: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(HoloBg).padding(horizontal = 8.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Поле-пилюля: скрепка и эмодзи ВНУТРИ.
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(24.dp)).background(HoloSurface),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            PillIcon(Icons.Filled.AttachFile, "Прикрепить") { /* TODO: вложения */ }
            BasicTextField(
                value = value,
                onValueChange = onChange,
                modifier = Modifier.weight(1f).padding(vertical = 12.dp),
                textStyle = TextStyle(color = Color.White, fontSize = 15.sp),
                cursorBrush = SolidColor(HoloAccent),
                maxLines = 4,
                decorationBox = { inner ->
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (value.isEmpty()) Text("Сообщение…", color = HoloSecondary, fontSize = 15.sp)
                        inner()
                    }
                },
            )
            PillIcon(Icons.Filled.EmojiEmotions, "Эмодзи") { /* TODO: эмодзи */ }
        }
        Spacer(Modifier.width(8.dp))
        // Отправка / голосовое — отдельной круглой кнопкой 44px.
        val hasText = value.isNotBlank()
        Box(
            Modifier.clip(CircleShape).background(HoloAccent).size(44.dp)
                .clickable { if (hasText) onSend() /* else TODO: запись голосового */ },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                if (hasText) Icons.AutoMirrored.Filled.Send else Icons.Filled.Mic,
                if (hasText) "Отправить" else "Голосовое",
                tint = HoloAccentDark, modifier = Modifier.size(20.dp),
            )
        }
    }
}

/** Иконка внутри поля-пилюли (44px касание). */
@Composable
private fun PillIcon(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(CircleShape).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = HoloSecondary, modifier = Modifier.size(22.dp)) }
}

@Composable
private fun MessageBubble(m: Message) {
    val timeStr = fmtMsgTime(m.time)
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
        Box(
            Modifier.widthIn(max = 290.dp).clip(RoundedCornerShape(14.dp))
                .background(if (m.mine) BubbleOut else BubbleIn)
                .padding(horizontal = 10.dp, vertical = 6.dp),
        ) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(m.text, color = Color.White, fontSize = 15.sp,
                    modifier = Modifier.weight(1f, fill = false).alignByBaseline())
                Spacer(Modifier.width(8.dp))
                // Время по базовой линии текста → садится в правый-нижний угол.
                Text(timeStr, color = Color.White.copy(alpha = 0.5f), fontSize = 10.sp,
                    modifier = Modifier.alignByBaseline())
                if (m.mine) {
                    Spacer(Modifier.width(3.dp))
                    StatusTick(m.status)
                }
            }
        }
    }
}

/** Галочка статуса доставки исходящего сообщения. */
@Composable
private fun StatusTick(status: String) {
    val tint = Color.White.copy(alpha = 0.7f)
    when (status) {
        "delivered" -> Icon(Icons.Filled.DoneAll, "доставлено", tint = tint, modifier = Modifier.size(13.dp))
        "sent" -> Icon(Icons.Filled.Done, "отправлено", tint = tint, modifier = Modifier.size(13.dp))
        "pending" -> Icon(Icons.Filled.Schedule, "отправляется", tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(12.dp))
        "queued" -> Icon(Icons.Filled.Schedule, "ожидание", tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(12.dp))
        "failed" -> Text("!", color = HoloError, fontSize = 11.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun AddScreen(
    model: AppModel,
    scannedKey: String? = null,
    onScan: () -> Unit = {},
    onDone: () -> Unit,
) {
    var keyHex by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    // Операция асинхронна; результат приходит в model.addStatus.
    LaunchedEffect(Unit) { model.addStatus = "" }
    val st = model.addStatus
    LaunchedEffect(st) { if (st == "added") { model.addStatus = ""; onDone() } }
    // Получаем ключ из сканера (приходит через Root → scannedKey).
    LaunchedEffect(scannedKey) { if (scannedKey != null) keyHex = scannedKey }
    Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
        FieldLabel("ПУБЛИЧНЫЙ КЛЮЧ (hex)")
        TextField(keyHex, { keyHex = it }, Modifier.fillMaxWidth(), colors = inputColors(),
            placeholder = { Text("64 hex-символа", color = HoloSecondary) })
        AccentButton("Сканировать QR", modifier = Modifier.fillMaxWidth()) { onScan() }
        FieldLabel("ИМЯ")
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
        AccentButton("Добавить") { model.addContact(keyHex.trim(), name.trim()) }
        val msg = when (st) { "known" -> "Контакт уже есть"; "error" -> "Неверный ключ"; else -> "" }
        if (msg.isNotEmpty()) Text(msg, color = HoloError, fontSize = 13.sp)
    }
}

@Composable
private fun MeScreen(model: AppModel) {
    val me = model.me ?: return
    val qr = remember(me.pubkey) { generateQrBitmap(me.pubkey) }
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(ScreenPad).fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(FormGap),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (qr != null) {
            Image(
                bitmap = qr.asImageBitmap(),
                contentDescription = "QR код публичного ключа",
                modifier = Modifier.size(240.dp).clip(RoundedCornerShape(8.dp)),
            )
            Spacer(Modifier.height(4.dp))
        }
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(FormGap)) {
            FieldLabel("FINGERPRINT (сверьте при встрече)")
            Text(me.fingerprint, color = HoloAccent, fontSize = 16.sp)
            FieldLabel("АДРЕС YGGDRASIL")
            Text(me.address, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
            FieldLabel("ПУБЛИЧНЫЙ КЛЮЧ")
            Text(me.pubkey, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
            val ctx = LocalContext.current
            AccentButton("Скопировать ключ", modifier = Modifier.fillMaxWidth()) {
                val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                        as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("p2p pubkey", me.pubkey))
                // API 33+ сам показывает тост «Скопировано», на старых — делаем вручную.
                if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
                    android.widget.Toast.makeText(ctx, "Скопировано", android.widget.Toast.LENGTH_SHORT).show()
                }
            }
        }
    }
}

@Composable
private fun InfoScreen(model: AppModel, onDeleted: () -> Unit) {
    val key = model.currentChat ?: return
    val c = model.contacts.firstOrNull { it.key == key } ?: return
    var name by remember(key) { mutableStateOf(c.name) }
    Column(Modifier.verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(FormGap)) {
            FieldLabel("ИМЯ")
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextField(name, { name = it }, Modifier.weight(1f), colors = inputColors())
                Spacer(Modifier.width(8.dp))
                AccentButton("OK") { model.rename(key, name) }
            }
            FieldLabel("FINGERPRINT")
            Text(c.fingerprint, color = HoloAccent, fontSize = 15.sp)
            Text("Сверьте слова с собеседником по другому каналу — защита от подмены ключа.",
                color = HoloSecondary, fontSize = 11.sp)
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (c.verified) {
                    Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                }
                Text(if (c.verified) "Проверен" else "Не проверен",
                    color = if (c.verified) HoloVerified else WarnColor, modifier = Modifier.weight(1f))
                AccentButton(if (c.verified) "Снять" else "Пометить") { model.toggleVerified(key) }
            }
            FieldLabel("ПУБЛИЧНЫЙ КЛЮЧ")
            Text(c.key, color = Color.White, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
            Button(onClick = { model.remove(key); onDeleted() }, modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(containerColor = HoloError)) {
                Icon(Icons.Filled.Delete, null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Удалить контакт", color = Color.White)
            }
            // Диагностика: замер скорости меша до этого контакта (~30 МБ блба).
            FieldLabel("ДИАГНОСТИКА")
            AccentButton("Тест скорости") { model.blastBench(key) }
            if (model.benchResult.isNotEmpty()) {
                Text(model.benchResult, color = HoloSecondary, fontSize = 12.sp)
            }
    }
}

@Composable
private fun GroupsScreen(model: AppModel, onOpen: (String) -> Unit) {
    if (model.groups.isEmpty()) {
        CenterMessage("Нет групп. Создайте новую.")
    } else {
        LazyColumn(Modifier.fillMaxSize()) {
            items(model.groups, key = { it.gid }) { g ->
                ListRow(onClick = { onOpen(g.gid) }) {
                    Icon(Icons.Filled.Group, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
                    Spacer(Modifier.width(12.dp))
                    Text(g.name, color = Color.White, fontSize = 16.sp, modifier = Modifier.weight(1f))
                    Text("${g.members} уч.", color = HoloSecondary, fontSize = 12.sp)
                }
            }
        }
    }
}

@Composable
private fun GroupChatScreen(model: AppModel) {
    val gid = model.currentGroup ?: return
    val msgs = model.groupMessages[gid] ?: emptyList()
    var input by remember(gid) { mutableStateOf("") }
    var focusedParticipant by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    LaunchedEffect(msgs.size) { if (msgs.isNotEmpty()) listState.animateScrollToItem(msgs.size) }

    val inRoom = model.currentRoom == gid
    val presenceMembers = if (inRoom) emptyList() else (model.groupPresence[gid] ?: emptyList())

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            // В комнате — сетка звонка; иначе если есть присутствие — зелёный баннер.
            if (inRoom) RoomGrid(model, true, emptyList(), onFocus = { focusedParticipant = it })
            else if (presenceMembers.isNotEmpty()) RoomBanner(model, presenceMembers) { model.joinRoom(gid) }

            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
            ) {
                itemsIndexed(msgs) { i, m ->
                    val prev = msgs.getOrNull(i - 1)
                    if (prev == null || dayLabel(prev.time) != dayLabel(m.time)) DateChip(dayLabel(m.time))
                    GroupMessageRow(model, m)
                }
            }
            MessageInput(input, { input = it }) {
                if (input.isNotBlank()) { model.sendGroupText(gid, input); input = "" }
            }
        }
        focusedParticipant?.let { key ->
            val isMe = key == "me"
            val name = if (isMe) "Вы" else
                model.contacts.firstOrNull { it.key == key }?.name ?: (key.take(6) + "…")
            val audio = if (isMe) true else model.roomMembers.firstOrNull { it.peer == key }?.audio ?: true
            val video = if (isMe) false else model.roomMembers.firstOrNull { it.peer == key }?.video ?: false
            ParticipantFullscreen(name, audio, video, isMe) { focusedParticipant = null }
        }
    }
}

/** Шапка группового чата: аватар группы · имя · «N уч · M в сети» (тап → инфо) ·
 *  войти/выйти из комнаты · меню. */
@Composable
private fun GroupChatTopBar(model: AppModel, onBack: () -> Unit, onInfo: () -> Unit) {
    val gid = model.currentGroup
    val g = gid?.let { model.groups.firstOrNull { it.gid == gid } }
    val name = g?.name ?: "Группа"
    val total = g?.members ?: 1
    val others = model.groupMemberKeys[gid] ?: emptyList()
    val onlineN = 1 + others.count { model.online[it] == true } // +1 = мы сами
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.AutoMirrored.Filled.ArrowBack, "Назад", onClick = onBack)
        Row(
            Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).clickable { onInfo() }
                .padding(horizontal = 4.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(name, size = 38.dp, bg = Color(0xFF0D2A0D)) {
                Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(20.dp))
            }
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(name, color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Medium,
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text("$total уч. · $onlineN в сети", color = HoloSecondary, fontSize = 11.sp)
            }
        }
        if (gid != null) {
            if (model.currentRoom == gid) {
                BarIcon(Icons.Filled.CallEnd, "Выйти из звонка") { model.leaveRoom() }
            } else {
                BarIcon(Icons.Filled.Call, "Войти в звонок") { model.joinRoom(gid) }
            }
        }
        BarIcon(Icons.Filled.MoreVert, "Ещё", tint = Color.White, onClick = onInfo)
    }
}

/** Зелёный баннер активной комнаты: «N в звонке · имена» + Войти. */
@Composable
private fun RoomBanner(model: AppModel, members: List<RoomMember>, onJoin: () -> Unit) {
    val names = members.joinToString(", ") { rm ->
        model.contacts.firstOrNull { it.key == rm.peer }?.name ?: (rm.peer.take(6) + "…")
    }
    Row(
        Modifier.fillMaxWidth().background(Color(0xFF12331A)).padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text("${members.size} в звонке", color = HoloOnline, fontSize = 13.sp, fontWeight = FontWeight.Medium)
            if (names.isNotEmpty()) Text(names, color = HoloSecondary, fontSize = 11.sp,
                maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.width(12.dp))
        Box(
            Modifier.clip(RoundedCornerShape(10.dp)).background(HoloOnline)
                .clickable { onJoin() }.padding(horizontal = 18.dp, vertical = 8.dp),
        ) { Text("Войти", color = Color(0xFF0A1F0A), fontSize = 14.sp, fontWeight = FontWeight.Medium) }
    }
}

/** Строка группового сообщения: системная / своя / входящая (имя цветом + аватар). */
@Composable
private fun GroupMessageRow(model: AppModel, m: GroupMsg) {
    when {
        m.system -> {
            val who = groupSenderName(model, m.sender)
            val text = if (m.text.isBlank()) "$who обновил(а) состав группы" else "$who ${m.text}"
            Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
                Box(Modifier.clip(RoundedCornerShape(10.dp)).background(HoloSurface)
                    .padding(horizontal = 10.dp, vertical = 4.dp)) {
                    Text(text, color = HoloSecondary, fontSize = 11.sp)
                }
            }
        }
        m.mine -> Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), horizontalArrangement = Arrangement.End) {
            GroupBubble(m.text, true, m.time)
        }
        else -> Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), horizontalArrangement = Arrangement.Start) {
            val who = groupSenderName(model, m.sender)
            val online = model.online[m.sender] == true
            SquareAvatar(who, size = 32.dp, online = online)
            Spacer(Modifier.width(8.dp))
            Column {
                Text(who, color = nameColor(m.sender), fontSize = 12.sp, fontWeight = FontWeight.Medium,
                    modifier = Modifier.padding(start = 2.dp, bottom = 2.dp))
                GroupBubble(m.text, false, m.time)
            }
        }
    }
}

/** Пузырь группового сообщения (время в углу последней строки; без галочек). */
@Composable
private fun GroupBubble(text: String, mine: Boolean, time: Long) {
    Box(
        Modifier.widthIn(max = 280.dp).clip(RoundedCornerShape(14.dp))
            .background(if (mine) BubbleOut else BubbleIn).padding(horizontal = 10.dp, vertical = 6.dp),
    ) {
        Row(verticalAlignment = Alignment.Bottom) {
            Text(text, color = Color.White, fontSize = 15.sp,
                modifier = Modifier.weight(1f, fill = false).alignByBaseline())
            Spacer(Modifier.width(8.dp))
            Text(fmtMsgTime(time), color = Color.White.copy(alpha = 0.5f), fontSize = 10.sp,
                modifier = Modifier.alignByBaseline())
        }
    }
}

/** Имя отправителя группового сообщения по hex-ключу. */
private fun groupSenderName(model: AppModel, hex: String): String = when {
    hex.isEmpty() -> "Участник"
    hex == model.me?.pubkey -> "Вы"
    else -> model.contacts.firstOrNull { it.key == hex }?.name ?: (hex.take(6) + "…")
}

/** Яркий детерминированный цвет имени отправителя (Discord-style). */
private val NameColors = listOf(
    Color(0xFF5BC0EB), Color(0xFF9BC53D), Color(0xFFE5793A), Color(0xFFEF6FB3),
    Color(0xFF7C5CFC), Color(0xFF20C997), Color(0xFFF6C453),
)
private fun nameColor(hex: String): Color =
    if (hex.isEmpty()) HoloSecondary else NameColors[abs(hex.hashCode()) % NameColors.size]

/** Сетка участников комнаты (2 колонки). Тап по тайлу → onFocus(key).
 *  [inRoom] — мы сейчас в комнате (показываем себя + model.roomMembers).
 *  [presenceMembers] — участники по маячкам когда мы ещё не зашли. */
@Composable
private fun RoomGrid(
    model: AppModel,
    inRoom: Boolean,
    presenceMembers: List<RoomMember>,
    onFocus: (String) -> Unit,
) {
    val members = if (inRoom) model.roomMembers else presenceMembers
    val total = if (inRoom) members.size + 1 else members.size
    Surface(color = HoloSurface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 10.dp, vertical = 10.dp)) {
            Text(
                if (inRoom) "В ЗВОНКЕ · $total" else "В КОМНАТЕ · $total",
                color = HoloSecondary, fontSize = 11.sp,
                modifier = Modifier.padding(start = 2.dp, bottom = 8.dp),
            )
            val all = if (inRoom)
                listOf(Triple("me", true, false)) + members.map { Triple(it.peer, it.audio, it.video) }
            else
                members.map { Triple(it.peer, it.audio, it.video) }
            all.chunked(2).forEach { row ->
                Row(
                    Modifier.fillMaxWidth().padding(bottom = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    row.forEach { (key, audio, video) ->
                        val isMe = key == "me"
                        val name = if (isMe) "Вы" else
                            model.contacts.firstOrNull { it.key == key }?.name ?: (key.take(6) + "…")
                        GridTile(name, audio, video, isMe, Modifier.weight(1f)) { onFocus(key) }
                    }
                    if (row.size == 1) Spacer(Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
private fun GridTile(
    name: String, audio: Boolean, video: Boolean, me: Boolean,
    modifier: Modifier = Modifier, onClick: () -> Unit,
) {
    Box(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(if (me) HoloAccentDark else Color(0xFF2A2F35))
            .clickable { onClick() }
            .aspectRatio(1f),
    ) {
        Column(
            Modifier.fillMaxSize().padding(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(64.dp).clip(CircleShape)
                    .background(if (me) HoloAccent else Color(0xFF3A3F45)),
                contentAlignment = Alignment.Center,
            ) {
                Text(name.take(1).uppercase(), color = if (me) HoloAccentDark else Color.White,
                    fontWeight = FontWeight.Bold, fontSize = 26.sp)
            }
            Spacer(Modifier.height(10.dp))
            Text(name, color = if (me) HoloAccent else Color.White,
                fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth(),
                textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        }
        // Статус микрофона/видео — правый верхний угол
        if (!audio || video) {
            Row(
                Modifier.align(Alignment.TopEnd).padding(6.dp)
                    .clip(RoundedCornerShape(6.dp)).background(Color(0xAA000000))
                    .padding(horizontal = 5.dp, vertical = 3.dp),
                horizontalArrangement = Arrangement.spacedBy(3.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (!audio) Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(13.dp))
                if (video) Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(13.dp))
            }
        }
    }
}

@Composable
private fun ParticipantFullscreen(
    name: String, audio: Boolean, video: Boolean, me: Boolean, onDismiss: () -> Unit,
) {
    Box(Modifier.fillMaxSize().background(Color(0xEE000000)).clickable { onDismiss() }) {
        // Кнопка закрытия — правый верхний угол
        Box(
            Modifier.align(Alignment.TopEnd).padding(16.dp).size(36.dp)
                .clip(CircleShape).background(Color(0x33FFFFFF)).clickable { onDismiss() },
            contentAlignment = Alignment.Center,
        ) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, "Закрыть", tint = Color.White,
                modifier = Modifier.size(18.dp).then(
                    Modifier.graphicsLayer { rotationZ = 90f }
                ))
        }
        // Центральный контент
        Column(
            Modifier.fillMaxSize(),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Box(
                Modifier.size(128.dp).clip(CircleShape)
                    .background(if (me) HoloAccent else Color(0xFF3A3F45))
                    .border(3.dp, if (me) HoloAccent.copy(alpha = 0.4f) else Color(0xFF4A4F55), CircleShape),
                contentAlignment = Alignment.Center,
            ) {
                Text(name.take(1).uppercase(), color = if (me) HoloAccentDark else Color.White,
                    fontWeight = FontWeight.Bold, fontSize = 52.sp)
            }
            Spacer(Modifier.height(20.dp))
            Text(name, color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(10.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (!audio) Row(
                    Modifier.clip(RoundedCornerShape(8.dp)).background(Color(0x33E53935))
                        .padding(horizontal = 10.dp, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(14.dp))
                    Text("Микрофон выкл", color = WarnColor, fontSize = 12.sp)
                }
                if (video) Row(
                    Modifier.clip(RoundedCornerShape(8.dp)).background(Color(0x22FFFFFF))
                        .padding(horizontal = 10.dp, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(14.dp))
                    Text("Видео", color = HoloSecondary, fontSize = 12.sp)
                }
            }
        }
    }
}

@Composable
private fun CreateGroupScreen(model: AppModel, onDone: () -> Unit) {
    var name by remember { mutableStateOf("") }
    Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
        FieldLabel("НАЗВАНИЕ")
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
        AccentButton("Создать") { model.createGroup(name); onDone() }
        Text("Вы станете администратором. Добавляйте участников из контактов внутри группы.",
            color = HoloSecondary, fontSize = 11.sp)
    }
}

@Composable
private fun AddMemberScreen(model: AppModel, onDone: () -> Unit) {
    val gid = model.currentGroup ?: return
    if (model.contacts.isEmpty()) {
        CenterMessage("Нет контактов для приглашения.")
    } else {
        LazyColumn(Modifier.fillMaxSize()) {
            items(model.contacts, key = { it.key }) { c ->
                ListRow(onClick = { model.inviteToGroup(gid, c.key); onDone() }) {
                    StatusDot(model.online[c.key] == true)
                    Spacer(Modifier.width(12.dp))
                    Text(c.name, color = Color.White, fontSize = 16.sp)
                }
            }
        }
    }
}

// ── Входящий звонок (полный экран, Telegram-style) ──────────────────────────
// Активный звонок — см. RoomCallScreen.kt (сетка участников).

@Composable
private fun IncomingCallOverlay(model: AppModel, ic: IncomingRoomCall) {
    val name = model.contacts.firstOrNull { it.key == ic.peer }?.name ?: "Контакт"
    Box(Modifier.fillMaxSize().background(Color(0xF2000000))) {
        Column(
            Modifier.fillMaxSize().padding(bottom = 64.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Spacer(Modifier.height(96.dp))
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    Modifier.size(88.dp).clip(CircleShape).background(HoloAccent),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(name.take(1).uppercase(), color = HoloAccentDark,
                        fontWeight = FontWeight.Bold, fontSize = 40.sp)
                }
                Spacer(Modifier.height(20.dp))
                Text(name, color = Color.White, fontSize = 26.sp, fontWeight = FontWeight.Medium)
                Text("Входящий звонок", color = HoloSecondary, fontSize = 15.sp,
                    modifier = Modifier.padding(top = 8.dp))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(56.dp),
                verticalAlignment = Alignment.CenterVertically) {
                CallButton(Icons.Filled.CallEnd, "Отклонить", HoloError) { model.declineDmCall() }
                CallButton(Icons.Filled.Call, "Принять", HoloOnline) { model.acceptDmCall() }
            }
        }
    }
}

@Composable
private fun CallButton(icon: ImageVector, desc: String, color: Color, onClick: () -> Unit) {
    Box(
        Modifier.clip(CircleShape).background(color).size(64.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = Color.White, modifier = Modifier.size(28.dp)) }
}

@Composable
private fun SettingsScreen(model: AppModel) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(4.dp)
    ) {
            FieldLabel("КАЧЕСТВО ВИДЕО-ЗВОНКА")
            Spacer(Modifier.height(4.dp))
            VideoQuality.presets.forEach { preset ->
                val selected = model.videoQuality == preset
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { model.changeVideoQuality(preset) }
                        .background(if (selected) HoloAccentDark else HoloBg)
                        .padding(horizontal = 12.dp, vertical = 14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    RadioButton(
                        selected = selected,
                        onClick = { model.changeVideoQuality(preset) },
                        colors = androidx.compose.material3.RadioButtonDefaults.colors(
                            selectedColor = HoloAccent,
                            unselectedColor = HoloSecondary,
                        )
                    )
                    Column {
                        Text(
                            preset.label,
                            color = if (selected) HoloAccent else Color.White,
                            fontSize = 15.sp,
                            fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                        )
                        Text(
                            "%.0f Мбит/с".format(preset.bitrate / 1_000_000.0),
                            color = HoloSecondary,
                            fontSize = 11.sp,
                        )
                    }
                }
                Divider(color = HoloBorder, thickness = 0.5.dp)
            }
            Spacer(Modifier.height(16.dp))
            Text(
                "Изменение применяется к следующему звонку.",
                color = HoloSecondary,
                fontSize = 11.sp,
            )
    }
}

// ── Экран «Сеть» (mesh) ─────────────────────────────────────────────────────

@Composable
private fun NetworkScreen(model: AppModel, onMyQr: () -> Unit) {
    val me = model.me ?: return
    val ctx = LocalContext.current
    // Офлайн-очередь = сообщения, ждущие появления получателя (статус "queued").
    val queued = model.messages.values.sumOf { l -> l.count { it.status == "queued" } }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // 1. Статус — ровно, в карточке.
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(10.dp).clip(CircleShape).background(HoloOnline))
                Spacer(Modifier.width(10.dp))
                Row {
                    Text("Подключено к ", color = Color.White, fontSize = 15.sp)
                    Text("mesh", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Bold)
                }
            }
        }

        // 2. Метрики. Пинг/трафик пока без нативного источника → «—».
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            MetricCard("Пиры", "${model.netPeers}", null, Modifier.weight(1f))
            MetricCard("Пинг", "—", "мс", Modifier.weight(1f))
            MetricCard("Трафик", "—", "КБ/с", Modifier.weight(1f))
        }

        // 3. Мой узел.
        FieldLabel("МОЙ УЗЕЛ")
        SurfaceCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(me.address, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace,
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(8.dp))
                Icon(Icons.Filled.ContentCopy, "Скопировать адрес", tint = HoloAccent,
                    modifier = Modifier.size(20.dp).clickable { copyToClipboard(ctx, me.address, "Адрес") })
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            OutlineAction("Мой QR", Icons.Filled.QrCode2, Modifier.weight(1f), onMyQr)
            OutlineAction("Поделиться", Icons.Filled.Share, Modifier.weight(1f)) { shareText(ctx, me.pubkey) }
        }

        // 4. Режим участия — выгоды + цена.
        FieldLabel("РЕЖИМ УЧАСТИЯ")
        SurfaceCard {
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Filled.Share, null, tint = HoloOnline, modifier = Modifier.size(26.dp))
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text("Узел сети", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                        Text("Ретранслирую трафик других — помогаю сети",
                            color = HoloSecondary, fontSize = 12.sp)
                    }
                    Spacer(Modifier.width(8.dp))
                    Switch(
                        checked = model.relayEnabled,
                        onCheckedChange = { model.relayEnabled = it },
                        colors = SwitchDefaults.colors(
                            checkedThumbColor = Color.White,
                            checkedTrackColor = HoloAccent,
                            uncheckedThumbColor = HoloSecondary,
                            uncheckedTrackColor = HoloBg,
                        ),
                    )
                }
                HorizontalDivider(color = HoloBorder, modifier = Modifier.padding(vertical = 12.dp))
                BenefitRow(Icons.Filled.Bolt, HoloOnline, "Приоритет доставки твоих сообщений")
                Spacer(Modifier.height(10.dp))
                BenefitRow(Icons.Filled.Schedule, HoloOnline, "Дольше хранится офлайн-очередь")
                Spacer(Modifier.height(10.dp))
                BenefitRow(Icons.Filled.BatteryStd, HoloSecondary, "Цена: выше расход батареи и трафика")
            }
        }

        // 5. Пиры. Списка адресов движок пока не отдаёт (только число) → заглушка.
        Row(verticalAlignment = Alignment.CenterVertically) {
            FieldLabel("ПИРЫ · ${model.netPeers}")
            Spacer(Modifier.weight(1f))
            Text("Все", color = HoloAccent, fontSize = 13.sp)
        }
        SurfaceCard {
            Text("Список адресов пиров — скоро", color = HoloSecondary, fontSize = 13.sp,
                modifier = Modifier.padding(vertical = 4.dp))
        }

        // 6. Офлайн-очередь.
        SurfaceCard(onClick = { /* TODO: экран очереди */ }) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Schedule, null, tint = HoloAccent, modifier = Modifier.size(26.dp))
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text("Офлайн-очередь · $queued", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    Text("доставится, когда получатель появится", color = HoloSecondary, fontSize = 12.sp)
                }
                Icon(Icons.Filled.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            }
        }
    }
}

/** Карточка-контейнер (тёмная поверхность, скругление) для разделов Сети. */
@Composable
private fun SurfaceCard(onClick: (() -> Unit)? = null, content: @Composable () -> Unit) {
    val base = Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(HoloSurface)
    Box((if (onClick != null) base.clickable { onClick() } else base).padding(14.dp)) { content() }
}

/** Метрика: подпись + крупное значение (+ опц. единица). */
@Composable
private fun MetricCard(label: String, value: String, unit: String?, modifier: Modifier = Modifier) {
    Box(modifier.clip(RoundedCornerShape(14.dp)).background(HoloSurface).padding(14.dp)) {
        Column {
            Text(label, color = HoloSecondary, fontSize = 11.sp)
            Spacer(Modifier.height(6.dp))
            Row(verticalAlignment = Alignment.Bottom) {
                Text(value, color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.Medium)
                if (unit != null) {
                    Spacer(Modifier.width(3.dp))
                    Text(unit, color = HoloSecondary, fontSize = 11.sp, modifier = Modifier.padding(bottom = 3.dp))
                }
            }
        }
    }
}

/** Кнопка-контур с иконкой (Мой QR / Поделиться). */
@Composable
private fun OutlineAction(text: String, icon: ImageVector, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Row(
        modifier.clip(RoundedCornerShape(12.dp)).border(1.dp, HoloBorder, RoundedCornerShape(12.dp))
            .clickable { onClick() }.padding(vertical = 12.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text(text, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

/** Строка «выгода/цена» режима участия. */
@Composable
private fun BenefitRow(icon: ImageVector, tint: Color, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, color = if (tint == HoloSecondary) HoloSecondary else Color.White, fontSize = 13.sp)
    }
}

private fun copyToClipboard(ctx: android.content.Context, text: String, label: String) {
    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
    cm.setPrimaryClip(android.content.ClipData.newPlainText(label, text))
    if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
        android.widget.Toast.makeText(ctx, "Скопировано", android.widget.Toast.LENGTH_SHORT).show()
    }
}

private fun shareText(ctx: android.content.Context, text: String) {
    val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(android.content.Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(android.content.Intent.createChooser(send, "Поделиться ключом"))
}

/** Встроенный Compose-сканер QR-кода на CameraX. Вызывает [onResult] ровно один раз. */
@Composable
private fun QrScannerScreen(onResult: (String) -> Unit) {
    val ctx = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    val previewView = remember { PreviewView(ctx) }
    var cameraProvider: ProcessCameraProvider? = remember { null }

    LaunchedEffect(lifecycleOwner) {
        cameraProvider = withContext(Dispatchers.IO) {
            ProcessCameraProvider.getInstance(ctx).get()
        }
        val preview = Preview.Builder().build().also {
            it.setSurfaceProvider(previewView.surfaceProvider)
        }
        val analysis = ImageAnalysis.Builder()
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()
        analysis.setAnalyzer(
            androidx.core.content.ContextCompat.getMainExecutor(ctx),
            QrAnalyzer(onResult),
        )
        cameraProvider?.unbindAll()
        cameraProvider?.bindToLifecycle(
            lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis,
        )
    }

    DisposableEffect(Unit) {
        onDispose { cameraProvider?.unbindAll() }
    }

    Box(Modifier.fillMaxSize()) {
        // Превью камеры
        AndroidView(factory = { previewView }, modifier = Modifier.fillMaxSize())

        // Тёмная маска: 4 прямоугольника вокруг прозрачного квадрата-прицела
        val boxSizePx = with(androidx.compose.ui.platform.LocalDensity.current) { 260.dp.toPx() }
        Canvas(Modifier.fillMaxSize()) {
            val l = (size.width - boxSizePx) / 2
            val t = (size.height - boxSizePx) / 2
            val r = l + boxSizePx
            val b = t + boxSizePx
            val dim = Color(0x99000000)
            drawRect(dim, topLeft = Offset.Zero, size = Size(size.width, t))          // верх
            drawRect(dim, topLeft = Offset(0f, t), size = Size(l, boxSizePx))         // лево
            drawRect(dim, topLeft = Offset(r, t), size = Size(size.width - r, boxSizePx)) // право
            drawRect(dim, topLeft = Offset(0f, b), size = Size(size.width, size.height - b)) // низ
        }

        // Рамка прицела
        Box(
            Modifier.size(260.dp).align(Alignment.Center)
                .border(2.dp, HoloAccent, RoundedCornerShape(12.dp)),
        )

        // Подсказка
        Text(
            "Наведите камеру на QR-код",
            color = Color.White,
            fontSize = 14.sp,
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 56.dp),
        )
    }
}

/** Анализатор кадров: ищет QR-код и вызывает [onResult] один раз при успехе. */
private class QrAnalyzer(private val onResult: (String) -> Unit) : ImageAnalysis.Analyzer {
    private val reader = com.google.zxing.qrcode.QRCodeReader()
    @Volatile private var done = false

    @ExperimentalGetImage
    override fun analyze(proxy: ImageProxy) {
        if (done) { proxy.close(); return }
        val img = proxy.image
        if (img != null) {
            try {
                val plane = img.planes[0]
                val buf = plane.buffer
                val data = ByteArray(buf.remaining()).also { buf.get(it) }
                val src = com.google.zxing.PlanarYUVLuminanceSource(
                    data, proxy.width, proxy.height, 0, 0, proxy.width, proxy.height, false,
                )
                val result = reader.decode(com.google.zxing.BinaryBitmap(
                    com.google.zxing.common.HybridBinarizer(src),
                ))
                done = true
                onResult(result.text)
            } catch (_: com.google.zxing.NotFoundException) {
                // нет QR в этом кадре — норма
            }
        }
        proxy.close()
    }
}

/** Папки = умные фильтры (Discord-рейл иконок). key→(иконка, подпись). */
private val Folders = listOf(
    Triple("all", Icons.Filled.Forum, "Все"),
    Triple("dm", Icons.AutoMirrored.Filled.Chat, "Чаты"),
    Triple("room", Icons.Filled.Headset, "Комнаты"),
    Triple("channel", Icons.Filled.Campaign, "Каналы"),
)

private fun folderLabel(key: String): String = Folders.firstOrNull { it.first == key }?.third ?: "Чаты"

/** Рейл папок-иконок (Discord-стиль): активная подсвечена, бейдж непрочитанного
 *  в правом-нижнем углу, `+` создаёт папку (пока заглушка). */
@Composable
private fun FolderRail(model: AppModel, activeFilter: String, onFilter: (String) -> Unit) {
    val dmUnread = model.unread.values.sum()
    val roomUnread = model.groupPresence.values.sumOf { it.size }
    fun badgeFor(key: String): Int = when (key) {
        "all" -> dmUnread
        "dm" -> dmUnread
        "room" -> roomUnread
        else -> 0
    }
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
            .background(HoloBg).padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Folders.forEach { (key, icon, label) ->
            FolderTile(icon, label, key == activeFilter, badgeFor(key)) { onFilter(key) }
        }
        FolderTile(Icons.Filled.Add, "Создать папку", false, 0) { /* TODO: создать папку */ }
    }
    HorizontalDivider(thickness = 1.dp, color = HoloBorder)
}

@Composable
private fun FolderTile(icon: ImageVector, label: String, selected: Boolean, badge: Int, onClick: () -> Unit) {
    Box(Modifier.size(48.dp).clickable { onClick() }, contentAlignment = Alignment.BottomEnd) {
        Box(
            Modifier.size(48.dp).clip(RoundedCornerShape(15.dp))
                .background(if (selected) FolderSelected else HoloSurface),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, label, tint = if (selected) HoloAccent else HoloSecondary,
                modifier = Modifier.size(24.dp))
        }
        if (badge > 0) {
            Box(
                Modifier.clip(CircleShape).background(HoloAccent).border(2.dp, HoloBg, CircleShape)
                    .defaultMinSize(minWidth = 18.dp, minHeight = 18.dp)
                    .padding(horizontal = 4.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(if (badge > 99) "99+" else "$badge", color = HoloAccentDark,
                    fontSize = 10.sp, fontWeight = FontWeight.Medium)
            }
        }
    }
}

/** Левое меню (Telegram-style): шапка-профиль «Вы» (тап → ключ/QR) + пункты
 *  Контакты·Звонки·Сеть·Сохранённое·Настройки + версия. */
@Composable
private fun AppDrawer(model: AppModel, navigate: (Screen) -> Unit) {
    val me = model.me
    ModalDrawerSheet(
        drawerContainerColor = HoloSurface,
        modifier = Modifier.widthIn(max = 300.dp),
    ) {
        // Шапка-профиль: квадратный аватар + «Вы»; тап → мой ключ/QR/профиль.
        Row(
            Modifier.fillMaxWidth().background(HoloAccentDark).clickable { navigate(Screen.Me) }
                .padding(20.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SquareAvatar(me?.pubkey ?: "?", size = 52.dp)
            Spacer(Modifier.width(14.dp))
            Text("Вы", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Medium,
                modifier = Modifier.weight(1f))
            Icon(Icons.Filled.QrCode2, "Мой ключ и QR", tint = HoloAccent, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.height(8.dp))
        // Основные разделы.
        listOf(
            Triple(Icons.Filled.Group, "Контакты", Screen.ContactList),
            Triple(Icons.Filled.Call, "Звонки", Screen.Calls),
            Triple(Icons.Filled.Share, "Сеть", Screen.Network),
            Triple(Icons.Filled.Lock, "Сохранённое", Screen.Saved),
        ).forEach { (icon, label, dest) -> DrawerItem(icon, label) { navigate(dest) } }
        HorizontalDivider(color = HoloBorder, modifier = Modifier.padding(vertical = 4.dp))
        DrawerItem(Icons.Filled.Settings, "Настройки") { navigate(Screen.Settings) }
        Spacer(Modifier.weight(1f))
        HorizontalDivider(color = HoloBorder)
        Text("p2p messenger · v0.3", color = HoloSecondary, fontSize = 11.sp,
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 14.dp))
    }
}

/** Ровный ряд левого меню (иконка + подпись, без инлайн-метрик). */
@Composable
private fun DrawerItem(icon: ImageVector, label: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = 20.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(18.dp))
        Text(label, color = Color.White, fontSize = 15.sp)
    }
}

/** Экран-заглушка для разделов, ещё не реализованных. */
@Composable
private fun ComingSoon(icon: ImageVector, title: String) {
    Box(Modifier.fillMaxSize().background(HoloBg), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(icon, null, tint = HoloBorder, modifier = Modifier.size(56.dp))
            Spacer(Modifier.height(14.dp))
            Text(title, color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(4.dp))
            Text("Скоро", color = HoloSecondary, fontSize = 13.sp)
        }
    }
}

/** Детерминированный цвет аватара по имени контакта. */
private fun avatarColor(name: String): Color {
    val palette = listOf(
        Color(0xFF1E3A5A), Color(0xFF2A1A3A), Color(0xFF1A2A1A),
        Color(0xFF2A2A1A), Color(0xFF3A1A1A), Color(0xFF1A3A3A),
    )
    return palette[abs(name.hashCode()) % palette.size]
}

/** Генерирует QR-код для строки [data] размером 512×512. Возвращает null при ошибке. */
private fun generateQrBitmap(data: String): android.graphics.Bitmap? = runCatching {
    val size = 512
    val bits = com.google.zxing.qrcode.QRCodeWriter()
        .encode(data, com.google.zxing.BarcodeFormat.QR_CODE, size, size)
    val pixels = IntArray(size * size) { i ->
        if (bits[i % size, i / size]) android.graphics.Color.BLACK else android.graphics.Color.WHITE
    }
    android.graphics.Bitmap.createBitmap(size, size, android.graphics.Bitmap.Config.ARGB_8888)
        .also { it.setPixels(pixels, 0, size, 0, 0, size, size) }
}.getOrNull()

@Composable
private fun inputColors() = TextFieldDefaults.colors(
    focusedContainerColor = HoloBg,
    unfocusedContainerColor = HoloBg,
    focusedTextColor = Color.White,
    unfocusedTextColor = Color.White,
    cursorColor = HoloAccent,
    focusedIndicatorColor = HoloAccent,
    unfocusedIndicatorColor = HoloBorder,
)
