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

internal val Folders = listOf(
    Triple("all", Icons.Filled.Forum, "Все"),
    Triple("dm", Icons.AutoMirrored.Filled.Chat, "Чаты"),
    Triple("room", Icons.Filled.Headset, "Комнаты"),
    Triple("channel", Icons.Filled.Campaign, "Каналы"),
)

internal fun folderLabel(key: String): String = Folders.firstOrNull { it.first == key }?.third ?: "Чаты"

/** Рейл папок-иконок (Discord-стиль): активная подсвечена, бейдж непрочитанного
 *  в правом-нижнем углу, `+` создаёт папку (пока заглушка). */
@Composable
internal fun FolderRail(model: AppModel, activeFilter: String, onFilter: (String) -> Unit) {
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
internal fun FolderTile(icon: ImageVector, label: String, selected: Boolean, badge: Int, onClick: () -> Unit) {
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
internal fun AppDrawer(model: AppModel, navigate: (Screen) -> Unit) {
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
internal fun DrawerItem(icon: ImageVector, label: String, onClick: () -> Unit) {
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
internal fun ComingSoon(icon: ImageVector, title: String) {
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

