@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

// Третий элемент — resId подписи (локализуется в месте рендера через stringResource).
internal val Folders = listOf(
    Triple("all", TablerIcons.Messages, R.string.folder_all),
    Triple("dm", TablerIcons.Message, R.string.folder_dm),
    Triple("room", TablerIcons.Headphones, R.string.folder_rooms),
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
    Folders.map { it.first } + (if (hasRequests(model)) listOf(RequestsFolder) else emptyList()) + model.folders.map { it.id }

/** Вкладка «Запросы · N»: стоит после «Группы», пока есть хоть один запрос или приглашение. */
internal const val RequestsFolder = "requests"

internal fun hasRequests(model: AppModel): Boolean = model.contactRequests.isNotEmpty() || model.groupInvites.isNotEmpty()

/**
 * Шапка главной на фоне: аватар (→ «Профиль»; он же говорит, под кем ты сейчас), «Чаты» со
 * строкой статуса меша (тап → «Сеть», долгий тап — диагностический замер приёма) и поиск.
 */
@Composable
internal fun HomeTopBar(model: AppModel, onProfile: () -> Unit, onNetwork: () -> Unit, onSearch: () -> Unit) {
    val profile = stringResource(R.string.me_title)
    val network = stringResource(R.string.menu_network)
    Row(
        Modifier.fillMaxWidth().background(HoloBg)
            .padding(start = Tokens.Space.m, end = Tokens.Space.s, top = Tokens.Space.s, bottom = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(Tokens.Size.touch).clip(CircleShape)
                .clickable(role = Role.Button, onClickLabel = profile) { onProfile() },
            contentAlignment = Alignment.Center,
        ) {
            SquareAvatar(
                model.selfName.ifBlank { model.me?.pubkey ?: "?" },
                size = Tokens.Size.avatarTop,
                avatarBytes = model.selfAvatar,
                circle = true,
            )
        }
        Column(
            Modifier.weight(1f)
                .heightIn(min = Tokens.Size.touch)
                .semantics { role = Role.Button }
                .pointerInput(Unit) {
                    detectTapGestures(onTap = { onNetwork() }, onLongPress = { model.startBenchRecv() })
                }
                .padding(start = Tokens.Space.s),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                stringResource(R.string.nav_chats),
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            NetStatus(model.netPeers, Tokens.Font.small, Modifier.semantics { contentDescription = network })
        }
        BarIcon(TablerIcons.Search, stringResource(R.string.cd_search), onClick = onSearch)
    }
}

/**
 * Полоса папок текстовыми табами на панели: встроенные «Все · Личные · Группы», дальше свои,
 * «+» — новая папка. Выбранная — белым с линией акцента; линия-основа — разделитель панели
 * под полосой, поэтому подчёркивание не висит в пустоте.
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
    val unreadAll = model.unread.values.sum() + model.groupUnread.values.sum()
    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.l),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.l),
        ) {
            Folders.forEach { (key, _, labelRes) ->
                FolderTab(
                    stringResource(labelRes),
                    key == activeFilter,
                    if (key == "all") unreadAll else 0,
                ) { onFilter(key) }
            }
            if (hasRequests(model)) {
                FolderTab(
                    stringResource(R.string.tab_requests),
                    activeFilter == RequestsFolder,
                    model.contactRequests.size + model.groupInvites.size,
                ) { onFilter(RequestsFolder) }
            }
            model.folders.forEach { f ->
                FolderTab(f.name, f.id == activeFilter, 0) { onFilter(f.id) }
            }
            val create = stringResource(R.string.folder_create)
            Box(
                Modifier.size(Tokens.Size.touch)
                    .clickable(role = Role.Button, onClickLabel = create) { onManage() },
                contentAlignment = Alignment.Center,
            ) {
                Icon(TablerIcons.Plus, create, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }
        HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider)
    }
}

/** Один таб: подпись (и счётчик у «Все»), у выбранного — белый текст и линия акцента снизу. */
@Composable
private fun FolderTab(label: String, selected: Boolean, badge: Int, onClick: () -> Unit) {
    // Цвет переезжает плавно — резкая смена читается как перерисовка экрана.
    val fg by animateColorAsState(
        if (selected) Color.White else Tokens.Colors.tertiary,
        animationSpec = tween(Tokens.Anim.fast),
        label = "tabFg",
    )
    Box(
        Modifier.heightIn(min = Tokens.Size.touch)
            .selectable(selected = selected, role = Role.Tab, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            Modifier.padding(horizontal = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
        ) {
            Text(label, color = fg, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1)
            if (badge > 0) CountBadge(badgeCount(badge))
        }
        // Ширину линия берёт у таба (matchParentSize): внутри горизонтальной прокрутки
        // ширина не ограничена, и fillMaxWidth дал бы линию нулевой длины.
        if (selected) {
            Box(Modifier.matchParentSize(), contentAlignment = Alignment.BottomCenter) {
                Box(Modifier.fillMaxWidth().height(Tokens.Size.tabLine).background(Tokens.Colors.accent))
            }
        }
    }
}
