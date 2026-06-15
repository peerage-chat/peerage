package dev.p2p.messenger

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
        model.audio = CallAudio(applicationContext)
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

private enum class Screen { Contacts, Chat, Add, Me, Info, Groups, GroupChat, CreateGroup, AddMember }

@Composable
private fun Root(model: AppModel) {
    var screen by remember { mutableStateOf(Screen.Contacts) }

    Box(Modifier.fillMaxSize()) {
        Surface(Modifier.fillMaxSize(), color = HoloBg) {
            when (screen) {
                Screen.Contacts -> ContactsScreen(model,
                    onOpen = { model.openChat(it); screen = Screen.Chat },
                    onAdd = { screen = Screen.Add },
                    onMe = { screen = Screen.Me },
                    onGroups = { model.refreshGroups(); screen = Screen.Groups })
                Screen.Chat -> ChatScreen(model,
                    onBack = { screen = Screen.Contacts },
                    onInfo = { screen = Screen.Info },
                    onCall = { key, video -> model.startOutgoing(key, video) })
                Screen.Add -> AddScreen(model, onDone = { screen = Screen.Contacts })
                Screen.Me -> MeScreen(model, onBack = { screen = Screen.Contacts })
                Screen.Info -> InfoScreen(model,
                    onBack = { screen = Screen.Chat },
                    onDeleted = { screen = Screen.Contacts })
                Screen.Groups -> GroupsScreen(model,
                    onBack = { screen = Screen.Contacts },
                    onOpen = { model.openGroup(it); screen = Screen.GroupChat },
                    onCreate = { screen = Screen.CreateGroup })
                Screen.GroupChat -> GroupChatScreen(model,
                    onBack = { screen = Screen.Groups },
                    onAddMember = { screen = Screen.AddMember })
                Screen.CreateGroup -> CreateGroupScreen(model, onDone = { screen = Screen.Groups })
                Screen.AddMember -> AddMemberScreen(model, onDone = { screen = Screen.GroupChat })
            }
        }
        model.call?.let { CallOverlay(model, it) }
    }
}

// ── Общие элементы ──────────────────────────────────────────────────────────

