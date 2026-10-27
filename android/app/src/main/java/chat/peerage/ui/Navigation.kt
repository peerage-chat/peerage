@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import chat.peerage.*
import chat.peerage.R

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
            .background(HoloBg)
            .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
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
            Modifier.size(Tokens.Size.tab)
                .clip(RoundedCornerShape(Tokens.Radius.chip))
                .background(Tokens.Colors.surface)
                .clickable { onManage() },
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

/**
 * Один таб-пилюля: подпись, счётчик непрочитанного, у активного — плашка
 * приглушённого акцента.
 *
 * Было подчёркивание, но оно требует линии-основы, которой у нас нет: полоса табов
 * стоит прямо на фоне экрана, и подчёркнутый текст висел в пустоте. Плашка же
 * работает по тому же правилу, что иконки шапки и поле ввода: контрол обязан иметь
 * фон, иначе он неотличим от подписи.
 */
@Composable
private fun FolderTab(label: String, selected: Boolean, badge: Int, onClick: () -> Unit) {
    // Цвет и фон переезжают плавно — резкая смена читается как перерисовка экрана.
    val fg by animateColorAsState(
        if (selected) HoloAccent else Tokens.Colors.secondary,
        animationSpec = tween(Tokens.Anim.fast),
        label = "tabFg",
    )
    val bg by animateColorAsState(
        if (selected) Tokens.Colors.accentMuted else Tokens.Colors.surface,
        animationSpec = tween(Tokens.Anim.fast),
        label = "tabBg",
    )
    Row(
        Modifier.height(Tokens.Size.tab)
            .clip(RoundedCornerShape(Tokens.Radius.chip))
            .background(bg)
            .clickable { onClick() }
            .padding(horizontal = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        Text(label, color = fg, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium, maxLines = 1)
        if (badge > 0) CountBadge(badgeCount(badge))
    }
}
