@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package dev.p2p.messenger.ui

import android.os.Build
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/**
 * Строка списка чатов — уже подготовленная к показу: личные чаты и группы приведены
 * к одному виду. Список на главной один, и решать «это контакт или группа» на каждом
 * элементе разметки означало бы писать её дважды.
 */
private data class ChatRowUi(
    val id: String,
    val group: Boolean,
    val title: String,
    val avatar: ByteArray? = null,
    val online: Boolean = false,
    val verified: Boolean = false,
    val unread: Int = 0,
    val muted: Boolean = false,
    val pinnedAt: Long = 0,
    val summary: ChatSummary? = null,
    /** Сколько человек сейчас в комнате (только для групп; 0 — никого). */
    val inRoom: Int = 0,
    val members: Int = 0,
) {
    val time: Long get() = summary?.lastTime ?: 0
    val pinned: Boolean get() = pinnedAt > 0
}

/**
 * Главная: список чатов с переключением папок горизонтальным свайпом.
 *
 * Свайп по списку — единственный горизонтальный жест на этом экране. Быстрые
 * действия над чатом ушли в меню долгого нажатия: два горизонтальных жеста в одной
 * зоне всегда дерутся, и проигрывает более редкий — то есть папки.
 */
@Composable
internal fun ChatsPager(
    model: AppModel,
    activeFilter: String,
    onFilter: (String) -> Unit,
    onOpenDm: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    val keys = folderKeys(model)
    val target = keys.indexOf(activeFilter).coerceAtLeast(0)
    val pager = rememberPagerState(initialPage = target) { keys.size }
    // Синхронизация в обе стороны: тап по табу листает страницу, свайп меняет таб.
    LaunchedEffect(target) {
        if (pager.currentPage != target) pager.animateScrollToPage(target)
    }
    LaunchedEffect(pager.settledPage) {
        keys.getOrNull(pager.settledPage)?.let { if (it != activeFilter) onFilter(it) }
    }
    HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { page ->
        ContactsScreen(model, keys.getOrElse(page) { "all" }, onOpenDm, onOpenGroup)
    }
}

