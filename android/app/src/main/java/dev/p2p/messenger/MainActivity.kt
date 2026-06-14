package dev.p2p.messenger

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    private val model = AppModel()
    private val perms =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model.audio = CallAudio(applicationContext)
        model.appContext = applicationContext
        val need = listOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.CAMERA)
            .filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (need.isNotEmpty()) perms.launch(need.toTypedArray())
        val dataDir = filesDir.absolutePath
        setContent { P2PTheme { App(model, dataDir) } }
    }
}

@Composable
fun App(model: AppModel, dataDir: String) {
    var booted by remember { mutableStateOf(false) }

    // Старт ядра (сеть) на IO; затем — насос событий и медленный тикер статуса.
    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) { model.start(dataDir) }
        booted = true
    }
    LaunchedEffect(booted) {
        if (!booted) return@LaunchedEffect
        while (true) {
            var e = Native.pollEvent()
            while (e.isNotEmpty()) {
                model.applyEvent(e)
                e = Native.pollEvent()
            }
            delay(120)
        }
    }
    LaunchedEffect(booted) {
        if (!booted) return@LaunchedEffect
        while (true) {
            model.netPeers = Native.netPeers()
            model.contacts.forEach { model.online[it.key] = Native.isOnline(it.key) }
            delay(3000)
        }
    }

    when {
        model.error != null -> CenterMessage("Ошибка: ${model.error}")
        !booted || model.me == null -> CenterMessage("Подключение к сети…")
        else -> Root(model)
    }
}

@Composable
private fun CenterMessage(text: String) {
    Box(Modifier.fillMaxSize().background(HoloBg), contentAlignment = Alignment.Center) {
        Text(text, color = HoloSecondary)
    }
}

private enum class Screen { Contacts, Chat, Add, Me, Info }

@Composable
private fun Root(model: AppModel) {
    var screen by remember { mutableStateOf(Screen.Contacts) }

    Box(Modifier.fillMaxSize()) {
        Surface(Modifier.fillMaxSize(), color = HoloBg) {
            when (screen) {
                Screen.Contacts -> ContactsScreen(model,
                    onOpen = { model.openChat(it); screen = Screen.Chat },
                    onAdd = { screen = Screen.Add },
                    onMe = { screen = Screen.Me })
                Screen.Chat -> ChatScreen(model,
                    onBack = { screen = Screen.Contacts },
                    onInfo = { screen = Screen.Info },
                    onCall = { key, video -> model.startOutgoing(key, video) })
                Screen.Add -> AddScreen(model, onDone = { screen = Screen.Contacts })
                Screen.Me -> MeScreen(model, onBack = { screen = Screen.Contacts })
                Screen.Info -> InfoScreen(model,
                    onBack = { screen = Screen.Chat },
                    onDeleted = { screen = Screen.Contacts })
            }
        }
        // Звонок — поверх всего.
        model.call?.let { CallOverlay(model, it) }
    }
}

@Composable
private fun TopBar(title: String, onBack: (() -> Unit)? = null, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            Text("‹", color = HoloAccent, fontSize = 26.sp, modifier = Modifier
                .clickable { onBack() }.padding(end = 12.dp))
        }
        Text(title, color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        actions()
    }
}

@Composable
private fun ContactsScreen(model: AppModel, onOpen: (String) -> Unit, onAdd: () -> Unit, onMe: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        TopBar("p2p messenger") {
            Text("⊕", color = HoloAccent, fontSize = 22.sp, modifier = Modifier.clickable { onAdd() }.padding(8.dp))
            Text("⚿", color = HoloAccent, fontSize = 20.sp, modifier = Modifier.clickable { onMe() }.padding(8.dp))
        }
        Text("Пиров в сети: ${model.netPeers}", color = HoloSecondary, fontSize = 11.sp,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        if (model.contacts.isEmpty()) {
            CenterMessage("Нет контактов. Добавьте по ключу (⊕).")
        } else {
            LazyColumn(Modifier.fillMaxSize()) {
                items(model.contacts, key = { it.key }) { c ->
                    ContactRow(c, model.online[c.key] == true, model.unread[c.key] ?: 0) { onOpen(c.key) }
                    HorizontalDivider(thickness = 1.dp, color = HoloBorder)
                }
            }
        }
    }
}

