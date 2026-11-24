package chat.peerage.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.delay

/**
 * Общий поиск (макет `misc/Search`, app-ui.md → «Второстепенные экраны»): поле в шапке;
 * ниже «Контакты и группы» по имени и «Сообщения · N» с подсветкой совпадения. Пустой
 * запрос — «Недавнее» (последние чаты). Поиск по переписке делает движок (индекс в общей
 * базе), запрос никуда не уходит. Находка ведёт к самому сообщению.
 */
@Composable
internal fun SearchTopBar(model: AppModel, onBack: () -> Unit) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    // Движок — на каждое изменение строки, с паузой на набор: индекс быстрый, но сотня
    // запросов на одно слово не нужна.
    LaunchedEffect(model.globalQuery) {
        delay(SEARCH_DEBOUNCE_MS)
        model.searchMessages(model.globalQuery.trim())
    }
    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(Tokens.Size.topBar)
            .padding(start = Tokens.Space.s, end = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        PlainIcon(TablerIcons.ChevronLeft, stringResource(R.string.common_back), onClick = onBack)
        Row(
            Modifier.weight(1f).height(Tokens.Size.searchField)
                .clip(RoundedCornerShape(Tokens.Radius.field))
                .background(Tokens.Colors.group)
                .padding(start = Tokens.Space.m + Tokens.Space.xxs, end = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s + Tokens.Space.xxs),
        ) {
            Icon(TablerIcons.Search, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
            Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                if (model.globalQuery.isEmpty()) {
                    Text(stringResource(R.string.search_hint), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label, maxLines = 1)
                }
                BasicTextField(
                    model.globalQuery,
                    { model.globalQuery = it },
                    singleLine = true,
                    textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.label),
                    cursorBrush = SolidColor(Tokens.Colors.accent),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                )
            }
            if (model.globalQuery.isNotEmpty()) {
                PlainIcon(TablerIcons.X, stringResource(R.string.search_clear), tint = Tokens.Colors.fieldIcon, small = true) {
                    model.globalQuery = ""
                }
            }
        }
    }
}

@Composable
internal fun SearchScreen(model: AppModel, onOpen: (ChatTarget) -> Unit) {
    BackHandler(model.globalQuery.isNotEmpty()) { model.globalQuery = "" }
    val q = model.globalQuery.trim().lowercase()
    if (q.isEmpty()) {
        RecentChats(model, onOpen)
        return
    }
    val contacts = model.contacts.filter {
        it.displayName.lowercase().contains(q) || it.profileName.lowercase().contains(q) || it.key.lowercase().startsWith(q)
    }
    val groups = model.groups.filter { it.name.lowercase().contains(q) }
    val hits = model.searchHits
    if (contacts.isEmpty() && groups.isEmpty() && hits.isEmpty()) {
        EmptyState(TablerIcons.Search, stringResource(R.string.search_empty), stringResource(R.string.search_empty_hint))
        return
    }
    val words = q.split(' ').filter { it.isNotBlank() }.map { searchStem(it) }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = Tokens.Space.s, bottom = Tokens.Space.xxl)) {
        val people = contacts.map { ChatTarget(it.key, group = false) } + groups.map { ChatTarget(it.gid, group = true) }
        if (people.isNotEmpty()) {
            item(key = "people_title") { SectionTitle(stringResource(R.string.search_people)) }
            item(key = "people") {
                CardSection {
                    people.forEachIndexed { i, t -> ChatRow(model, t, divider = i > 0) { onOpen(t) } }
                }
            }
        }
        if (hits.isNotEmpty()) {
            item(key = "hits_title") { SectionTitle(stringResource(R.string.search_messages, hits.size)) }
            item(key = "hits") {
                CardSection {
                    hits.forEachIndexed { i, hit ->
                        HitRow(model, hit, words, divider = i > 0) {
                            val t = ChatTarget(hit.chat, hit.group)
                            onOpen(t)
                            // После открытия: открытие сбрасывает цель и читает хвост, а окно
                            // вокруг находки встаёт следом в той же очереди.
                            model.showAt(t, hit.time, hit.row, MsgRef(hit.id, hit.sender))
                        }
                    }
                }
            }
            item(key = "caption") {
                Text(
                    stringResource(R.string.search_local_caption),
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                    lineHeight = Tokens.Font.lineHeight,
                    modifier = Modifier.padding(start = Tokens.Space.panelText, end = Tokens.Space.panelText, top = Tokens.Space.s),
                )
            }
        }
    }
}

/** Пустой запрос: последние чаты — чаще всего ищут того, с кем только что говорили. */
@Composable
private fun RecentChats(model: AppModel, onOpen: (ChatTarget) -> Unit) {
    val recent = model.chatSummaries.values
        .filter { it.lastTime > 0 && !model.isSaved(it.id.takeUnless { _ -> it.group }) }
        .sortedByDescending { it.lastTime }
        .take(RECENT_CHATS)
        .map { ChatTarget(it.id, it.group) }
    if (recent.isEmpty()) {
        CenterMessage(stringResource(R.string.search_prompt))
        return
    }
    Column(Modifier.fillMaxSize().padding(top = Tokens.Space.s)) {
        SectionTitle(stringResource(R.string.search_recent))
        CardSection {
            recent.forEachIndexed { i, t -> ChatRow(model, t, divider = i > 0) { onOpen(t) } }
        }
    }
}