@Composable
internal fun ContactsScreen(
    model: AppModel,
    activeFilter: String,
    onOpenDm: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    // Пользовательская папка — это ФИЛЬТР по правилам, встроенные — по типу чата.
    val folder = model.folderById(activeFilter)
    val showDm = folder != null || activeFilter == "all" || activeFilter == "dm"
    val showRoom = folder != null || activeFilter == "all" || activeFilter == "room"
    val dmList = if (folder == null) model.contacts else model.contacts.filter { model.folderHasDm(folder, it.key) }
    val groupList = if (folder == null) model.groups else model.groups.filter { model.folderHasGroup(folder, it.gid) }

    // Один список: закреплённые сверху (позже закрепили — выше), дальше по времени
    // последнего сообщения. Чаты без переписки уходят в конец по алфавиту — иначе
    // новый пустой контакт с временем 0 встал бы ниже годовалой переписки случайно.
    val rows = buildList {
        if (showRoom) {
            groupList.forEach { g ->
                val s = model.chatSummaries[model.chatKey(g.gid, true)]
                add(
                    ChatRowUi(
                        id = g.gid, group = true, title = g.name,
                        unread = model.groupUnread[g.gid] ?: 0,
                        muted = s?.muted ?: false, pinnedAt = s?.pinnedAt ?: 0, summary = s,
                        inRoom = model.groupPresence[g.gid]?.size ?: 0, members = g.members,
                    ),
                )
            }
        }
        if (showDm) {
            dmList.forEach { c ->
                val s = model.chatSummaries[model.chatKey(c.key, false)]
                add(
                    ChatRowUi(
                        id = c.key, group = false, title = c.displayName,
                        avatar = model.contactAvatars[c.avatarId],
                        online = model.online[c.key] == true, verified = c.verified,
                        unread = model.unread[c.key] ?: 0,
                        muted = s?.muted ?: false, pinnedAt = s?.pinnedAt ?: 0, summary = s,
                    ),
                )
            }
        }
    }.sortedWith(
        compareByDescending<ChatRowUi> { it.pinnedAt }
            .thenByDescending { it.time }
            .thenBy { it.title.lowercase() },
    )

    Column(Modifier.fillMaxSize()) {
        // Очередь недоставленного разрослась. НИЧЕГО не удаляем — сообщения ждут
        // доставки; просто предупреждаем, чтобы человек знал (тумблер в Настройках).
        if (model.outboxWarn && model.warnOutboxGrowth) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = ScreenPad, vertical = 6.dp)
                    .clip(RoundedCornerShape(Tokens.Radius.chip))
                    .background(Color(0xFF3A2F12))
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("⚠", color = Color(0xFFF6C453), fontSize = 15.sp)
                Spacer(Modifier.width(10.dp))
                Text(
                    stringResource(R.string.outbox_warn, model.outboxPending),
                    color = Color(0xFFF6C453),
                    fontSize = 13.sp,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        // Счётчик пиров переехал в шапку (PeerIndicator). Здесь — только бенч-замер
        // приёма, пока он активен (диагностика; запуск — долгим тапом по индикатору).
        if (model.benchRecv) {
            Text(
                stringResource(R.string.bench_recv_rate, "%.1f".format(model.benchRecvMbps)),
                color = HoloAccent,
                fontSize = 11.sp,
                modifier = Modifier.padding(horizontal = ScreenPad, vertical = 6.dp),
            )
        }
        when {
            activeFilter == "channel" -> CenterMessage(stringResource(R.string.channels_soon))
            model.contacts.isEmpty() && model.groups.isEmpty() &&
                model.contactRequests.isEmpty() && model.groupInvites.isEmpty() ->
                CenterMessage(stringResource(R.string.contacts_empty))
            else -> LazyColumn(Modifier.fillMaxSize()) {
                // Гейт: входящие запросы от незнакомцев — карточками наверху (фильтр DM/all).
                if (showDm && model.contactRequests.isNotEmpty()) {
                    item(key = "req_header") {
                        Text(
                            stringResource(R.string.requests_header, model.contactRequests.size),
                            color = HoloAccent,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(start = ScreenPad, end = ScreenPad, top = 10.dp, bottom = 4.dp),
                        )
                    }
                    items(model.contactRequests, key = { "req_${it.key}" }) { r ->
                        ContactRequestCard(model, r)
                    }
                }
                // Приглашения в группы — карточками (фильтр Комнаты/all).
                if (showRoom && model.groupInvites.isNotEmpty()) {
                    item(key = "ginv_header") {
                        Text(
                            stringResource(R.string.group_invites_header, model.groupInvites.size),
                            color = HoloAccent,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(start = ScreenPad, end = ScreenPad, top = 10.dp, bottom = 4.dp),
                        )
                    }
                    items(model.groupInvites, key = { "ginv_${it.gid}" }) { inv ->
                        GroupInviteCard(model, inv)
                    }
                }
                // Один контейнер-карточка: личные чаты и группы вперемешку,
                // закреплённые сверху. Скругление — только у первой и последней
                // строки, поэтому список читается как единая карточка, а не как
                // стопка отдельных.
                itemsIndexed(rows, key = { _, r -> (if (r.group) "g_" else "c_") + r.id }) { i, r ->
                    ChatListRow(
                        model = model,
                        row = r,
                        first = i == 0,
                        last = i == rows.lastIndex,
                        // Граница между закреплёнными и остальными — разделитель во
                        // всю ширину: он и разделяет группы, и не оставляет дыры.
                        fullDivider = i > 0 && rows[i - 1].pinned && !r.pinned,
                        onClick = { if (r.group) onOpenGroup(r.id) else onOpenDm(r.id) },
                    )
                }
                item(key = "tail") { Spacer(Modifier.height(88.dp)) } // место под FAB
            }
        }
    }
}

/**
 * Строка чата: аватар, имя, превью последнего сообщения и правая колонка
 * «время + значок».
 *
 * Правая колонка — ДВЕ строки фиксированной высоты, выровненные с именем и превью:
 * время всегда на первой, значок всегда на второй. Раньше колонка центрировалась
 * целиком, и время съезжало вниз у чатов без счётчика.
 */