@Composable
private fun ContactRow(c: Contact, online: Boolean, unread: Int, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("●", color = if (online) HoloOnline else Color(0xFF666666), fontSize = 11.sp)
        Spacer(Modifier.width(12.dp))
        Text(c.name, color = Color.White, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (c.verified) {
            Spacer(Modifier.width(6.dp)); Text("✓", color = HoloVerified, fontSize = 13.sp)
        }
        Spacer(Modifier.weight(1f))
        if (unread > 0) {
            Box(Modifier.clip(CircleShape).background(HoloAccent).padding(horizontal = 7.dp, vertical = 2.dp)) {
                Text("$unread", color = HoloAccentDark, fontSize = 11.sp)
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
            Text("📞", fontSize = 18.sp, modifier = Modifier.clickable { onCall(key, false) }.padding(8.dp))
            Text("📹", fontSize = 18.sp, modifier = Modifier.clickable { onCall(key, true) }.padding(8.dp))
            Text("ⓘ", color = HoloAccent, fontSize = 20.sp, modifier = Modifier.clickable { onInfo() }.padding(8.dp))
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp), reverseLayout = false) {
            items(msgs ?: emptyList()) { m -> MessageBubble(m) }
        }
        Row(Modifier.fillMaxWidth().background(HoloSurface).padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
            TextField(
                value = input, onValueChange = { input = it },
                modifier = Modifier.weight(1f), placeholder = { Text("Сообщение…", color = HoloSecondary) },
                colors = inputColors(), maxLines = 4,
            )
            Spacer(Modifier.width(8.dp))
            Box(
                Modifier.clip(CircleShape).background(HoloAccent).size(44.dp)
                    .clickable { model.send(key, input); input = "" },
                contentAlignment = Alignment.Center,
            ) { Text("➤", color = HoloAccentDark, fontSize = 18.sp) }
        }
    }
}

@Composable
private fun MessageBubble(m: Message) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
        Column(
            Modifier.widthIn(max = 280.dp).clip(RoundedCornerShape(10.dp))
                .background(if (m.mine) HoloAccent else HoloSurface).padding(10.dp),
        ) {
            Text(m.text, color = if (m.mine) HoloAccentDark else Color.White, fontSize = 15.sp)
            if (m.mine) {
                val mark = when (m.status) {
                    "delivered" -> "✓✓"; "failed" -> "не доставлено"; "sent" -> "✓"; else -> ""
                }
                if (mark.isNotEmpty()) Text(mark, color = HoloAccentDark.copy(alpha = 0.7f), fontSize = 10.sp,
                    modifier = Modifier.align(Alignment.End))
            }
        }
    }
}

@Composable
private fun AddScreen(model: AppModel, onDone: () -> Unit) {
    var keyHex by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize()) {
        TopBar("Добавить контакт", onBack = onDone)
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("ПУБЛИЧНЫЙ КЛЮЧ (hex)", color = HoloSecondary, fontSize = 11.sp)
            TextField(keyHex, { keyHex = it }, Modifier.fillMaxWidth(), colors = inputColors(),
                placeholder = { Text("64 hex-символа", color = HoloSecondary) })
            Text("ИМЯ", color = HoloSecondary, fontSize = 11.sp)
            TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
            Button(onClick = {
                status = when (model.addContact(keyHex.trim(), name.trim())) {
                    "added" -> { onDone(); "" }
                    "known" -> "Контакт уже есть"
                    else -> "Неверный ключ"
                }
            }, colors = ButtonDefaults.buttonColors(containerColor = HoloAccent)) {
                Text("Добавить", color = HoloAccentDark)
            }
            if (status.isNotEmpty()) Text(status, color = HoloError)
        }
    }
}

@Composable
private fun MeScreen(model: AppModel, onBack: () -> Unit) {
    val me = model.me ?: return
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        TopBar("Мой код", onBack = onBack)
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("FINGERPRINT (сверьте при встрече)", color = HoloSecondary, fontSize = 11.sp)
            Text(me.fingerprint, color = HoloAccent, fontSize = 16.sp)
            Text("АДРЕС YGGDRASIL", color = HoloSecondary, fontSize = 11.sp)
            Text(me.address, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
            Text("ПУБЛИЧНЫЙ КЛЮЧ", color = HoloSecondary, fontSize = 11.sp)
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
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("ИМЯ", color = HoloSecondary, fontSize = 11.sp)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextField(name, { name = it }, Modifier.weight(1f), colors = inputColors())
                Spacer(Modifier.width(8.dp))
                Button(onClick = { model.rename(key, name) },
                    colors = ButtonDefaults.buttonColors(containerColor = HoloAccent)) {
                    Text("OK", color = HoloAccentDark)
                }
            }
            Text("FINGERPRINT", color = HoloSecondary, fontSize = 11.sp)
            Text(c.fingerprint, color = HoloAccent, fontSize = 15.sp)
            Text("Сверьте слова с собеседником по другому каналу — защита от подмены ключа.",
                color = HoloSecondary, fontSize = 11.sp)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(if (c.verified) "✓ Проверен" else "Не проверен",
                    color = if (c.verified) HoloVerified else Color(0xFFE6A23C),
                    modifier = Modifier.weight(1f))
                Button(onClick = { model.toggleVerified(key) },
                    colors = ButtonDefaults.buttonColors(containerColor = HoloAccent)) {
                    Text(if (c.verified) "Снять" else "Пометить проверенным", color = HoloAccentDark)
                }
            }
            Text("ПУБЛИЧНЫЙ КЛЮЧ", color = HoloSecondary, fontSize = 11.sp)
            Text(c.key, color = Color.White, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
            Button(onClick = { model.remove(key); onDeleted() },
                colors = ButtonDefaults.buttonColors(containerColor = HoloError)) {
                Text("Удалить контакт", color = Color.White)
            }
        }
    }
}

