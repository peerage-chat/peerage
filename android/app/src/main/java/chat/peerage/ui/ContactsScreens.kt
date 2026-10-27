@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package chat.peerage.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

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
    /** В непрочитанном меня упомянули (только группы). */
    val mentioned: Boolean = false,
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
    pager: PagerState,
    onOpenDm: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    // Состояние пейджера живёт выше (MainActivity): по нему же рисуются табы, так что
    // синхронизировать нечего — и драться двум эффектам тоже нечему.
    val keys = folderKeys(model)
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
                        mentioned = model.groupMentioned[g.gid] == true,
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
                    .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs)
                    .clip(RoundedCornerShape(Tokens.Radius.card))
                    .background(Tokens.Colors.noticeBg)
                    .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.m),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(Icons.Filled.Warning, null, tint = Tokens.Colors.noticeText, modifier = Modifier.size(Tokens.Size.iconS))
                Spacer(Modifier.width(Tokens.Space.m))
                Text(
                    pluralStringResource(R.plurals.outbox_warn, model.outboxPending, model.outboxPending),
                    color = Tokens.Colors.noticeText,
                    fontSize = Tokens.Font.body,
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
                fontSize = Tokens.Font.caption,
                modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.s),
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
                        SectionTitle(stringResource(R.string.requests_header, model.contactRequests.size))
                    }
                    items(model.contactRequests, key = { "req_${it.key}" }) { r ->
                        ContactRequestCard(model, r)
                    }
                }
                // Приглашения в группы — карточками (фильтр Комнаты/all).
                if (showRoom && model.groupInvites.isNotEmpty()) {
                    item(key = "ginv_header") {
                        SectionTitle(stringResource(R.string.group_invites_header, model.groupInvites.size))
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
                // Папка, в которую ничего не попало, — словами, а не пустым экраном:
                // пустота читается как «список не загрузился».
                if (rows.isEmpty()) {
                    item(key = "empty") {
                        Box(Modifier.fillParentMaxHeight(0.6f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                            Text(stringResource(R.string.folder_empty), color = HoloSecondary, fontSize = Tokens.Font.body)
                        }
                    }
                }
                item(key = "tail") { Spacer(Modifier.height(Tokens.Size.fabClearance)) } // место под FAB
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
    val bg = if (row.pinned) Tokens.Colors.rowPinned else Tokens.Colors.surface
    CardBlock(first, last, bg) {
        if (!first) {
            HorizontalDivider(
                thickness = 1.dp,
                color = Tokens.Colors.divider,
                modifier = if (fullDivider) {
                    Modifier
                } else {
                    // Обычный разделитель начинается за аватаром: он отделяет строки,
                    // а не рассекает список.
                    Modifier.padding(start = AvatarRowInset)
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
                    SquareAvatar(row.title, size = Tokens.Size.avatarRow, bg = Tokens.Colors.groupAvatar) {
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
                        Modifier.height(Tokens.Size.chatRowLine1),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            row.title,
                            color = Color.White,
                            fontSize = Tokens.Font.label,
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
                                modifier = Modifier.size(Tokens.Size.iconXs),
                            )
                        }
                        if (row.muted) {
                            Spacer(Modifier.width(Tokens.Space.xs))
                            Icon(
                                TablerIcons.BellOff,
                                null,
                                tint = Tokens.Colors.tertiary,
                                modifier = Modifier.size(Tokens.Size.iconXs),
                            )
                        }
                    }
                    Spacer(Modifier.height(Tokens.Space.xxs))
                    Row(Modifier.height(Tokens.Size.chatRowLine2), verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            chatPreview(model, row),
                            color = Tokens.Colors.secondary,
                            fontSize = Tokens.Font.body,
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
                    Box(Modifier.height(Tokens.Size.chatRowLine1), contentAlignment = Alignment.CenterEnd) {
                        Text(
                            fmtListTime(row.time),
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.caption,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                    Spacer(Modifier.height(Tokens.Space.xxs))
                    // Нижний слот занимает место ВСЕГДА, даже пустой: иначе время
                    // прыгало бы по вертикали в зависимости от наличия счётчика.
                    Box(Modifier.height(Tokens.Size.chatRowLine2), contentAlignment = Alignment.CenterEnd) {
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
        // Упоминание меня — отдельный значок «@» перед счётчиком, и всегда акцентный,
        // даже в чате без звука: ради этого человека и звали.
        row.unread > 0 && row.mentioned -> Row(verticalAlignment = Alignment.CenterVertically) {
            CountBadge("@")
            Spacer(Modifier.width(Tokens.Space.xs))
            CountBadge(badgeCount(row.unread), muted = row.muted)
        }
        row.unread > 0 -> CountBadge(badgeCount(row.unread), muted = row.muted)
        // Те же значки, что в пузыре чата: часы → одна галочка → две. Один и тот же
        // факт не должен выглядеть в списке иначе, чем внутри переписки.
        s != null && s.lastMine && !s.lastSystem -> when (s.lastStatus) {
            "delivered" -> Icon(
                Icons.Filled.DoneAll,
                stringResource(R.string.status_delivered),
                tint = HoloAccent,
                modifier = Modifier.size(Tokens.Size.iconXs),
            )
            "sent" -> Icon(
                Icons.Filled.Done,
                stringResource(R.string.status_sent),
                tint = Tokens.Colors.tertiary,
                modifier = Modifier.size(Tokens.Size.iconXs),
            )
            "pending", "queued" -> Icon(
                Icons.Filled.Schedule,
                stringResource(R.string.status_sending),
                tint = Tokens.Colors.tertiary,
                modifier = Modifier.size(Tokens.Size.iconXs),
            )
            else -> Unit
        }
        row.inRoom > 0 -> Icon(
            Icons.Filled.Headset,
            null,
            tint = HoloOnline,
            modifier = Modifier.size(Tokens.Size.iconXs),
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
        s.lastText.isNotBlank() -> Mentions.plain(s.lastText, s.lastMentions, model::mentionName)
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

/**
 * Карточка-решение над списком: входящий запрос в контакты или приглашение в группу.
 * Одна раскладка на обе — раньше это были две копии с одинаковыми отступами, и
 * поправить одну значило забыть про вторую.
 */
@Composable
private fun DecisionCard(
    avatar: @Composable () -> Unit,
    title: @Composable () -> Unit,
    subtitle: String,
    subtitleLines: Int = 1,
    actions: @Composable RowScope.() -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs)
            .clip(RoundedCornerShape(Tokens.Radius.card))
            .background(HoloSurface)
            .padding(Tokens.Space.m),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            avatar()
            Spacer(Modifier.width(Tokens.Space.m))
            Column(Modifier.weight(1f)) {
                title()
                if (subtitle.isNotBlank()) {
                    Text(
                        subtitle,
                        color = HoloSecondary,
                        fontSize = Tokens.Font.body,
                        maxLines = subtitleLines,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        Spacer(Modifier.height(Tokens.Space.m))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
            content = actions,
        )
    }
}

/** Карточка входящего запроса в контакты (гейт вместо TOFU): аватар-буква, короткий
 *  ключ, превью первого сообщения и кнопки Принять / Отклонить / Заблокировать. */
@Composable
private fun ContactRequestCard(model: AppModel, r: ContactRequest) {
    DecisionCard(
        avatar = { SquareAvatar(r.key, size = Tokens.Size.avatarRow, circle = true) },
        title = {
            Text(
                r.shortKey + "…",
                color = Color.White,
                fontSize = Tokens.Font.label,
                fontFamily = FontFamily.Monospace,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        subtitle = r.preview,
        subtitleLines = 2,
    ) {
        AccentButton(stringResource(R.string.req_accept)) { model.acceptRequest(r.key) }
        OutlineButton(stringResource(R.string.req_decline), tint = HoloSecondary) { model.declineRequest(r.key) }
        Spacer(Modifier.weight(1f))
        TextButton(onClick = { model.blockRequest(r.key) }) {
            Text(stringResource(R.string.req_block), color = HoloError)
        }
    }
}

/** Карточка входящего приглашения в группу (вместо тихого авто-входа): аватар-иконка,
 *  имя группы, кто пригласил, кнопки Принять / Отклонить. */
@Composable
private fun GroupInviteCard(model: AppModel, inv: GroupInvite) {
    DecisionCard(
        avatar = {
            SquareAvatar(inv.name.ifBlank { "#" }, size = Tokens.Size.avatarRow, bg = Tokens.Colors.groupAvatar) {
                Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(Tokens.Size.iconM))
            }
        },
        title = {
            Text(
                inv.name.ifBlank { stringResource(R.string.group_default_name) },
                color = Color.White,
                fontSize = Tokens.Font.label,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        subtitle = stringResource(R.string.group_invite_from, model.peerName(inv.inviter)),
    ) {
        if (inv.waiting) {
            // Согласие уже дано, но пригласивший офлайн: карточка остаётся и честно
            // говорит, чего ждём. Раньше она просто исчезала, и вступление молча
            // терялось — повторить было нечем.
            Text(stringResource(R.string.group_invite_waiting), color = HoloSecondary, fontSize = Tokens.Font.body)
        } else {
            AccentButton(stringResource(R.string.group_invite_join)) { model.acceptGroupInvite(inv.gid) }
            OutlineButton(stringResource(R.string.req_decline), tint = HoloSecondary) { model.declineGroupInvite(inv.gid) }
        }
    }
}

/** Отступ разделителя у строки с аватаром: поле карточки + аватар + зазор до текста.
 *  Разделитель начинается под текстом — он отделяет строки, а не рассекает список. */
private val AvatarRowInset = Tokens.Space.m + Tokens.Size.avatarRow + Tokens.Space.m

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
            ContactCardRow(model, c, first = i == 0, last = i == model.contacts.lastIndex) { onOpen(c.key) }
        }
        item { Spacer(Modifier.height(Tokens.Space.l)) }
    }
}

/**
 * Строка контакта в карточке-списке: аватар, имя с отметкой проверки, статус. Одна
 * на «Контакты» и «Новый чат» — там была её копия, уже без отметки проверки.
 */
@Composable
internal fun LazyItemScope.ContactCardRow(model: AppModel, c: Contact, first: Boolean, last: Boolean, onClick: () -> Unit) {
    val online = model.online[c.key] == true
    CardBlock(first, last) {
        CardRow(onClick = onClick, divider = !first, inset = AvatarRowInset) {
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
                        fontSize = Tokens.Font.label,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (c.verified) {
                        Spacer(Modifier.width(Tokens.Space.xs))
                        Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(Tokens.Size.iconXs))
                    }
                }
                Text(
                    stringResource(if (online) R.string.status_online else R.string.status_offline),
                    color = if (online) HoloOnline else Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.small,
                )
            }
        }
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
        if (msg.isNotEmpty()) Text(msg, color = HoloError, fontSize = Tokens.Font.body)
    }
}

@Composable
internal fun MeScreen(model: AppModel, navigate: (Screen) -> Unit = {}) {
    val me = model.me ?: return
    // В QR — код обмена, а не голый ключ: после смены адреса по одной лишь
    // личности до нас не достучаться (ADR-007).
    // Самоназвание едет в коде — сменили имя, код другой.
    val code = remember(me.pubkey, model.selfName) { model.myContactCode() }
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
            // Аватар меняется прямо здесь: именно тут человек на него смотрит, и
            // искать смену в настройках никто не догадывается.
            EditableAvatar(model.selfName.ifBlank { me.pubkey }, model.selfAvatar, rememberAvatarPicker(model))
            Spacer(Modifier.height(Tokens.Space.m))
            Text(
                model.selfName.ifBlank { stringResource(R.string.common_you) },
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = FontWeight.Medium,
            )
            Text(
                pluralStringResource(R.plurals.peers_online, model.netPeers, model.netPeers),
                color = if (model.netPeers > 0) HoloOnline else Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
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
                        modifier = Modifier.size(Tokens.Size.qr).clip(RoundedCornerShape(Tokens.Radius.tile)),
                    )
                    Spacer(Modifier.height(Tokens.Space.m))
                }
                Text(
                    stringResource(R.string.me_qr_hint),
                    color = Tokens.Colors.secondary,
                    fontSize = Tokens.Font.small,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
            // Копирование — своей строкой под QR: это то же действие «отдать код»,
            // только текстом. Иконка в строке адреса копировала не адрес, а код.
            // Копируется ТОТ ЖЕ код, что в QR: голый ключ после смены адреса уже не
            // ведёт к нам (ADR-007), а «Добавить» принимает и то, и другое.
            CardRow(onClick = { copyToClipboard(ctx, code, "exchange code") }, inset = 0.dp) {
                Icon(Icons.Filled.ContentCopy, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconM))
                Spacer(Modifier.width(Tokens.Space.m))
                Text(stringResource(R.string.me_copy_key), color = Color.White, fontSize = Tokens.Font.label)
            }
            CardRow(inset = 0.dp) {
                Column(Modifier.weight(1f)) {
                    FieldLabel(stringResource(R.string.me_address_label))
                    Text(
                        me.address,
                        color = Color.White,
                        fontSize = Tokens.Font.small,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
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
                    fontSize = Tokens.Font.caption,
                    modifier = Modifier.padding(top = Tokens.Space.s),
                )
                Text(
                    me.fingerprint,
                    color = HoloAccent,
                    fontSize = Tokens.Font.body,
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
                // Та же закладка, что у «Сохранённого» в пересылке: замок обещал
                // шифрование, которого у раздела нет отдельно от остального.
                Triple(Icons.Filled.Bookmark, stringResource(R.string.menu_saved), Screen.Saved),
                Triple(Icons.Filled.Storage, stringResource(R.string.menu_storage), Screen.Storage),
                Triple(Icons.Filled.Settings, stringResource(R.string.menu_settings), Screen.Settings),
            )
            items.forEachIndexed { i, (icon, label, dest) ->
                CardRow(onClick = { navigate(dest) }, divider = i > 0) {
                    Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconM))
                    Spacer(Modifier.width(Tokens.Space.m))
                    Text(label, color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
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
            stringResource(R.string.app_name) + " · " + appVersion(),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.caption,
            modifier = Modifier.padding(Tokens.Space.l),
        )
    }
}
