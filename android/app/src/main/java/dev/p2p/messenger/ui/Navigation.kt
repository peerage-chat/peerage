@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Storage
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
    Triple("all", TablerIcons.Messages, R.string.folder_all),
    Triple("dm", TablerIcons.Message, R.string.nav_chats),
    Triple("room", TablerIcons.Headphones, R.string.folder_rooms),
    Triple("channel", TablerIcons.Speakerphone, R.string.folder_channels),
)

internal fun folderLabel(key: String): Int = Folders.firstOrNull { it.first == key }?.third ?: R.string.nav_chats

/** Заголовок Главной: у своей папки — её имя, у встроенной — локализованная подпись. */
@Composable
internal fun folderTitle(model: AppModel, key: String): String =
    model.folderById(key)?.name ?: stringResource(folderLabel(key))

/** Ключи папок в порядке показа: встроенные, затем свои. Тот же порядок держит
 *  страницы пейджера — свайп и табы обязаны совпадать. */
@Composable
internal fun folderKeys(model: AppModel): List<String> =
    Folders.map { it.first } + model.folders.map { it.id }

/**
 * Полоса папок текстовыми табами. Пришла на смену плиточному рейлу: тот занимал
 * 62dp высоты постоянно и читался вторым уровнем навигации там, где нужен фильтр.
 * Табы — 40dp и та же работа.
 *
 * Переключение — тап или горизонтальный свайп по списку (см. [ChatsPager]).
 */
@Composable
internal fun FolderTabs(
    model: AppModel,
    activeFilter: String,
    onFilter: (String) -> Unit,
    onManage: () -> Unit = {},
) {
    val unreadAll = model.unread.values.sum()
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
            .background(HoloBg).padding(horizontal = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Folders.forEach { (key, _, labelRes) ->
            FolderTab(
                stringResource(labelRes),
                key == activeFilter,
                if (key == "all") unreadAll else 0,
            ) { onFilter(key) }
        }
        model.folders.forEach { f ->
            FolderTab(f.name, f.id == activeFilter, 0) { onFilter(f.id) }
        }
        Box(
            Modifier.size(Tokens.Size.tabBar).clickable { onManage() },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                TablerIcons.Plus,
                stringResource(R.string.folder_create),
                tint = Tokens.Colors.tertiary,
                modifier = Modifier.size(Tokens.Size.iconS),
            )
        }
    }
}

/** Один таб: подпись, счётчик непрочитанного и акцентное подчёркивание активного. */
@Composable
private fun FolderTab(label: String, selected: Boolean, badge: Int, onClick: () -> Unit) {
    // Цвет переезжает плавно — резкая смена читается как перерисовка экрана.
    val fg by animateColorAsState(
        if (selected) HoloAccent else Tokens.Colors.secondary,
        animationSpec = tween(Tokens.Anim.fast),
        label = "tabFg",
    )
    Box(contentAlignment = Alignment.BottomCenter) {
        Row(
            Modifier.height(Tokens.Size.tabBar).clickable { onClick() }
                .padding(horizontal = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
        ) {
            Text(label, color = fg, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1)
            if (badge > 0) {
                Box(
                    Modifier.clip(RoundedCornerShape(Tokens.Radius.badge)).background(HoloAccent)
                        .padding(horizontal = 5.dp, vertical = 1.dp),
                ) {
                    Text(
                        if (badge > 99) "99+" else "$badge",
                        color = HoloAccentDark,
                        fontSize = 10.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }
        }
        if (selected) {
            Box(
                Modifier.padding(horizontal = Tokens.Space.s).height(2.dp).width(28.dp)
                    .clip(RoundedCornerShape(topStart = 2.dp, topEnd = 2.dp))
                    .background(HoloAccent),
            )
        }
    }
}

/** Левое меню (Telegram-style): шапка-профиль «Вы» (тап → ключ/QR) + пункты
 *  Контакты·Звонки·Сеть·Сохранённое·Настройки + версия. */
@Composable
internal fun AppDrawer(model: AppModel, navigate: (Screen) -> Unit) {
    val me = model.me
    // Своё имя из движка (M2); иначе «Вы».
    val savedName = model.selfName.takeIf { it.isNotBlank() }
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
            SquareAvatar(me?.pubkey ?: "?", size = 52.dp, avatarBytes = model.selfAvatar)
            Spacer(Modifier.width(14.dp))
            Text(
                savedName ?: stringResource(R.string.common_you),
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
            Triple(Icons.Filled.Storage, stringResource(R.string.menu_storage), Screen.Storage),
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
