@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.Forum
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

// Третий элемент — resId подписи (локализуется в месте рендера через stringResource).
internal val Folders = listOf(
    Triple("all", Icons.Filled.Forum, R.string.folder_all),
    Triple("dm", Icons.AutoMirrored.Filled.Chat, R.string.nav_chats),
    Triple("room", Icons.Filled.Headset, R.string.folder_rooms),
    Triple("channel", Icons.Filled.Campaign, R.string.folder_channels),
)

internal fun folderLabel(key: String): Int = Folders.firstOrNull { it.first == key }?.third ?: R.string.nav_chats

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
        Folders.forEach { (key, icon, labelRes) ->
            FolderTile(icon, stringResource(labelRes), key == activeFilter, badgeFor(key)) { onFilter(key) }
        }
        FolderTile(Icons.Filled.Add, stringResource(R.string.folder_create), false, 0) { /* TODO: создать папку */ }
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
            Icon(
                icon,
                label,
                tint = if (selected) HoloAccent else HoloSecondary,
                modifier = Modifier.size(24.dp),
            )
        }
        if (badge > 0) {
            Box(
                Modifier.clip(RoundedCornerShape(percent = 30)).background(HoloAccent).border(2.dp, HoloBg, RoundedCornerShape(percent = 30))
                    .defaultMinSize(minWidth = 18.dp, minHeight = 18.dp)
                    .padding(horizontal = 4.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    if (badge > 99) "99+" else "$badge",
                    color = HoloAccentDark,
                    fontSize = 10.sp,
                    fontWeight = FontWeight.Medium,
                )
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
            Text(
                stringResource(R.string.common_you),
                color = Color.White,
                fontSize = 18.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.weight(1f),
            )
            Icon(Icons.Filled.QrCode2, stringResource(R.string.menu_my_key_qr), tint = HoloAccent, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.height(8.dp))
        // Основные разделы.
        listOf(
            Triple(Icons.Filled.Group, stringResource(R.string.menu_contacts), Screen.ContactList),
            Triple(Icons.Filled.Call, stringResource(R.string.menu_calls), Screen.Calls),
            Triple(Icons.Filled.Share, stringResource(R.string.menu_network), Screen.Network),
            Triple(Icons.Filled.Lock, stringResource(R.string.menu_saved), Screen.Saved),
        ).forEach { (icon, label, dest) -> DrawerItem(icon, label) { navigate(dest) } }
        HorizontalDivider(color = HoloBorder, modifier = Modifier.padding(vertical = 4.dp))
        DrawerItem(Icons.Filled.Settings, stringResource(R.string.menu_settings)) { navigate(Screen.Settings) }
        Spacer(Modifier.weight(1f))
        HorizontalDivider(color = HoloBorder)
        Text(
            "p2p messenger · v0.3",
            color = HoloSecondary,
            fontSize = 11.sp,
            modifier = Modifier.padding(horizontal = 20.dp, vertical = 14.dp),
        )
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
