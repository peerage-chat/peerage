@file:OptIn(
    androidx.compose.material3.ExperimentalMaterial3Api::class,
    androidx.compose.foundation.ExperimentalFoundationApi::class,
)

package chat.peerage.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
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
    /** Режим уведомлений не «все» — значок и серый счётчик. */
    val muted: Boolean = false,
    val pinnedAt: Long = 0,
    val summary: ChatSummary? = null,
    /** Сколько человек сейчас в комнате (только для групп; 0 — никого). */
    val inRoom: Int = 0,
    val members: Int = 0,
    /** «Сохранённое» — чат с собой: своя аватарка, без галочек, сети и уведомлений. */
    val saved: Boolean = false,
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
internal fun ChatsPager(model: AppModel, pager: PagerState, nav: HomeNav) {
    // Состояние пейджера живёт выше (MainActivity): по нему же рисуются табы, так что
    // синхронизировать нечего — и драться двум эффектам тоже нечему.
    val keys = folderKeys(model)
    HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { page ->
        ContactsScreen(model, keys.getOrElse(page) { "all" }, nav)
    }
}

@Composable
internal fun ContactsScreen(
    model: AppModel,
    activeFilter: String,
    nav: HomeNav,
) {
    // Пользовательская папка — это ФИЛЬТР по правилам, встроенные — по типу чата.
    val folder = model.folderById(activeFilter)
    val showDm = folder != null || activeFilter == "all" || activeFilter == "dm"
    val showRoom = folder != null || activeFilter == "all" || activeFilter == "room"

    // Запросы и приглашения — строками над чатами: в «Все», запросы ещё в «Личные»,
    // приглашения — в «Группы»; на вкладке «Запросы» — только они.
    val showReq = activeFilter == "all" || activeFilter == "dm" || activeFilter == RequestsFolder
    val showInv = activeFilter == "all" || activeFilter == "room" || activeFilter == RequestsFolder
    val requests = if (showReq) model.contactRequests.sortedByDescending { it.time } else emptyList()
    val invites = if (showInv) model.groupInvites.toList() else emptyList()
    val dmList = (if (folder == null) model.contacts else model.contacts.filter { model.folderHasDm(folder, it.key) })
        .filterNot { model.isSaved(it.key) }
    // «Сохранённое» — строкой среди чатов, виден всегда: по времени последней заметки, без
    // заметок — в самом низу. В своей папке — если папка берёт личные чаты.
    val me = model.me?.pubkey
    val savedTitle = stringResource(R.string.menu_saved)
    val savedSummary = me?.let { model.chatSummaries[model.chatKey(it, false)] }
    val groupList = if (folder == null) model.groups else model.groups.filter { model.folderHasGroup(folder, it.gid) }

    // Один список: закреплённые сверху (позже закрепили — выше), дальше по времени
    // последнего сообщения. Чаты без переписки уходят в конец по алфавиту — иначе
    // новый пустой контакт с временем 0 встал бы ниже годовалой переписки случайно;
    // пустое «Сохранённое» — под ними.
    val rows = buildList {
        if (showRoom) {
            groupList.forEach { g ->
                val s = model.chatSummaries[model.chatKey(g.gid, true)]
                add(
                    ChatRowUi(
                        id = g.gid, group = true, title = g.name, avatar = model.groupPhoto(g.gid),
                        unread = model.groupUnread[g.gid] ?: 0,
                        mentioned = model.groupMentioned[g.gid] == true,
                        muted = model.chatQuiet(g.gid, true), pinnedAt = s?.pinnedAt ?: 0, summary = s,
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
                        muted = model.chatQuiet(c.key, false), pinnedAt = s?.pinnedAt ?: 0, summary = s,
                    ),
                )
            }
            if (me != null && (folder == null || model.folderHasDm(folder, me))) {
                add(ChatRowUi(id = me, group = false, title = savedTitle, pinnedAt = savedSummary?.pinnedAt ?: 0, summary = savedSummary, saved = true))
            }
        }
    }.sortedWith(
        compareByDescending<ChatRowUi> { it.pinnedAt }
            .thenByDescending { it.time }
            .thenBy { it.saved }
            .thenBy { it.title.lowercase() },
    )

    // Полосы над списком (очередь, копии) — первым элементом списка: уезжают вместе с
    // чатами, а не занимают экран постоянно. Шапка и вкладки остаются на месте.
    val strips: @Composable () -> Unit = {
        QueueStrip(model, nav.openOutbox)
        BackupStripView(model, nav)
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
    }
    when {
        // Совсем новый профиль — пустая главная с действиями; «Сохранённое» и так в профиле.
        model.contacts.isEmpty() && model.groups.isEmpty() && savedSummary == null &&
            model.contactRequests.isEmpty() && model.groupInvites.isEmpty() -> Column(Modifier.fillMaxSize()) {
            strips()
            HomeEmpty(nav)
        }
        else -> LazyColumn(Modifier.fillMaxSize()) {
            item(key = "strips") { Column { strips() } }
            items(requests, key = { "req_${it.key}" }) { r ->
                RequestRow(
                    title = r.shownName,
                    tag = stringResource(R.string.req_tag),
                    lookalike = r.lookalikeKey.isNotEmpty(),
                    preview = r.greeting.ifBlank { stringResource(R.string.req_no_message) },
                    time = r.time,
                    group = false,
                    first = r == requests.first(),
                ) { nav.openRequest(r.key) }
            }
            items(invites, key = { "ginv_${it.gid}" }) { inv ->
                RequestRow(
                    title = inv.name.ifBlank { stringResource(R.string.group_default_name) },
                    tag = stringResource(R.string.invite_tag),
                    preview = stringResource(R.string.group_invite_from, model.peerName(inv.inviter)),
                    time = 0,
                    group = true,
                    first = requests.isEmpty() && inv == invites.first(),
                ) { nav.openInvite(inv.gid) }
            }
            // Один список прямо на панели: личные чаты и группы вперемешку,
            // закреплённые сверху (на ступень светлее).
            itemsIndexed(rows, key = { _, r -> (if (r.group) "g_" else "c_") + r.id }) { i, r ->
                ChatListRow(
                    model = model,
                    row = r,
                    first = i == 0 && requests.isEmpty() && invites.isEmpty(),
                    // Граница между закреплёнными и остальными — разделитель во
                    // всю ширину: он и разделяет группы, и не оставляет дыры.
                    fullDivider = i > 0 && rows[i - 1].pinned && !r.pinned,
                    onClick = { if (r.group) nav.openGroup(r.id) else nav.openDm(r.id) },
                )
            }
            // Папка, в которую ничего не попало, — словами, а не пустым экраном:
            // пустота читается как «список не загрузился».
            if (rows.isEmpty() && requests.isEmpty() && invites.isEmpty()) {
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

/**
 * Строка чата прямо на панели: аватар, имя, превью и справа время со значком.
 *
 * Две линии — «имя … время» и «превью … значок»: время всегда на первой, значок на второй, и
 * высоты у них не фиксированы — при крупном шрифте строка растёт, а не обрезается. Нижняя
 * линия не ниже значка, даже без него: иначе имя прыгало бы от наличия счётчика.
 */
@Composable
private fun ChatListRow(
    model: AppModel,
    row: ChatRowUi,
    first: Boolean,
    fullDivider: Boolean,
    onClick: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    // В две панели открытый справа чат подсвечен (макеты `home/Sheet*`): иначе не видно,
    // чей это лист.
    val open = LocalPaneChat.current?.let { it.id == row.id && it.group == row.group } == true
    val bg = when {
        open -> Tokens.Colors.accentMuted
        row.pinned -> Tokens.Colors.rowPinned
        else -> Tokens.Colors.panel
    }
    Column(Modifier.fillMaxWidth().background(bg)) {
        if (!first) {
            HorizontalDivider(
                thickness = 1.dp,
                color = Tokens.Colors.divider,
                // Обычный разделитель начинается за аватаром: он отделяет строки, а не
                // рассекает список; граница закреплённых — во всю ширину.
                modifier = if (fullDivider) Modifier else Modifier.padding(start = AvatarRowInset),
            )
        }
        Box {
            Row(
                Modifier.fillMaxWidth()
                    .combinedClickable(onClick = onClick, onLongClick = { menu = true })
                    .heightIn(min = Tokens.Size.chatRow)
                    .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (row.saved) {
                    // Сущность, а не человек — скруглённый квадрат с закладкой (макет home/HomeSections).
                    SquareAvatar(row.title, size = Tokens.Size.avatarRow, bg = Tokens.Colors.avatar, ring = bg) {
                        Icon(TablerIcons.Bookmark, null, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconRow))
                    }
                } else {
                    SquareAvatar(
                        row.title,
                        size = Tokens.Size.avatarRow,
                        online = if (row.group) null else row.online,
                        avatarBytes = row.avatar,
                        circle = !row.group, // человек — круг, сущность — скруглённый квадрат
                        ring = bg,
                    )
                }
                Spacer(Modifier.width(Tokens.Space.m))
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                row.title,
                                color = Color.White,
                                fontSize = Tokens.Font.name,
                                fontWeight = FontWeight.Medium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f, fill = false),
                            )
                            if (row.verified) {
                                Spacer(Modifier.width(Tokens.Space.xs))
                                Icon(
                                    Icons.Filled.Verified,
                                    stringResource(R.string.cd_verified),
                                    tint = HoloVerified,
                                    modifier = Modifier.size(Tokens.Size.iconXs),
                                )
                            }
                            if (row.muted) {
                                Spacer(Modifier.width(Tokens.Space.xs))
                                Icon(
                                    TablerIcons.BellOff,
                                    stringResource(R.string.cd_muted),
                                    tint = Tokens.Colors.tertiary,
                                    modifier = Modifier.size(Tokens.Size.iconXs),
                                )
                            }
                        }
                        Spacer(Modifier.width(Tokens.Space.s))
                        Text(
                            fmtListTime(row.time),
                            style = Tokens.Text.meta,
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.small,
                            maxLines = 1,
                            softWrap = false,
                        )
                    }
                    Spacer(Modifier.height(Tokens.Space.xxs))
                    Row(
                        Modifier.heightIn(min = Tokens.Size.badge),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        val typing = !row.group && !row.saved && isTyping(model, row.id)
                        Text(
                            if (typing) stringResource(R.string.status_typing) else chatPreview(model, row),
                            color = if (typing) Tokens.Colors.accent else Tokens.Colors.secondary,
                            fontSize = Tokens.Font.preview,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        Spacer(Modifier.width(Tokens.Space.s))
                        ChatRowBadge(model, row)
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
private fun ChatRowBadge(model: AppModel, row: ChatRowUi) {
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
        // Те же значки, что в пузыре чата: часы → одна галочка → две, прочитанное —
        // акцентом. Один и тот же факт не должен выглядеть в списке иначе, чем в ленте.
        // В превью строка реакции — галочке не к чему относиться.
        s != null && !row.saved && s.reaction == null && s.lastMine && !s.lastSystem && s.lastKind != "deleted" &&
            s.lastKind != "call" && s.lastKind != "video_call" -> when (shownStatus(model, row.id, row.group, s.lastStatus)) {
            "read" -> Icon(
                Icons.Filled.DoneAll,
                stringResource(R.string.status_read),
                tint = HoloAccent,
                modifier = Modifier.size(Tokens.Size.iconXs),
            )
            "delivered" -> Icon(
                Icons.Filled.DoneAll,
                stringResource(R.string.status_delivered),
                tint = Tokens.Colors.tertiary,
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
            "held" -> Icon(
                TablerIcons.Clock,
                stringResource(R.string.status_held),
                tint = Tokens.Colors.heldTick,
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

/** Меню чата по долгому нажатию: закрепить и уведомления. Действий больше двух, и они
 *  разной цены — поэтому меню, а не свайп по строке (горизонталь отдана папкам). */
@Composable
private fun ChatRowMenu(model: AppModel, row: ChatRowUi, open: Boolean, onClose: () -> Unit) {
    val chat = ChatTarget(row.id, row.group)
    var notify by remember(chat) { mutableStateOf(false) }
    if (notify) NotifySheet(model, chat) { notify = false }
    PopMenu(open, onClose) {
        PopMenuItem(
            TablerIcons.Pin,
            stringResource(if (row.pinned) R.string.chat_menu_unpin else R.string.chat_menu_pin),
        ) {
            model.pinChat(row.id, row.group, !row.pinned)
            onClose()
        }
        // Режим виден справа; выбор и срок — в шторке. Себе уведомлений не приходит.
        if (!row.saved) {
            NotifyMenuItem(model, chat) {
                onClose()
                notify = true
            }
        }
    }
}

/**
 * Превью последнего сообщения. В группе перед текстом — имя автора, у своих —
 * «Вы:». Вложение показывается словом («Фото», «Голосовое», имя файла): текста у
 * него нет, а пустая строка выглядела бы как потерянное сообщение.
 */
@Composable
private fun chatPreview(model: AppModel, row: ChatRowUi): String {
    val s = row.summary ?: return stringResource(if (row.saved) R.string.chats_saved_empty else R.string.chats_preview_empty)
    // Реакция на моё новее последнего сообщения — та же строка, что в шторке.
    s.reaction?.let { r ->
        val ctx = LocalContext.current
        return model.peerName(r.by) + ": " + model.reactionText(ctx, r.emoji, r.quote, r.kind)
    }
    // Системная строка — та же, что в ленте. Автором её не подписываем: она и так
    // говорит о ком-то в третьем лице.
    if (s.lastSystem) return groupSystemLine(model, s.lastKind, s.lastSender, s.lastText).text
    val body = when {
        s.lastKind == "deleted" -> stringResource(R.string.msg_deleted)
        // Звонок: исход в тексте — служебное слово, показываем его по-человечески.
        s.lastKind == "call" || s.lastKind == "video_call" ->
            return callTitle(s.lastKind == "video_call", s.lastMine, s.lastText)
        s.lastText.isNotBlank() -> Markdown.plain(Mentions.plain(s.lastText, s.lastMentions, model::mentionName))
        s.lastKind == "image" -> stringResource(R.string.chats_preview_photo)
        s.lastKind == "video" -> stringResource(R.string.chats_preview_video)
        s.lastKind == "voice" -> stringResource(R.string.chats_preview_voice)
        s.lastKind == "file" -> stringResource(R.string.chats_preview_file)
        else -> stringResource(R.string.chats_preview_empty)
    }.replace('\n', ' ')
    val who = when {
        row.saved -> "" // всё в «Сохранённом» — своё
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
/** Куда ведёт главная: чаты, карточки решений и действия пустого состояния и полос. */
internal class HomeNav(
    val openDm: (String) -> Unit,
    val openGroup: (String) -> Unit,
    val openRequest: (String) -> Unit,
    val openInvite: (String) -> Unit,
    val openOutbox: () -> Unit,
    val myCode: () -> Unit,
    val scan: () -> Unit,
    val newGroup: () -> Unit,
    /** Копия ключа: true — с 24 слов (сделать, сверить), false — сразу проверка. */
    val keyFlow: (wordsFirst: Boolean) -> Unit,
    val dataBackup: () -> Unit,
)

/**
 * Полоса очереди над списком: сообщения ждут отправки. Видна, когда нет соединения (уйдут при
 * его появлении) или очередь выросла (уйдут, когда получатели появятся в сети; предупреждение
 * выключается в настройках). Ничего не удаляется. Тап — «Исходящие».
 */
@Composable
private fun QueueStrip(model: AppModel, onOpen: () -> Unit) {
    val n = model.outboxPending
    val offline = model.netPeers == 0
    if (n <= 0 || !(offline || (model.outboxWarn && model.warnOutboxGrowth))) return
    Row(
        Modifier.fillMaxWidth()
            .padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.m, bottom = Tokens.Space.xs)
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(Tokens.Colors.queueStrip)
            .clickable(role = Role.Button) { onOpen() }
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(TablerIcons.Clock, null, tint = Tokens.Colors.queueText, modifier = Modifier.size(Tokens.Size.iconRow))
        Column(Modifier.weight(1f)) {
            Text(
                pluralStringResource(R.plurals.queue_count, n, n),
                color = Tokens.Colors.queueText,
                fontSize = Tokens.Font.preview,
                fontWeight = FontWeight.Medium,
            )
            Text(
                stringResource(if (offline) R.string.queue_offline_sub else R.string.queue_growth_sub),
                color = Tokens.Colors.queueSub,
                fontSize = Tokens.Font.small,
            )
        }
        Icon(TablerIcons.ChevronRight, null, tint = Tokens.Colors.queueSub, modifier = Modifier.size(Tokens.Size.iconS))
    }
}

/**
 * Напоминание о копиях — одно за раз, под полосой очереди (app-ui.md → «Напоминания о
 * копиях»). Какое и когда — решает [chat.peerage.BackupFacts]; «Позже» только откладывает.
 * Янтарные — когда ключ под угрозой, нейтральные — плановое.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun BackupStripView(model: AppModel, nav: HomeNav) {
    val hasChats = model.chatSummaries.values.any { it.lastTime > 0 }
    val strip = model.backupFacts.strip(System.currentTimeMillis() / 1000, hasChats) ?: return
    val amber = strip == BackupStrip.KEY_MISSING || strip == BackupStrip.KEY_CHECK_FAILED
    val title: String
    val text: String
    val primary: String
    val secondary: String
    val onPrimary: () -> Unit
    val onSecondary: () -> Unit
    val later = stringResource(R.string.backup_later)
    when (strip) {
        BackupStrip.KEY_MISSING -> {
            title = stringResource(R.string.backup_key_missing)
            text = stringResource(R.string.backup_key_missing_sub)
            primary = stringResource(R.string.backup_key_make)
            secondary = later
            onPrimary = { nav.keyFlow(true) }
            onSecondary = { model.backupLater(strip) }
        }
        BackupStrip.KEY_CHECK_DUE -> {
            val days = daysSince(model.backupFacts.keyCheckedAt)
            title = stringResource(R.string.backup_check)
            text = pluralStringResource(R.plurals.backup_check_sub, days, days)
            primary = stringResource(R.string.backup_check_go)
            secondary = later
            onPrimary = { nav.keyFlow(false) }
            onSecondary = { model.backupLater(strip) }
        }
        BackupStrip.KEY_CHECK_FAILED -> {
            title = stringResource(R.string.backup_check_failed)
            text = stringResource(R.string.backup_check_failed_sub)
            primary = stringResource(R.string.backup_reconcile)
            secondary = stringResource(R.string.backup_check_again)
            onPrimary = { nav.keyFlow(true) }
            onSecondary = { nav.keyFlow(false) }
        }
        BackupStrip.DATA_STALE -> {
            val at = model.backupFacts.dataBackupAt
            val days = daysSince(at)
            title = if (at == 0L) {
                stringResource(R.string.backup_data_none)
            } else {
                pluralStringResource(R.plurals.backup_data_ago, days, days)
            }
            text = stringResource(R.string.backup_data_sub)
            primary = stringResource(R.string.backup_data_make)
            secondary = later
            onPrimary = nav.dataBackup
            onSecondary = { model.backupLater(strip) }
        }
    }
    val fg = if (amber) Tokens.Colors.queueText else Color.White
    val sub = if (amber) Tokens.Colors.queueSub else Tokens.Colors.secondary
    Column(
        Modifier.fillMaxWidth()
            .padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.m, bottom = Tokens.Space.xs)
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(if (amber) Tokens.Colors.queueStrip else Tokens.Colors.group)
            // Низ и зазор ужаты на запас зоны касания пилюль (48 при видимых 36).
            .padding(start = Tokens.Space.m + Tokens.Space.xxs, end = Tokens.Space.m + Tokens.Space.xxs, top = Tokens.Space.m + Tokens.Space.xxs, bottom = Tokens.Space.s),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m)) {
            Icon(
                when (strip) {
                    BackupStrip.KEY_CHECK_FAILED -> TablerIcons.AlertTriangle
                    BackupStrip.DATA_STALE -> TablerIcons.Database
                    else -> TablerIcons.Key
                },
                null,
                tint = if (amber) Tokens.Colors.stripAmber else Tokens.Colors.accent,
                modifier = Modifier.size(Tokens.Size.iconRow),
            )
            Column(Modifier.weight(1f)) {
                Text(title, color = fg, fontSize = Tokens.Font.preview, fontWeight = FontWeight.Medium)
                Text(
                    text,
                    color = sub,
                    fontSize = Tokens.Font.small,
                    lineHeight = Tokens.Font.lineHeightLoose,
                    modifier = Modifier.padding(top = Tokens.Space.xxs),
                )
            }
        }
        // Переносом, а не обрезкой: «Сверить» и «Проверить ещё раз» на крупном шрифте в строку не встают.
        FlowRow(
            Modifier.padding(start = Tokens.Size.iconRow + Tokens.Space.m, top = Tokens.Space.xs),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            StripPill(
                primary,
                bg = if (amber) Tokens.Colors.stripAmber else Tokens.Colors.accentMuted,
                fg = if (amber) Tokens.Colors.onStripAmber else Tokens.Colors.accentSoft,
                onClick = onPrimary,
            )
            StripPill(secondary, bg = Color.Transparent, fg = sub, onClick = onSecondary)
        }
    }
}

/** Пилюля полосы: видимая высота 36, зона касания — 48. */
@Composable
private fun StripPill(text: String, bg: Color, fg: Color, onClick: () -> Unit) {
    val press = remember { androidx.compose.foundation.interaction.MutableInteractionSource() }
    Box(
        Modifier.minimumInteractiveComponentSize()
            .pressScale(press)
            .heightIn(min = Tokens.Size.pill)
            .clip(RoundedCornerShape(Tokens.Radius.pill))
            .background(bg)
            .clickable(interactionSource = press, indication = null, role = Role.Button) { onClick() }
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = fg, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** Целых дней с момента `at` (unix-секунды). */
private fun daysSince(at: Long): Int = ((System.currentTimeMillis() / 1000 - at) / BackupFacts.DAY).toInt().coerceAtLeast(0)

/**
 * Пустая главная: что это за место и как его наполнить. Контакт добавляется только кодом —
 * поэтому главное действие «Показать мой код», рядом — «Сканировать код».
 */
@Composable
private fun HomeEmpty(nav: HomeNav) {
    Box(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(horizontal = Tokens.Space.xxl, vertical = Tokens.Space.xxl),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.panel)).background(Tokens.Colors.group)
                .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.xxl, bottom = Tokens.Space.xl),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Box(
                Modifier.size(Tokens.Size.avatarProfile).clip(RoundedCornerShape(Tokens.Radius.group))
                    .background(Tokens.Colors.accentMuted),
                contentAlignment = Alignment.Center,
            ) {
                Icon(TablerIcons.Messages, null, tint = Tokens.Colors.accent, modifier = Modifier.size(Tokens.Size.iconXl))
            }
            Text(
                stringResource(R.string.home_empty_title),
                color = Color.White,
                fontSize = Tokens.Font.sheet,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(top = Tokens.Space.l),
            )
            Text(
                stringResource(R.string.home_empty_text),
                color = Tokens.Colors.secondary,
                fontSize = Tokens.Font.preview,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = Tokens.Space.s),
            )
            Spacer(Modifier.height(Tokens.Space.xl))
            AccentButton(stringResource(R.string.home_empty_my_code), Modifier.fillMaxWidth(), onClick = nav.myCode)
            Spacer(Modifier.height(Tokens.Space.s))
            PlateButton(stringResource(R.string.my_code_scan), TablerIcons.Scan, Modifier.fillMaxWidth(), onClick = nav.scan)
            GhostButton(stringResource(R.string.home_empty_group), modifier = Modifier.padding(top = Tokens.Space.xs), onClick = nav.newGroup)
        }
    }
}

/**
 * Строка запроса или приглашения над чатами: чуть другой фон и метка «Запрос» / «Приглашение».
 * Кнопок в строке нет — решение принимается в карточке, где видно всё, что пришло.
 */
@Composable
private fun RequestRow(
    title: String,
    tag: String,
    preview: String,
    time: Long,
    group: Boolean,
    first: Boolean,
    lookalike: Boolean = false,
    onClick: () -> Unit,
) {
    Column(Modifier.fillMaxWidth().background(Tokens.Colors.rowRequest)) {
        if (!first) {
            HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = AvatarRowInset))
        }
        Row(
            Modifier.fillMaxWidth()
                .clickable(role = Role.Button) { onClick() }
                .heightIn(min = Tokens.Size.chatRow)
                .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            SquareAvatar(title, size = Tokens.Size.avatarRow, circle = !group)
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Row(
                        Modifier.weight(1f),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
                    ) {
                        Text(
                            title,
                            color = Color.White,
                            fontSize = Tokens.Font.name,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        // Имя как у контакта (7e) — знак прямо у имени (`requests/Rows`).
                        if (lookalike) {
                            Icon(
                                TablerIcons.AlertTriangle,
                                stringResource(R.string.req_lookalike_short),
                                tint = Tokens.Colors.stripAmber,
                                modifier = Modifier.size(Tokens.Size.iconXs),
                            )
                        }
                        RowTag(tag)
                    }
                    if (time > 0) {
                        Spacer(Modifier.width(Tokens.Space.s))
                        Text(
                            fmtListTime(time),
                            style = Tokens.Text.meta,
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.small,
                            maxLines = 1,
                        )
                    }
                }
                Spacer(Modifier.height(Tokens.Space.xxs))
                Text(
                    preview,
                    color = Tokens.Colors.secondary,
                    fontSize = Tokens.Font.preview,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** Метка у имени в строке: «Запрос», «Приглашение». */
@Composable
private fun RowTag(text: String) {
    Text(
        text,
        color = Tokens.Colors.accentSoft,
        fontSize = Tokens.Font.caption,
        fontWeight = FontWeight.Medium,
        maxLines = 1,
        modifier = Modifier.clip(RoundedCornerShape(Tokens.Radius.badge)).background(Tokens.Colors.accentMuted)
            .padding(horizontal = Tokens.Space.xs + Tokens.Space.xxs, vertical = Tokens.Space.xxs),
    )
}

/** Отступ разделителя у строки с аватаром: поле карточки + аватар + зазор до текста.
 *  Разделитель начинается под текстом — он отделяет строки, а не рассекает список. */
private val AvatarRowInset = Tokens.Space.l + Tokens.Size.avatarRow + Tokens.Space.m

/**
 * «Контакты» (макет `screens/Contacts`): полоса запросов, по алфавиту с буквами-разделителями,
 * у сверенных — щит. Тап → личный чат. [query] не null — над списком поле поиска.
 */
@Composable
internal fun ContactListScreen(
    model: AppModel,
    query: String?,
    onQuery: (String) -> Unit,
    onRequests: () -> Unit,
    onOpen: (String) -> Unit,
) {
    if (model.contacts.isEmpty() && model.contactRequests.isEmpty()) {
        EmptyState(Icons.Filled.Group, stringResource(R.string.contacts_empty_title), stringResource(R.string.contacts_empty_hint))
        return
    }
    val shown = remember(model.contacts.toList(), query) { alphabetical(model.contacts, query.orEmpty()) }
    LazyColumn(Modifier.fillMaxSize().padding(top = Tokens.Space.s)) {
        if (query != null) item(key = "search") { ContactSearch(query, autoFocus = true, onQuery = onQuery) }
        if (query == null && model.contactRequests.isNotEmpty()) {
            item(key = "requests") { RequestsStrip(model.contactRequests, onRequests) }
        }
        shown.forEachIndexed { i, c ->
            val letter = initial(c.displayName)
            val newLetter = i == 0 || initial(shown[i - 1].displayName) != letter
            if (newLetter) item(key = "letter_$letter") { LetterHeader(letter) }
            item(key = c.key) { ContactRow(model, c, divider = !newLetter) { onOpen(c.key) } }
        }
        item { Spacer(Modifier.height(Tokens.Space.l)) }
    }
}

/** Контакты по алфавиту языка телефона; [query] — подстрока имени без учёта регистра. */
private fun alphabetical(contacts: List<Contact>, query: String): List<Contact> {
    val collator = java.text.Collator.getInstance()
    return contacts.filter { query.isBlank() || it.displayName.contains(query.trim(), ignoreCase = true) }
        .sortedWith { a, b -> collator.compare(a.displayName, b.displayName) }
}

/** Буква раздела: первая буква имени заглавной; цифры и знаки — под «#». */
private fun initial(name: String): String {
    val c = name.trim().firstOrNull() ?: return "#"
    return if (c.isLetter()) c.uppercase() else "#"
}

@Composable
private fun LetterHeader(letter: String) {
    Text(
        letter,
        color = Tokens.Colors.accent,
        fontSize = Tokens.Font.body,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m + Tokens.Space.xxs, bottom = Tokens.Space.xs),
    )
}

/** Строка контакта прямо на панели: аватар со статусом, имя, «в сети», щит у сверенного. */
@Composable
private fun ContactRow(model: AppModel, c: Contact, divider: Boolean, onClick: () -> Unit) {
    val online = model.online[c.key] == true
    Column {
        if (divider) {
            HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = ContactRowInset))
        }
        Row(
            Modifier.fillMaxWidth().clickable(role = Role.Button, onClick = onClick)
                .heightIn(min = Tokens.Size.contactRow)
                .padding(horizontal = Tokens.Space.xl),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
        ) {
            SquareAvatar(
                c.displayName,
                size = Tokens.Size.avatarContact,
                // Точка только «в сети» (макет): «не в сети» и так написано строкой ниже.
                online = online.takeIf { it },
                avatarBytes = model.contactAvatars[c.avatarId],
                circle = true,
                ring = Tokens.Colors.panel,
            )
            Column(Modifier.weight(1f)) {
                Text(
                    c.displayName,
                    color = Tokens.Colors.onDark,
                    fontSize = Tokens.Font.name,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    stringResource(if (online) R.string.status_online else R.string.status_offline),
                    color = if (online) HoloOnline else Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                )
            }
            if (c.verified) {
                Icon(
                    TablerIcons.ShieldCheck,
                    stringResource(R.string.profile_key_ok),
                    tint = Tokens.Colors.online,
                    modifier = Modifier.size(Tokens.Size.iconS),
                )
            }
        }
    }
}

/** Разделитель строк контактов начинается под текстом: поле + аватар + зазор. */
private val ContactRowInset = Tokens.Space.xl + Tokens.Size.avatarContact + Tokens.Space.m + Tokens.Space.xxs

/** «Запросы в контакты · N» и чьи (макет `screens/Contacts`); тап — вкладка «Запросы» главной. */
@Composable
private fun RequestsStrip(requests: List<ContactRequest>, onOpen: () -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.xs, bottom = Tokens.Space.s - Tokens.Space.xxs)
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(Tokens.Colors.accentMuted)
            .clickable(role = Role.Button, onClick = onOpen)
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs, vertical = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(TablerIcons.UserPlus, null, tint = Tokens.Colors.accentSoft, modifier = Modifier.size(Tokens.Size.iconRow))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.xxs)) {
            Text(
                stringResource(R.string.contacts_requests_strip, requests.size),
                color = Tokens.Colors.requestStripText,
                fontSize = Tokens.Font.preview,
                fontWeight = FontWeight.Medium,
            )
            Text(
                requests.sortedByDescending { it.time }.joinToString(", ") { it.shownName },
                color = Tokens.Colors.requestStripSub,
                fontSize = Tokens.Font.small,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Icon(TablerIcons.ChevronRight, null, tint = Tokens.Colors.requestStripSub, modifier = Modifier.size(Tokens.Size.iconS))
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

/** Чат, открытый в правой панели (две панели); null — одна панель или ничего не открыто. */
internal val LocalPaneChat = compositionLocalOf<ChatTarget?> { null }