/** Иконка-кнопка топ-бара: единый размер (48dp касание / 24dp иконка). */
@Composable
private fun BarIcon(icon: ImageVector, desc: String, onClick: () -> Unit, tint: Color = HoloAccent) {
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
            BarIcon(Icons.AutoMirrored.Filled.ArrowBack, "Назад", onBack)
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
private fun ContactsScreen(model: AppModel, onOpen: (String) -> Unit, onAdd: () -> Unit, onMe: () -> Unit, onGroups: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        TopBar("p2p messenger") {
            BarIcon(Icons.Filled.Group, "Группы", onGroups)
            BarIcon(Icons.Filled.PersonAdd, "Добавить контакт", onAdd)
            BarIcon(Icons.Filled.QrCode2, "Мой код", onMe)
        }
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
private fun ChatScreen(model: AppModel, onBack: () -> Unit, onInfo: () -> Unit, onCall: (String, Boolean) -> Unit) {
    val key = model.currentChat ?: return
    val contact = model.contacts.firstOrNull { it.key == key }
    val msgs = model.messages[key]
    var input by remember(key) { mutableStateOf("") }

    Column(Modifier.fillMaxSize()) {
        TopBar(contact?.name ?: "Чат", onBack = onBack) {
            BarIcon(Icons.Filled.Call, "Аудио-звонок", { onCall(key, false) })
            BarIcon(Icons.Filled.Videocam, "Видео-звонок", { onCall(key, true) })
            BarIcon(Icons.Filled.Info, "О контакте", onInfo)
        }
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
                        "sent" -> Icon(Icons.Filled.Done, "отправлено", tint = tint, modifier = Modifier.size(13.dp))
                        "failed" -> Text("не доставлено", color = HoloAccentDark.copy(alpha = 0.8f), fontSize = 10.sp)
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
    Column(Modifier.fillMaxSize()) {
        TopBar("Добавить контакт", onBack = onDone)
        Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
            FieldLabel("ПУБЛИЧНЫЙ КЛЮЧ (hex)")
            TextField(keyHex, { keyHex = it }, Modifier.fillMaxWidth(), colors = inputColors(),
                placeholder = { Text("64 hex-символа", color = HoloSecondary) })
            FieldLabel("ИМЯ")
            TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
            AccentButton("Добавить") { model.addContact(keyHex.trim(), name.trim()) }
            val msg = when (st) { "known" -> "Контакт уже есть"; "error" -> "Неверный ключ"; else -> "" }
            if (msg.isNotEmpty()) Text(msg, color = HoloError, fontSize = 13.sp)
        }
    }
}

@Composable
private fun MeScreen(model: AppModel, onBack: () -> Unit) {
    val me = model.me ?: return
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        TopBar("Мой код", onBack = onBack)
        Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
            FieldLabel("FINGERPRINT (сверьте при встрече)")
            Text(me.fingerprint, color = HoloAccent, fontSize = 16.sp)
            FieldLabel("АДРЕС YGGDRASIL")
            Text(me.address, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
            FieldLabel("ПУБЛИЧНЫЙ КЛЮЧ")
            Text(me.pubkey, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
        }
    }
}

@Composable
private fun InfoScreen(model: AppModel, onBack: () -> Unit, onDeleted: () -> Unit) {
    val key = model.currentChat ?: return
    val c = model.contacts.firstOrNull { it.key == key } ?: return
    var name by remember(key) { mutableStateOf(c.name) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        TopBar("Контакт", onBack = onBack)
        Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
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
}

@Composable
private fun GroupsScreen(model: AppModel, onBack: () -> Unit, onOpen: (String) -> Unit, onCreate: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        TopBar("Группы", onBack = onBack) {
            BarIcon(Icons.Filled.Add, "Новая группа", onCreate)
        }
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
}

@Composable
private fun GroupChatScreen(model: AppModel, onBack: () -> Unit, onAddMember: () -> Unit) {
    val gid = model.currentGroup ?: return
    val group = model.groups.firstOrNull { it.gid == gid }
    val msgs = model.groupMessages[gid]
    var input by remember(gid) { mutableStateOf("") }

    val inRoom = model.currentRoom == gid

    Column(Modifier.fillMaxSize()) {
        TopBar(group?.name ?: "Группа", onBack = onBack) {
            if (inRoom) {
                BarIcon(Icons.Filled.CallEnd, "Выйти из звонка", { model.leaveRoom() })
            } else {
                BarIcon(Icons.Filled.Call, "Войти в звонок", { model.joinRoom(gid) })
            }
            BarIcon(Icons.Filled.GroupAdd, "Добавить участника", onAddMember)
        }
        if (inRoom) RoomBar(model)
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
}

/** Полоса «кто в звонке» (зачаток сетки участников H1.2). */
@Composable
private fun RoomBar(model: AppModel) {
    val members = model.roomMembers
    Surface(color = HoloSurface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
            Text("В ЗВОНКЕ · ${members.size + 1}", color = HoloSecondary, fontSize = 11.sp)
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(top = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                RoomTile("вы", audio = true, video = false, me = true)
                members.forEach { m ->
                    val name = model.contacts.firstOrNull { it.key == m.peer }?.name ?: (m.peer.take(6) + "…")
                    RoomTile(name, m.audio, m.video, me = false)
                }
            }
        }
    }
}

@Composable
private fun RoomTile(name: String, audio: Boolean, video: Boolean, me: Boolean) {
    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.width(64.dp)) {
        Box(
            Modifier.size(52.dp).clip(CircleShape)
                .background(if (me) HoloAccent else Color(0xFF3A3F45)),
            contentAlignment = Alignment.Center,
        ) {
            Text(name.take(1).uppercase(), color = if (me) HoloAccentDark else Color.White,
                fontWeight = FontWeight.Bold, fontSize = 18.sp)
        }
        Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            if (!audio) Icon(Icons.Filled.MicOff, "микрофон выкл", tint = WarnColor, modifier = Modifier.size(12.dp))
            if (video) Icon(Icons.Filled.Videocam, "видео", tint = HoloSecondary, modifier = Modifier.size(12.dp))
        }
        Text(name, fontSize = 10.sp, color = HoloSecondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun CreateGroupScreen(model: AppModel, onDone: () -> Unit) {
    var name by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize()) {
        TopBar("Новая группа", onBack = onDone)
        Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
            FieldLabel("НАЗВАНИЕ")
            TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
            AccentButton("Создать") { model.createGroup(name); onDone() }
            Text("Вы станете администратором. Добавляйте участников из контактов внутри группы.",
                color = HoloSecondary, fontSize = 11.sp)
        }
    }
}

@Composable
private fun AddMemberScreen(model: AppModel, onDone: () -> Unit) {
    val gid = model.currentGroup ?: return
    Column(Modifier.fillMaxSize()) {
        TopBar("Добавить участника", onBack = onDone)
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
}

// ── Оверлей звонка ──────────────────────────────────────────────────────────

@Composable
private fun CallOverlay(model: AppModel, call: CallState) {
    val name = model.contacts.firstOrNull { it.key == call.peer }?.name ?: "Контакт"
    val statusText = when (call.status) {
        "incoming" -> "Входящий звонок"
        "ringing" -> "Звоним…"
        else -> "Идёт звонок"
    }
    val showVideo = call.withVideo && call.status == "active"
    Box(Modifier.fillMaxSize().background(Color(0xF2000000))) {
        if (showVideo) VideoSurface(Modifier.fillMaxSize())
        Column(
            Modifier.fillMaxSize().padding(bottom = 48.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Bottom,
        ) {
            if (!showVideo) {
                Spacer(Modifier.weight(1f))
                Text(name, color = Color.White, fontSize = 26.sp, fontWeight = FontWeight.Medium)
                Text(statusText, color = HoloSecondary, fontSize = 15.sp, modifier = Modifier.padding(top = 6.dp))
                Spacer(Modifier.weight(1f))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(28.dp), verticalAlignment = Alignment.CenterVertically) {
                if (call.status == "incoming") {
                    CallButton(Icons.Filled.Call, "Принять", HoloOnline) { model.acceptCall() }
                    CallButton(Icons.Filled.CallEnd, "Отклонить", HoloError) { model.hangUp() }
                } else {
                    if (call.status == "active") {
                        CallButton(if (call.muted) Icons.Filled.MicOff else Icons.Filled.Mic, "Микрофон", HoloSurface) { model.toggleMute() }
                    }
                    CallButton(Icons.Filled.CallEnd, "Завершить", HoloError) { model.hangUp() }
                }
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

/** Входящее видео: SurfaceView, в который аппаратный декодер рендерит напрямую. */
@Composable
private fun VideoSurface(modifier: Modifier = Modifier) {
    androidx.compose.ui.viewinterop.AndroidView(
        modifier = modifier,
        factory = { ctx ->
            android.view.SurfaceView(ctx).apply {
                holder.addCallback(object : android.view.SurfaceHolder.Callback {
                    @Volatile private var receiver: VideoReceiver? = null
                    override fun surfaceCreated(h: android.view.SurfaceHolder) {
                        // Создание/старт MediaCodec-декодера блокирует — не на UI-потоке.
                        Thread {
                            receiver = VideoReceiver(h.surface, VIDEO_W, VIDEO_H).also { it.start() }
                        }.start()
                    }
                    override fun surfaceChanged(h: android.view.SurfaceHolder, f: Int, w: Int, ht: Int) {}
                    override fun surfaceDestroyed(h: android.view.SurfaceHolder) {
                        val r = receiver; receiver = null
                        Thread { r?.stop() }.start() // MediaCodec.release блокирует
                    }
                })
            }
        },
    )
}

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
