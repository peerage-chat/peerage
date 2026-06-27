@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

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
import dev.p2p.messenger.*

@Composable
internal fun ChatTopBar(model: AppModel, onBack: () -> Unit, onProfile: () -> Unit) {
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
internal fun ChatScreen(model: AppModel) {
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
internal fun E2EBanner() {
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
internal fun DateChip(label: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.Center) {
        Box(
            Modifier.clip(RoundedCornerShape(10.dp)).background(HoloSurface)
                .padding(horizontal = 10.dp, vertical = 3.dp),
        ) { Text(label, color = HoloSecondary, fontSize = 11.sp) }
    }
}

@Composable
internal fun MessageInput(value: String, onChange: (String) -> Unit, onSend: () -> Unit) {
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
internal fun PillIcon(icon: ImageVector, desc: String, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(CircleShape).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(icon, desc, tint = HoloSecondary, modifier = Modifier.size(22.dp)) }
}

@Composable
internal fun MessageBubble(m: Message) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp),
        horizontalArrangement = if (m.mine) Arrangement.End else Arrangement.Start) {
        Box(
            Modifier.widthIn(max = 290.dp).clip(RoundedCornerShape(14.dp))
                .background(if (m.mine) BubbleOut else BubbleIn)
                .padding(horizontal = 10.dp, vertical = 6.dp),
        ) {
            // Текст резервирует справа место под время+галочку (реальный отступ,
            // не пробелы) → время оверлеем точно в правом-нижнем углу, без overlap.
            Text(m.text, color = Color.White, fontSize = 15.sp,
                modifier = Modifier.padding(end = if (m.mine) 52.dp else 40.dp))
            Row(
                Modifier.align(Alignment.BottomEnd),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(fmtMsgTime(m.time), color = Color.White.copy(alpha = 0.5f), fontSize = 10.sp)
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
internal fun StatusTick(status: String) {
    val tint = Color.White.copy(alpha = 0.7f)
    when (status) {
        "delivered" -> Icon(Icons.Filled.DoneAll, "доставлено", tint = tint, modifier = Modifier.size(13.dp))
        "sent" -> Icon(Icons.Filled.Done, "отправлено", tint = tint, modifier = Modifier.size(13.dp))
        "pending" -> Icon(Icons.Filled.Schedule, "отправляется", tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(12.dp))
        "queued" -> Icon(Icons.Filled.Schedule, "ожидание", tint = tint.copy(alpha = 0.6f), modifier = Modifier.size(12.dp))
        "failed" -> Text("!", color = HoloError, fontSize = 11.sp, fontWeight = FontWeight.Bold)
    }
}