/** Строка чата: контакт — кругом и с состоянием сети, группа — квадратом и с числом участников. */
@Composable
private fun ChatRow(model: AppModel, t: ChatTarget, divider: Boolean, onClick: () -> Unit) {
    val (name, avatar) = chatFace(model, t)
    val sub = if (t.group) {
        val n = model.groups.firstOrNull { it.gid == t.id }?.members ?: 0
        pluralStringResource(R.plurals.search_group_subtitle, n, n)
    } else {
        presenceLabel(model.presence(t.id))
    }
    SearchRow(divider, onClick, avatar = { RowAvatar(name, avatar, t.group) }) {
        Text(name, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, maxLines = 1, modifier = Modifier.padding(top = Tokens.Space.xxs))
    }
}

/** Находка: чат и когда, ниже — кусок текста с подсвеченным совпадением. */
@Composable
private fun HitRow(model: AppModel, hit: AppModel.SearchHit, words: List<String>, divider: Boolean, onClick: () -> Unit) {
    val saved = !hit.group && model.isSaved(hit.chat)
    val (name, avatar) = if (saved) stringResource(R.string.menu_saved) to null else chatFace(model, ChatTarget(hit.chat, hit.group))
    SearchRow(divider, onClick, avatar = { RowAvatar(name, avatar, hit.group, saved) }) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                name,
                color = Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(hitDay(hit.time), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, maxLines = 1)
        }
        Text(
            snippet(hit.text, words),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(top = Tokens.Space.xxs),
        )
    }
}

@Composable
internal fun SearchRow(
    divider: Boolean,
    onClick: () -> Unit,
    avatar: @Composable () -> Unit,
    text: @Composable ColumnScope.() -> Unit,
) {
    CardRow(onClick = onClick, divider = divider, inset = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m + Tokens.Space.xxs) {
        avatar()
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(
            Modifier.weight(1f).heightIn(min = Tokens.Size.personRow - Tokens.Space.l),
            verticalArrangement = Arrangement.Center,
            content = text,
        )
    }
}

@Composable
internal fun RowAvatar(name: String, bytes: ByteArray?, group: Boolean, saved: Boolean = false) {
    if (saved) {
        SquareAvatar(name, size = Tokens.Size.avatarPick, bg = Tokens.Colors.accentMuted) {
            Icon(TablerIcons.Bookmark, null, tint = Tokens.Colors.accentSoft, modifier = Modifier.size(Tokens.Size.iconRow))
        }
        return
    }
    SquareAvatar(
        name,
        size = Tokens.Size.avatarPick,
        avatarBytes = bytes,
        circle = !group,
        bg = if (group) Tokens.Colors.groupAvatar else null,
    )
}

/**
 * Кусок текста вокруг первого совпадения: длинное сообщение с совпадением в конце иначе
 * обрезалось бы раньше, чем дошло до него. Подсвечено само совпадение — то, что набрано.
 */
private fun snippet(text: String, words: List<String>): AnnotatedString {
    val flat = text.replace('\n', ' ')
    val lower = flat.lowercase()
    if (lower.length != flat.length) return AnnotatedString(flat)
    val ranges = words.flatMap { w -> matchStarts(lower, w).map { it until it + w.length } }.sortedBy { it.first }
    val first = ranges.firstOrNull()?.first ?: 0
    // Начало — с границы слова за пару слов до совпадения.
    val from = if (first <= SNIPPET_LEAD) 0 else flat.lastIndexOf(' ', first - SNIPPET_LEAD).let { if (it < 0) 0 else it + 1 }
    val lead = if (from > 0) "…" else ""
    return buildAnnotatedString {
        append(lead)
        append(flat.substring(from))
        val shift = lead.length - from
        ranges.filter { it.first >= from }.forEach {
            addStyle(SpanStyle(background = Tokens.Colors.searchMark, color = Color.White), it.first + shift, it.last + 1 + shift)
        }
    }
}

/** Где слово запроса стоит началом слова текста — так движок и ищет. */
private fun matchStarts(lower: String, word: String): List<Int> {
    val out = mutableListOf<Int>()
    var at = lower.indexOf(word)
    while (at >= 0) {
        if (at == 0 || !lower[at - 1].isLetterOrDigit()) out += at
        at = lower.indexOf(word, at + 1)
    }
    return out
}

/** «14:22» сегодня, «вчера», «28 окт» в этом году, «28.10.2025» — раньше. */
@Composable
private fun hitDay(ts: Long): String {
    val nowSec = System.currentTimeMillis() / 1000
    val date = java.util.Date(ts * 1000)
    return when (dayKey(ts)) {
        dayKey(nowSec) -> java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(date)
        dayKey(nowSec - 86_400) -> stringResource(R.string.search_yesterday)
        else -> if (yearOf(ts) == yearOf(nowSec)) {
            java.text.SimpleDateFormat("d MMM", java.util.Locale.getDefault()).format(date).trimEnd('.')
        } else {
            java.text.SimpleDateFormat("dd.MM.yyyy", java.util.Locale.getDefault()).format(date)
        }
    }
}

private const val RECENT_CHATS = 8
private const val SNIPPET_LEAD = 24