@Composable
private fun ChatListRow(
    model: AppModel,
    row: ChatRowUi,
    first: Boolean,
    last: Boolean,
    fullDivider: Boolean,
    onClick: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val r = Tokens.Radius.card
    val shape = RoundedCornerShape(
        topStart = if (first) r else 0.dp,
        topEnd = if (first) r else 0.dp,
        bottomStart = if (last) r else 0.dp,
        bottomEnd = if (last) r else 0.dp,
    )
    val bg = if (row.pinned) Tokens.Colors.rowPinned else Tokens.Colors.surface
    Column(Modifier.padding(horizontal = Tokens.Space.s).clip(shape).background(bg)) {
        if (!first) {
            HorizontalDivider(
                thickness = 1.dp,
                color = Tokens.Colors.divider,
                modifier = if (fullDivider) {
                    Modifier
                } else {
                    // Обычный разделитель начинается за аватаром: он отделяет строки,
                    // а не рассекает список.
                    Modifier.padding(start = Tokens.Size.avatarRow + Tokens.Space.m + Tokens.Space.m)
                },
            )
        }
        Box {
            Row(
                Modifier.fillMaxWidth()
                    .combinedClickable(onClick = onClick, onLongClick = { menu = true })
                    .heightIn(min = Tokens.Size.chatRow)
                    .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (row.group) {
                    SquareAvatar(row.title, size = Tokens.Size.avatarRow, bg = Color(0xFF0D2A0D)) {
                        Icon(
                            Icons.Filled.Group,
                            null,
                            tint = HoloOnline,
                            modifier = Modifier.size(Tokens.Size.iconM),
                        )
                    }
                } else {
                    SquareAvatar(
                        row.title,
                        size = Tokens.Size.avatarRow,
                        online = row.online,
                        avatarBytes = row.avatar,
                        circle = true, // человек — круг, сущность — скруглённый квадрат
                        ring = bg,
                    )
                }
                Spacer(Modifier.width(Tokens.Space.m))
                Column(Modifier.weight(1f)) {
                    Row(
                        Modifier.height(20.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            row.title,
                            color = Color.White,
                            fontSize = 15.sp,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        if (row.verified) {
                            Spacer(Modifier.width(Tokens.Space.xs))
                            Icon(
                                Icons.Filled.Verified,
                                null,
                                tint = HoloVerified,
                                modifier = Modifier.size(13.dp),
                            )
                        }
                        if (row.muted) {
                            Spacer(Modifier.width(Tokens.Space.xs))
                            Icon(
                                TablerIcons.BellOff,
                                null,
                                tint = Tokens.Colors.tertiary,
                                modifier = Modifier.size(13.dp),
                            )
                        }
                    }
                    Spacer(Modifier.height(Tokens.Space.xxs))
                    Row(Modifier.height(18.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            chatPreview(model, row),
                            color = Tokens.Colors.secondary,
                            fontSize = 13.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                Spacer(Modifier.width(Tokens.Space.s))
                // Ширина МИНИМАЛЬНАЯ, а не фиксированная: «yesterday» в 46dp не
                // влезает и обрезается в «yesterda». Правый край всё равно общий —
                // колонка прижата к нему, — а левый пусть плавает под содержимое.
                Column(
                    Modifier.widthIn(min = Tokens.Size.metaSlot),
                    horizontalAlignment = Alignment.End,
                ) {
                    Box(Modifier.height(20.dp), contentAlignment = Alignment.CenterEnd) {
                        Text(
                            fmtListTime(row.time),
                            color = Tokens.Colors.tertiary,
                            fontSize = 11.sp,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                    Spacer(Modifier.height(Tokens.Space.xxs))
                    // Нижний слот занимает место ВСЕГДА, даже пустой: иначе время
                    // прыгало бы по вертикали в зависимости от наличия счётчика.
                    Box(Modifier.height(18.dp), contentAlignment = Alignment.CenterEnd) {
                        ChatRowBadge(row)
                    }
                }
            }
            ChatRowMenu(model, row, menu) { menu = false }
        }
    }
}

/** Нижний слот правой колонки: счётчик непрочитанного, а если его нет — статус
 *  доставки СВОЕГО последнего сообщения. У чужих там пусто. */
@Composable
private fun ChatRowBadge(row: ChatRowUi) {
    val s = row.summary
    when {
        row.unread > 0 -> Box(
            Modifier.clip(RoundedCornerShape(Tokens.Radius.chip))
                .background(if (row.muted) Tokens.Colors.unreadMuted else HoloAccent)
                .defaultMinSize(minWidth = 19.dp)
                .padding(horizontal = 6.dp, vertical = 1.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                if (row.unread > 99) "99+" else "${row.unread}",
                color = if (row.muted) Color(0xFFB9B9C2) else HoloAccentDark,
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
            )
        }
        // Те же значки, что в пузыре чата: часы → одна галочка → две. Один и тот же
        // факт не должен выглядеть в списке иначе, чем внутри переписки.
        s != null && s.lastMine && !s.lastSystem -> when (s.lastStatus) {
            "delivered" -> Icon(
                Icons.Filled.DoneAll,
                stringResource(R.string.status_delivered),
                tint = HoloAccent,
                modifier = Modifier.size(15.dp),
            )
            "sent" -> Icon(
                Icons.Filled.Done,
                stringResource(R.string.status_sent),
                tint = Tokens.Colors.tertiary,
                modifier = Modifier.size(15.dp),
            )
            "pending", "queued" -> Icon(
                Icons.Filled.Schedule,
                stringResource(R.string.status_sending),
                tint = Tokens.Colors.tertiary,
                modifier = Modifier.size(14.dp),
            )
            else -> Unit
        }
        row.inRoom > 0 -> Icon(
            Icons.Filled.Headset,
            null,
            tint = HoloOnline,
            modifier = Modifier.size(14.dp),
        )
    }
}

/** Меню чата по долгому нажатию: закрепить и звук. Действий больше двух, и они
 *  разной цены — поэтому меню, а не свайп по строке (горизонталь отдана папкам). */
@Composable
private fun ChatRowMenu(model: AppModel, row: ChatRowUi, open: Boolean, onClose: () -> Unit) {
    DropdownMenu(expanded = open, onDismissRequest = onClose) {
        DropdownMenuItem(
            text = {
                Text(stringResource(if (row.pinned) R.string.chat_menu_unpin else R.string.chat_menu_pin))
            },
            onClick = {
                model.pinChat(row.id, row.group, !row.pinned)
                onClose()
            },
        )
        DropdownMenuItem(
            text = {
                Text(stringResource(if (row.muted) R.string.chat_menu_unmute else R.string.chat_menu_mute))
            },
            onClick = {
                model.muteChat(row.id, row.group, !row.muted)
                onClose()
            },
        )
    }
}

/**
 * Превью последнего сообщения. В группе перед текстом — имя автора, у своих —
 * «Вы:». Вложение показывается словом («Фото», «Голосовое», имя файла): текста у
 * него нет, а пустая строка выглядела бы как потерянное сообщение.
 */
@Composable
private fun chatPreview(model: AppModel, row: ChatRowUi): String {
    val s = row.summary ?: return stringResource(R.string.chats_preview_empty)
    val body = when {
        s.lastText.isNotBlank() -> s.lastText
        s.lastKind == "image" -> stringResource(R.string.chats_preview_photo)
        s.lastKind == "video" -> stringResource(R.string.chats_preview_video)
        s.lastKind == "voice" -> stringResource(R.string.chats_preview_voice)
        s.lastKind == "file" -> stringResource(R.string.chats_preview_file)
        else -> stringResource(R.string.chats_preview_empty)
    }.replace('\n', ' ')
    // Системную строку («Артём добавил Веру») подписывать автором незачем — она и
    // так говорит о ком-то в третьем лице.
    if (s.lastSystem) return body
    val who = when {
        s.lastMine -> stringResource(R.string.chats_preview_you)
        row.group -> model.peerName(s.lastSender) + ":"
        else -> ""
    }
    return if (who.isEmpty()) body else "$who $body"
}

/** Карточка входящего запроса в контакты (гейт вместо TOFU): аватар-буква, короткий
 *  ключ, превью первого сообщения и кнопки Принять / Отклонить / Заблокировать. */
@Composable
private fun ContactRequestCard(model: AppModel, r: ContactRequest) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = ScreenPad, vertical = 4.dp)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .background(HoloSurface)
            .padding(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SquareAvatar(r.shortKey, size = 40.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    r.shortKey + "…",
                    color = Color.White,
                    fontSize = 15.sp,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (r.preview.isNotBlank()) {
                    Text(
                        r.preview,
                        color = HoloSecondary,
                        fontSize = 13.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = { model.acceptRequest(r.key) },
                colors = ButtonDefaults.buttonColors(containerColor = HoloAccent, contentColor = HoloAccentDark),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
            ) { Text(stringResource(R.string.req_accept), fontSize = 13.sp) }
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = { model.declineRequest(r.key) }) {
                Text(stringResource(R.string.req_decline), color = HoloSecondary, fontSize = 13.sp)
            }
            Spacer(Modifier.weight(1f))
            TextButton(onClick = { model.blockRequest(r.key) }) {
                Text(stringResource(R.string.req_block), color = HoloError, fontSize = 13.sp)
            }
        }
    }
}

/** Карточка входящего приглашения в группу (вместо тихого авто-входа): аватар-иконка,
 *  имя группы, кто пригласил, кнопки Принять / Отклонить. */
@Composable
private fun GroupInviteCard(model: AppModel, inv: GroupInvite) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = ScreenPad, vertical = 4.dp)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .background(HoloSurface)
            .padding(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SquareAvatar(inv.name.ifBlank { "#" }, size = 40.dp, bg = Color(0xFF0D2A0D)) {
                Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(20.dp))
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    inv.name.ifBlank { stringResource(R.string.group_default_name) },
                    color = Color.White,
                    fontSize = 15.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    stringResource(R.string.group_invite_from, model.peerName(inv.inviter)),
                    color = HoloSecondary,
                    fontSize = 13.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Spacer(Modifier.height(10.dp))
        if (inv.waiting) {
            // Согласие уже дано, но пригласивший офлайн: карточка остаётся и честно
            // говорит, чего ждём. Раньше она просто исчезала, и вступление молча
            // терялось — повторить было нечем.
            Text(
                stringResource(R.string.group_invite_waiting),
                color = HoloSecondary,
                fontSize = 13.sp,
            )
        } else {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Button(
                    onClick = { model.acceptGroupInvite(inv.gid) },
                    colors = ButtonDefaults.buttonColors(containerColor = HoloAccent, contentColor = HoloAccentDark),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
                ) { Text(stringResource(R.string.group_invite_join), fontSize = 13.sp) }
                Spacer(Modifier.width(8.dp))
                TextButton(onClick = { model.declineGroupInvite(inv.gid) }) {
                    Text(stringResource(R.string.req_decline), color = HoloSecondary, fontSize = 13.sp)
                }
            }
        }
    }
}

/** Полный список контактов (раздел меню «Контакты»). Тап → личный чат.
 *  Добавление — через кнопку в шапке (Root). */
@Composable
internal fun ContactListScreen(model: AppModel, onOpen: (String) -> Unit) {
    if (model.contacts.isEmpty()) {
        EmptyState(Icons.Filled.Group, stringResource(R.string.contacts_empty_title), stringResource(R.string.contacts_empty_hint))
        return
    }
    // Один контейнер на весь список — как на главной: контакты это такой же
    // перечень людей, и выглядеть он обязан так же.
    LazyColumn(Modifier.fillMaxSize().padding(top = Tokens.Space.s)) {
        itemsIndexed(model.contacts, key = { _, c -> c.key }) { i, c ->
            val online = model.online[c.key] == true
            val first = i == 0
            val last = i == model.contacts.lastIndex
            Column(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s)
                    .clip(
                        RoundedCornerShape(
                            topStart = if (first) Tokens.Radius.card else 0.dp,
                            topEnd = if (first) Tokens.Radius.card else 0.dp,
                            bottomStart = if (last) Tokens.Radius.card else 0.dp,
                            bottomEnd = if (last) Tokens.Radius.card else 0.dp,
                        ),
                    )
                    .background(Tokens.Colors.surface),
            ) {
                CardRow(onClick = { onOpen(c.key) }, divider = !first, inset = 70.dp) {
                    SquareAvatar(
                        c.displayName,
                        size = Tokens.Size.avatarRow,
                        online = online,
                        avatarBytes = model.contactAvatars[c.avatarId],
                        circle = true,
                        ring = Tokens.Colors.surface,
                    )
                    Spacer(Modifier.width(Tokens.Space.m))
                    Column(Modifier.weight(1f)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                c.displayName,
                                color = Color.White,
                                fontSize = 15.sp,
                                fontWeight = FontWeight.Medium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f, fill = false),
                            )
                            if (c.verified) {
                                Spacer(Modifier.width(Tokens.Space.xs))
                                Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(13.dp))
                            }
                        }
                        Text(
                            stringResource(if (online) R.string.status_online else R.string.status_offline),
                            color = if (online) HoloOnline else Tokens.Colors.tertiary,
                            fontSize = 12.sp,
                        )
                    }
                }
            }
        }
        item { Spacer(Modifier.height(Tokens.Space.l)) }
    }
}