@Composable
private fun CallOverlay(model: AppModel, call: CallState) {
    val name = model.contacts.firstOrNull { it.key == call.peer }?.name ?: "Контакт"
    val statusText = when (call.status) {
        "incoming" -> "Входящий звонок"
        "ringing" -> "Звоним…"
        else -> "Идёт звонок"
    }
    Box(Modifier.fillMaxSize().background(Color(0xF2000000)), contentAlignment = Alignment.Center) {
        // Входящее видео (если видео-звонок активен) — на фоне.
        if (call.withVideo && call.status == "active") {
            VideoFrame(Modifier.fillMaxSize())
        }
        Column(
            Modifier.fillMaxSize().padding(bottom = 48.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Bottom,
        ) {
            if (!(call.withVideo && call.status == "active")) {
                Spacer(Modifier.weight(1f))
                Text(name, color = Color.White, fontSize = 26.sp, fontWeight = FontWeight.Medium)
                Text(statusText, color = HoloSecondary, fontSize = 15.sp)
                Spacer(Modifier.weight(1f))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(28.dp), verticalAlignment = Alignment.CenterVertically) {
                if (call.status == "incoming") {
                    RoundButton("✓", HoloOnline) { model.acceptCall() }
                    RoundButton("✕", HoloError) { model.hangUp() }
                } else {
                    if (call.status == "active") {
                        RoundButton(if (call.muted) "🔇" else "🎤", HoloSurface) { model.toggleMute() }
                    }
                    RoundButton("✕", HoloError) { model.hangUp() }
                }
            }
        }
    }
}

/** Отрисовка входящих видео-кадров: поллит [Native.callPullVideo] и рисует Bitmap. */
@Composable
private fun VideoFrame(modifier: Modifier = Modifier) {
    var bitmap by remember { mutableStateOf<android.graphics.Bitmap?>(null) }
    LaunchedEffect(Unit) {
        while (true) {
            val data = Native.callPullVideo()
            if (data.size > 8) {
                val w = (data[0].toInt() and 0xff) or ((data[1].toInt() and 0xff) shl 8) or
                    ((data[2].toInt() and 0xff) shl 16) or ((data[3].toInt() and 0xff) shl 24)
                val h = (data[4].toInt() and 0xff) or ((data[5].toInt() and 0xff) shl 8) or
                    ((data[6].toInt() and 0xff) shl 16) or ((data[7].toInt() and 0xff) shl 24)
                if (w in 1..4096 && h in 1..4096 && data.size >= 8 + w * h * 3) {
                    val px = IntArray(w * h)
                    var o = 8
                    for (i in 0 until w * h) {
                        val r = data[o++].toInt() and 0xff
                        val g = data[o++].toInt() and 0xff
                        val b = data[o++].toInt() and 0xff
                        px[i] = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
                    }
                    bitmap = android.graphics.Bitmap.createBitmap(px, w, h, android.graphics.Bitmap.Config.ARGB_8888)
                }
            }
            delay(40)
        }
    }
    bitmap?.let {
        androidx.compose.foundation.Image(
            bitmap = it.asImageBitmap(),
            contentDescription = null,
            modifier = modifier,
            contentScale = androidx.compose.ui.layout.ContentScale.Fit,
        )
    }
}

@Composable
private fun RoundButton(label: String, color: Color, onClick: () -> Unit) {
    Box(
        Modifier.clip(CircleShape).background(color).size(64.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Text(label, fontSize = 24.sp, color = Color.White) }
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
