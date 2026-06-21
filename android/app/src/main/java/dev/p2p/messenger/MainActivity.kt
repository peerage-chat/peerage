package dev.p2p.messenger

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

// ── Единая сетка размеров/отступов ──────────────────────────────────────────
private val ScreenPad = 16.dp        // отступ контента экрана
private val FormGap = 12.dp          // вертикальный зазор в формах
private val RowPadV = 14.dp          // вертикальный паддинг ряда списка
private val OfflineDot = Color(0xFF555555)
private val WarnColor = Color(0xFFE6A23C)

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
        // Постоянный mesh-узел: держим процесс/сеть живыми при погасшем экране.
        ConnService.start(applicationContext)
        val dataDir = filesDir.absolutePath
        setContent { P2PTheme { App(model, dataDir) } }
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

private enum class Screen { Contacts, Chat, Add, Me, Info, Groups, GroupChat, CreateGroup, AddMember, Settings }

@Composable
private fun Root(model: AppModel) {
    var screen by remember { mutableStateOf(Screen.Contacts) }

    Box(Modifier.fillMaxSize()) {
        Surface(Modifier.fillMaxSize(), color = HoloBg) {
            Column(Modifier.fillMaxSize()) {
                // ── Шапка: конфигурируется по текущему экрану ─────────────────────
                when (screen) {
                    Screen.Contacts -> TopBar("p2p messenger") {
                        BarIcon(Icons.Filled.Group, "Группы") { model.refreshGroups(); screen = Screen.Groups }
                        BarIcon(Icons.Filled.PersonAdd, "Добавить контакт") { screen = Screen.Add }
                        BarIcon(Icons.Filled.QrCode2, "Мой код") { screen = Screen.Me }
                        BarIcon(Icons.Filled.Settings, "Настройки") { screen = Screen.Settings }
                    }
                    Screen.Chat -> {
                        val key = model.currentChat
                        val name = key?.let { k -> model.contacts.firstOrNull { it.key == k }?.name } ?: "Чат"
                        TopBar(name, onBack = { screen = Screen.Contacts }) {
                            if (key != null) {
                                BarIcon(Icons.Filled.Call, "Аудио-звонок") { model.callDm(key, false) }
                                BarIcon(Icons.Filled.Videocam, "Видео-звонок") { model.callDm(key, true) }
                                BarIcon(Icons.Filled.Info, "О контакте") { screen = Screen.Info }
                            }
                        }
                    }
                    Screen.Add -> TopBar("Добавить контакт", onBack = { screen = Screen.Contacts })
                    Screen.Me -> TopBar("Мой код", onBack = { screen = Screen.Contacts })
                    Screen.Info -> TopBar("Контакт", onBack = { screen = Screen.Chat })
                    Screen.Groups -> TopBar("Группы", onBack = { screen = Screen.Contacts }) {
                        BarIcon(Icons.Filled.Add, "Новая группа") { screen = Screen.CreateGroup }
                    }
                    Screen.GroupChat -> {
                        val gid = model.currentGroup
                        val name = gid?.let { g -> model.groups.firstOrNull { it.gid == g }?.name } ?: "Группа"
                        TopBar(name, onBack = { screen = Screen.Groups }) {
                            if (gid != null) {
                                if (model.currentRoom == gid) {
                                    BarIcon(Icons.Filled.CallEnd, "Выйти из звонка") { model.leaveRoom() }
                                } else {
                                    BarIcon(Icons.Filled.Call, "Войти в звонок") { model.joinRoom(gid) }
                                }
                                BarIcon(Icons.Filled.GroupAdd, "Добавить участника") { screen = Screen.AddMember }
                            }
                        }
                    }
                    Screen.CreateGroup -> TopBar("Новая группа", onBack = { screen = Screen.Groups })
                    Screen.AddMember -> TopBar("Добавить участника", onBack = { screen = Screen.GroupChat })
                    Screen.Settings -> TopBar("Настройки", onBack = { screen = Screen.Contacts })
                }
                // ── DmCallBar: прямо под шапкой, толкает контент вниз ─────────────
                if (model.incomingCall == null) {
                    model.dmCallPeer?.let { DmCallBar(model, it) }
                }
                // ── Контент экрана (без шапки) ────────────────────────────────────
                Box(Modifier.weight(1f)) {
                    when (screen) {
                        Screen.Contacts -> ContactsScreen(model) { model.openChat(it); screen = Screen.Chat }
                        Screen.Chat -> ChatScreen(model)
                        Screen.Add -> AddScreen(model) { screen = Screen.Contacts }
                        Screen.Me -> MeScreen(model)
                        Screen.Info -> InfoScreen(model) { screen = Screen.Contacts }
                        Screen.Groups -> GroupsScreen(model) { model.openGroup(it); screen = Screen.GroupChat }
                        Screen.GroupChat -> GroupChatScreen(model)
                        Screen.CreateGroup -> CreateGroupScreen(model) { screen = Screen.Groups }
                        Screen.AddMember -> AddMemberScreen(model) { screen = Screen.GroupChat }
                        Screen.Settings -> SettingsScreen(model)
                    }
                }
            }
        }
        model.incomingCall?.let { IncomingCallOverlay(model, it) }
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
private fun TopBar(title: String, onBack: (() -> Unit)? = null, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            BarIcon(Icons.AutoMirrored.Filled.ArrowBack, "Назад", onClick = onBack)
        } else {
            Spacer(Modifier.width(12.dp))
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

@Composable
private fun ListRow(onClick: () -> Unit, content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = ScreenPad, vertical = RowPadV),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
    HorizontalDivider(thickness = 1.dp, color = HoloBorder)
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
private fun ContactsScreen(model: AppModel, onOpen: (String) -> Unit) {
    Column(Modifier.fillMaxSize()) {
        // Долгий тап → режим приёмника теста скорости (живой Мбит/с).
        val statusLabel = if (model.benchRecv) "⏱ приём: %.1f Мбит/с".format(model.benchRecvMbps)
        else "Пиров в сети: ${model.netPeers}"
        Text(statusLabel, color = HoloSecondary, fontSize = 11.sp,
            modifier = Modifier.padding(horizontal = ScreenPad, vertical = 6.dp)
                .pointerInput(Unit) { detectTapGestures(onLongPress = { model.startBenchRecv() }) })
        if (model.contacts.isEmpty()) {
            CenterMessage("Нет контактов. Добавьте по ключу.")
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(model.contacts, key = { it.key }) { c ->
                    ListRow(onClick = { onOpen(c.key) }) {
                        StatusDot(model.online[c.key] == true)
                        Spacer(Modifier.width(12.dp))
                        Text(c.name, color = Color.White, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (c.verified) {
                            Spacer(Modifier.width(6.dp))
                            Icon(Icons.Filled.Verified, "Проверен", tint = HoloVerified, modifier = Modifier.size(15.dp))
                        }
                        Spacer(Modifier.weight(1f))
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

@Composable
private fun ChatScreen(model: AppModel) {
    val key = model.currentChat ?: return
    val msgs = model.messages[key]
    var input by remember(key) { mutableStateOf("") }

    Column(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp)) {
            items(msgs ?: emptyList()) { m -> MessageBubble(m) }
        }
        MessageInput(input, { input = it }) { model.send(key, input); input = "" }
    }
}

@Composable
private fun MessageInput(value: String, onChange: (String) -> Unit, onSend: () -> Unit) {
    Row(Modifier.fillMaxWidth().background(HoloSurface).padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
        TextField(value, onChange, Modifier.weight(1f),
            placeholder = { Text("Сообщение…", color = HoloSecondary) }, colors = inputColors(), maxLines = 4)
        Spacer(Modifier.width(8.dp))
        SendButton(onSend)
    }
}

@Composable
private fun MessageBubble(m: Message) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
        Column(
            Modifier.widthIn(max = 280.dp).clip(RoundedCornerShape(10.dp))
                .background(if (m.mine) HoloAccent else HoloSurface).padding(horizontal = 10.dp, vertical = 7.dp),
        ) {
            Text(m.text, color = if (m.mine) HoloAccentDark else Color.White, fontSize = 15.sp)
            if (m.mine) {
                val tint = HoloAccentDark.copy(alpha = 0.7f)
                Row(Modifier.align(Alignment.End).padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                    when (m.status) {
                        "delivered" -> Icon(Icons.Filled.DoneAll, "доставлено", tint = tint, modifier = Modifier.size(13.dp))
                        // "sent" остаётся для сообщений из БД (сохранены старым кодом).
                        "sent" -> Icon(Icons.Filled.Done, "отправлено", tint = tint, modifier = Modifier.size(13.dp))
                        // "pending" — Rust пробует доставить, ACK ещё не пришёл.
                        "pending" -> Icon(Icons.Filled.Schedule, "отправляется", tint = tint.copy(alpha = 0.5f), modifier = Modifier.size(13.dp))
                        "failed" -> Text("не доставлено", color = HoloAccentDark.copy(alpha = 0.8f), fontSize = 10.sp)
                        "queued" -> Text("ожидание…", color = HoloAccentDark.copy(alpha = 0.7f), fontSize = 10.sp)
                    }
                }
            }
        }
    }
}

@Composable
private fun AddScreen(model: AppModel, onDone: () -> Unit) {
    var keyHex by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    // Операция асинхронна; результат приходит в model.addStatus.
    LaunchedEffect(Unit) { model.addStatus = "" }
    val st = model.addStatus
    LaunchedEffect(st) { if (st == "added") { model.addStatus = ""; onDone() } }
    val scanLauncher = rememberLauncherForActivityResult(ScanContract()) { result ->
        result.contents?.let { keyHex = it }
    }
    Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
        FieldLabel("ПУБЛИЧНЫЙ КЛЮЧ (hex)")
        TextField(keyHex, { keyHex = it }, Modifier.fillMaxWidth(), colors = inputColors(),
            placeholder = { Text("64 hex-символа", color = HoloSecondary) })
        AccentButton("Сканировать QR", modifier = Modifier.fillMaxWidth()) {
            scanLauncher.launch(ScanOptions().setBeepEnabled(false).setOrientationLocked(false))
        }
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
    val msgs = model.groupMessages[gid]
    var input by remember(gid) { mutableStateOf("") }
    var focusedParticipant by remember { mutableStateOf<String?>(null) }

    val inRoom = model.currentRoom == gid
    val presenceMembers = if (inRoom) emptyList() else (model.groupPresence[gid] ?: emptyList())
    val showGrid = inRoom || presenceMembers.isNotEmpty()

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            if (showGrid) RoomGrid(model, inRoom, presenceMembers, onFocus = { focusedParticipant = it })
            LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp)) {
                items(msgs ?: emptyList()) { m ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp),
                        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
                        Box(Modifier.widthIn(max = 280.dp).clip(RoundedCornerShape(10.dp))
                            .background(if (m.mine) HoloAccent else HoloSurface).padding(horizontal = 10.dp, vertical = 7.dp)) {
                            Text(m.text, color = if (m.mine) HoloAccentDark else Color.White, fontSize = 15.sp)
                        }
                    }
                }
            }
            MessageInput(input, { input = it }) { model.sendGroupText(gid, input); input = "" }
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
        Column(Modifier.padding(horizontal = 8.dp, vertical = 8.dp)) {
            Text(
                if (inRoom) "В ЗВОНКЕ · $total" else "В КОМНАТЕ · $total",
                color = HoloSecondary, fontSize = 11.sp,
                modifier = Modifier.padding(start = 4.dp, bottom = 6.dp),
            )
            val all = if (inRoom)
                listOf(Triple("me", true, false)) + members.map { Triple(it.peer, it.audio, it.video) }
            else
                members.map { Triple(it.peer, it.audio, it.video) }
            all.chunked(2).forEach { row ->
                Row(
                    Modifier.fillMaxWidth().padding(bottom = 6.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
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
    Row(
        modifier
            .clip(RoundedCornerShape(8.dp))
            .background(if (me) HoloAccentDark else Color(0xFF2A2F35))
            .clickable { onClick() }
            .padding(horizontal = 10.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(40.dp).clip(CircleShape)
                .background(if (me) HoloAccent else Color(0xFF3A3F45)),
            contentAlignment = Alignment.Center,
        ) {
            Text(name.take(1).uppercase(), color = if (me) HoloAccentDark else Color.White,
                fontWeight = FontWeight.Bold, fontSize = 16.sp)
        }
        Spacer(Modifier.width(8.dp))
        Column(Modifier.weight(1f)) {
            Text(name, color = if (me) HoloAccent else Color.White,
                fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (!audio || video) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (!audio) Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(11.dp))
                    if (video) Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(11.dp))
                }
            }
        }
    }
}

@Composable
private fun ParticipantFullscreen(
    name: String, audio: Boolean, video: Boolean, me: Boolean, onDismiss: () -> Unit,
) {
    Box(
        Modifier.fillMaxSize().background(Color(0xEE000000)).clickable { onDismiss() },
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Box(
                Modifier.size(120.dp).clip(CircleShape)
                    .background(if (me) HoloAccent else Color(0xFF3A3F45)),
                contentAlignment = Alignment.Center,
            ) {
                Text(name.take(1).uppercase(), color = if (me) HoloAccentDark else Color.White,
                    fontWeight = FontWeight.Bold, fontSize = 52.sp)
            }
            Spacer(Modifier.height(20.dp))
            Text(name, color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(10.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
                if (!audio) Row(verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    Icon(Icons.Filled.MicOff, null, tint = WarnColor, modifier = Modifier.size(15.dp))
                    Text("Выкл", color = WarnColor, fontSize = 13.sp)
                }
                if (video) Row(verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    Icon(Icons.Filled.Videocam, null, tint = HoloSecondary, modifier = Modifier.size(15.dp))
                    Text("Видео", color = HoloSecondary, fontSize = 13.sp)
                }
            }
            Spacer(Modifier.height(36.dp))
            Text("Нажмите чтобы закрыть", color = HoloSecondary, fontSize = 11.sp)
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

/** Мини-полоса активного DM-звонка — встраивается в Column Root между шапкой и контентом. */
@Composable
private fun DmCallBar(model: AppModel, peerKey: String) {
    val name = model.contacts.firstOrNull { it.key == peerKey }?.name ?: (peerKey.take(6) + "…")
    val ringing = model.roomMembers.isEmpty()
    Surface(color = Color(0xFF1B2E1B), modifier = Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Filled.Call, null, tint = HoloOnline, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(name, color = Color.White, fontSize = 14.sp)
                Text(if (ringing) "Вызов…" else "Идёт звонок",
                    color = HoloSecondary, fontSize = 11.sp)
            }
            IconButton(onClick = { model.leaveRoom() }) {
                Icon(Icons.Filled.CallEnd, "Завершить", tint = HoloError,
                    modifier = Modifier.size(22.dp))
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