@Composable
internal fun AddScreen(
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
    LaunchedEffect(st) {
        if (st == "added") {
            model.addStatus = ""
            onDone()
        }
    }
    // Получаем ключ из сканера (приходит через Root → scannedKey).
    LaunchedEffect(scannedKey) { if (scannedKey != null) keyHex = scannedKey }
    Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
        FieldLabel(stringResource(R.string.add_label_pubkey))
        TextField(
            keyHex,
            { keyHex = it },
            Modifier.fillMaxWidth(),
            colors = inputColors(),
            placeholder = { Text(stringResource(R.string.add_hint_hex), color = HoloSecondary) },
        )
        AccentButton(stringResource(R.string.title_scan_qr), modifier = Modifier.fillMaxWidth()) { onScan() }
        FieldLabel(stringResource(R.string.label_name))
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
        AccentButton(stringResource(R.string.add_btn)) { model.addContact(keyHex.trim(), name.trim()) }
        val msg = when (st) {
            "known" -> stringResource(R.string.add_known)
            "error" -> stringResource(R.string.add_error)
            else -> ""
        }
        if (msg.isNotEmpty()) Text(msg, color = HoloError, fontSize = 13.sp)
    }
}

@Composable
internal fun MeScreen(model: AppModel, navigate: (Screen) -> Unit = {}) {
    val me = model.me ?: return
    // В QR — код обмена, а не голый ключ: после смены адреса по одной лишь
    // личности до нас не достучаться (ADR-007).
    val code = remember(me.pubkey) { model.myContactCode() }
    val qr = remember(code) { generateQrBitmap(code) }
    val ctx = LocalContext.current
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
    ) {
        // ── Шапка профиля: аватар, имя, адрес ───────────────────────────────
        // Профиль занял целый экран вместо тесной шапки выдвижного меню: здесь
        // живут вещи, которые человек показывает другим (код, отпечаток) — им
        // нужно место, а в панели помещалась одна строка.
        Column(
            Modifier.fillMaxWidth().padding(vertical = Tokens.Space.l),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            SquareAvatar(
                model.selfName.ifBlank { me.pubkey },
                size = Tokens.Size.avatarBig,
                avatarBytes = model.selfAvatar,
                circle = true,
                ring = HoloBg,
            )
            Spacer(Modifier.height(Tokens.Space.m))
            Text(
                model.selfName.ifBlank { stringResource(R.string.common_you) },
                color = Color.White,
                fontSize = 20.sp,
                fontWeight = FontWeight.Medium,
            )
            Text(
                stringResource(R.string.peers_online, model.netPeers),
                color = if (model.netPeers > 0) HoloOnline else Tokens.Colors.tertiary,
                fontSize = 13.sp,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }

        // ── Код обмена ──────────────────────────────────────────────────────
        SectionTitle(stringResource(R.string.me_qr_card))
        CardSection {
            Column(
                Modifier.fillMaxWidth().padding(Tokens.Space.l),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (qr != null) {
                    Image(
                        bitmap = qr.asImageBitmap(),
                        contentDescription = stringResource(R.string.qr_cd),
                        modifier = Modifier.size(220.dp).clip(RoundedCornerShape(Tokens.Radius.tile)),
                    )
                    Spacer(Modifier.height(Tokens.Space.m))
                }
                Text(
                    stringResource(R.string.me_qr_hint),
                    color = Tokens.Colors.secondary,
                    fontSize = 12.sp,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
            CardRow(inset = 0.dp) {
                Column(Modifier.weight(1f)) {
                    FieldLabel(stringResource(R.string.me_address_label))
                    Text(
                        me.address,
                        color = Color.White,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Spacer(Modifier.width(Tokens.Space.s))
                BarIcon(TablerIcons.Check, stringResource(R.string.me_copy_key)) {
                    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                        as android.content.ClipboardManager
                    cm.setPrimaryClip(android.content.ClipData.newPlainText("p2p pubkey", me.pubkey))
                    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
                        android.widget.Toast.makeText(ctx, ctx.getString(R.string.common_copied), android.widget.Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }

        // ── Отпечаток ───────────────────────────────────────────────────────
        SectionTitle(stringResource(R.string.me_fingerprint_label))
        CardSection {
            Column(Modifier.padding(Tokens.Space.m)) {
                FingerprintGrid(model.visualFingerprint())
                Text(
                    stringResource(R.string.fp_self_hint),
                    color = Tokens.Colors.secondary,
                    fontSize = 11.sp,
                    modifier = Modifier.padding(top = Tokens.Space.s),
                )
                Text(
                    me.fingerprint,
                    color = HoloAccent,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(top = Tokens.Space.xs),
                )
            }
        }

        // ── Разделы приложения ──────────────────────────────────────────────
        // Списком, а не вкладками: это разнородные места, куда заходят по одному
        // и возвращаются, — вкладки обещали бы, что их содержимое сопоставимо.
        SectionTitle(stringResource(R.string.settings_title_sections))
        CardSection {
            val items = listOf(
                Triple(Icons.Filled.Group, stringResource(R.string.menu_contacts), Screen.ContactList),
                Triple(Icons.Filled.Call, stringResource(R.string.menu_calls), Screen.Calls),
                Triple(Icons.Filled.Share, stringResource(R.string.menu_network), Screen.Network),
                Triple(Icons.Filled.Lock, stringResource(R.string.menu_saved), Screen.Saved),
                Triple(Icons.Filled.Storage, stringResource(R.string.menu_storage), Screen.Storage),
                Triple(Icons.Filled.Settings, stringResource(R.string.menu_settings), Screen.Settings),
            )
            items.forEachIndexed { i, (icon, label, dest) ->
                CardRow(onClick = { navigate(dest) }, divider = i > 0) {
                    Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconM))
                    Spacer(Modifier.width(Tokens.Space.m))
                    Text(label, color = Color.White, fontSize = 15.sp, modifier = Modifier.weight(1f))
                    Icon(
                        TablerIcons.ChevronRight,
                        null,
                        tint = Tokens.Colors.tertiary,
                        modifier = Modifier.size(Tokens.Size.iconS),
                    )
                }
            }
        }
        // Версия — из того же ресурса, что и в «О приложении»: подпись «v0.3» здесь
        // жила отдельной строкой в коде и уже разошлась с настройками.
        Text(
            stringResource(R.string.app_name) + " · " + stringResource(R.string.settings_version_value),
            color = Tokens.Colors.tertiary,
            fontSize = 11.sp,
            modifier = Modifier.padding(Tokens.Space.l),
        )
    }
}
