@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.peerage.AppModel
import chat.peerage.ChatSearch
import chat.peerage.ChatTarget
import chat.peerage.HoloBg
import chat.peerage.MediaCounts
import chat.peerage.MediaTab
import chat.peerage.Message
import chat.peerage.MsgRef
import chat.peerage.R
import chat.peerage.SearchSheet
import chat.peerage.Tokens
import chat.peerage.goToHit
import chat.peerage.messageDays
import chat.peerage.moreChatHits
import chat.peerage.refreshMediaCounts
import chat.peerage.runChatSearch
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.util.Calendar
import java.util.Locale
import java.util.TimeZone

/*
 * Поиск в чате (app-ui.md → «Поиск в чате», макеты `chatsearch/`). Поле встаёт на место
 * шапки; пустой запрос — уточнения; находки — в ленте (подсветка, «N из M», ↑ ↓) или
 * списком по дням; «Перейти к дате» — шторка-календарь. Ищет движок (журнал движка, 2.9а).
 */

/** Пауза после ввода перед запросом: не искать на каждую букву подряд. */
private const val SEARCH_DEBOUNCE_MS = 250L

/** Что подсветить в пузырях: слова запроса и находка, показанная сейчас (ярче). */
internal data class SearchMarks(val words: List<String>, val current: MsgRef?)

/** Подсветка ленты, пока открыт поиск «в ленте»; null — поиска нет. */
internal val LocalSearchMarks = compositionLocalOf<SearchMarks?> { null }

internal fun searchMarks(s: ChatSearch?): SearchMarks? =
    if (s == null || s.asList || !s.settled || s.total == 0) null else SearchMarks(s.words, s.shown)

/**
 * Подсветить совпадения: слово запроса в начале слова текста — и до конца этого слова
 * («книг» → «книгу»). Так же движок и ищет, началом слова.
 */
internal fun AnnotatedString.marked(marks: SearchMarks?, m: Message): AnnotatedString {
    if (marks == null || marks.words.isEmpty()) return this
    val lower = text.lowercase()
    // Строчные буквы иной длины (редкие символы) сдвинули бы диапазоны — тогда без подсветки.
    if (lower.length != text.length) return this
    val ranges = marks.words.flatMap { matchRanges(lower, it) }
    if (ranges.isEmpty()) return this
    val current = marks.current?.let { it.id == m.id && it.sender == m.sender } == true
    val style = if (current) {
        SpanStyle(background = Tokens.Colors.accent, color = Tokens.Colors.accentDark)
    } else {
        SpanStyle(background = Tokens.Colors.searchMark, color = Color.White)
    }
    return buildAnnotatedString {
        append(this@marked)
        ranges.forEach { addStyle(style, it.first, it.last + 1) }
    }
}

private fun matchRanges(lower: String, word: String): List<IntRange> {
    val out = mutableListOf<IntRange>()
    var at = lower.indexOf(word)
    while (at >= 0) {
        if (at == 0 || !lower[at - 1].isLetterOrDigit()) {
            var end = at + word.length
            while (end < lower.length && lower[end].isLetterOrDigit()) end++
            out += at until end
        }
        at = lower.indexOf(word, at + 1)
    }
    return out
}

// ── Шапка ─────────────────────────────────────────────────────────────────────

/** Поле поиска на месте шапки чата: назад · поле (участник-фильтр, запрос, очистить) · календарь. */
@Composable
internal fun ChatSearchTopBar(model: AppModel, s: ChatSearch) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    LaunchedEffect(s.query, s.sender) {
        delay(SEARCH_DEBOUNCE_MS)
        model.runChatSearch(s)
    }
    BackHandler { model.chatSearch = null }
    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(Tokens.Size.topBar)
            .padding(start = Tokens.Space.s, end = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        PlainIcon(TablerIcons.ChevronLeft, stringResource(R.string.search_close)) { model.chatSearch = null }
        Row(
            Modifier.weight(1f).height(Tokens.Size.searchField)
                .clip(RoundedCornerShape(Tokens.Radius.button))
                .background(Tokens.Colors.panel)
                .padding(start = Tokens.Space.m, end = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            Icon(TablerIcons.Search, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconS))
            if (s.sender.isNotEmpty()) SenderToken(senderName(model, s.sender)) { s.sender = "" }
            Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                if (s.query.isEmpty()) {
                    Text(stringResource(R.string.search_in_chat), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name, maxLines = 1)
                }
                BasicTextField(
                    s.query,
                    { s.query = it },
                    singleLine = true,
                    textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.name),
                    cursorBrush = SolidColor(Tokens.Colors.accent),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                )
            }
            if (s.query.isNotEmpty()) {
                PlainIcon(TablerIcons.X, stringResource(R.string.search_clear), tint = Tokens.Colors.fieldIcon, small = true) { s.query = "" }
            }
        }
        // На пустом запросе календарь — строкой в уточнениях, в шапке он лишний.
        if (s.query.isNotBlank()) {
            BarIcon(
                TablerIcons.Calendar,
                stringResource(R.string.search_go_date),
                tint = if (s.sheet == SearchSheet.Date) Tokens.Colors.accentSoft else Tokens.Colors.iconBar,
            ) { s.sheet = SearchSheet.Date }
        }
    }
}

/** Кнопка-иконка без плашки (назад в поиске, очистить поле). Зона касания — полная. */
@Composable
private fun PlainIcon(icon: ImageVector, desc: String, tint: Color = Tokens.Colors.iconBar, small: Boolean = false, onClick: () -> Unit) {
    Box(
        Modifier.size(if (small) Tokens.Size.searchClear else Tokens.Size.touch)
            .clip(CircleShape)
            .clickable(role = Role.Button, onClickLabel = desc) { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, desc, tint = tint, modifier = Modifier.size(if (small) Tokens.Size.iconS else Tokens.Size.iconM))
    }
}

/** Фильтр по участнику внутри поля: «Борис ×». */
@Composable
private fun SenderToken(name: String, onClear: () -> Unit) {
    Row(
        Modifier.height(Tokens.Size.reactChip).clip(RoundedCornerShape(Tokens.Radius.inner))
            .background(Tokens.Colors.accentMuted)
            .clickable(role = Role.Button, onClickLabel = stringResource(R.string.search_all_members)) { onClear() }
            .padding(start = Tokens.Space.s + Tokens.Space.xxs, end = Tokens.Space.xs + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        Text(name, color = Tokens.Colors.accentSoft, fontSize = Tokens.Font.preview, fontWeight = FontWeight.Medium, maxLines = 1)
        Icon(TablerIcons.X, null, tint = Tokens.Colors.accentSoft, modifier = Modifier.size(Tokens.Size.iconXs))
    }
}

@Composable
private fun senderName(model: AppModel, key: String): String =
    if (key == model.me?.pubkey) stringResource(R.string.common_you) else model.peerName(key)

// ── Панель вместо ленты ───────────────────────────────────────────────────────

/** Что встаёт на место ленты: уточнения (пустой запрос), находки списком или «ничего не найдено». */
@Composable
internal fun ChatSearchPanel(model: AppModel, s: ChatSearch, modifier: Modifier = Modifier, onMedia: (MediaTab) -> Unit) {
    Column(modifier.fillMaxWidth()) {
        when {
            s.query.isBlank() -> SearchStart(model, s, onMedia)
            s.settled && s.total == 0 -> {
                SearchChips(model, s)
                SearchNothing(model, s)
            }
            else -> {
                SearchChips(model, s)
                SearchHitList(model, s)
            }
        }
    }
}

/**
 * Пустой запрос (макет `chatsearch/Main`): «Уточнить», разделы «Медиа и файлы» со счётом
 * (ведут на свой экран сразу на нужной вкладке) и пояснение, что запрос никуда не уходит.
 */
@Composable
private fun SearchStart(model: AppModel, s: ChatSearch, onMedia: (MediaTab) -> Unit) {
    var counts by remember(s.chat) { mutableStateOf<MediaCounts?>(null) }
    LaunchedEffect(s.chat) { model.refreshMediaCounts(s.chat) { counts = it } }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = Tokens.Space.xs)) {
        SectionTitle(stringResource(R.string.search_refine))
        CardSection {
            if (s.chat.group) {
                OptionRow(TablerIcons.User, stringResource(R.string.search_from_member), null, divider = false) {
                    s.sheet = SearchSheet.Member
                }
            }
            OptionRow(TablerIcons.Calendar, stringResource(R.string.search_go_date), null, divider = s.chat.group) {
                s.sheet = SearchSheet.Date
            }
        }
        SectionTitle(stringResource(R.string.profile_media))
        CardSection {
            listOf(
                Triple(MediaTab.Media, TablerIcons.Photo, R.string.search_media_photos),
                Triple(MediaTab.Files, TablerIcons.File, R.string.media_tab_files),
                Triple(MediaTab.Links, TablerIcons.Link, R.string.media_tab_links),
                Triple(MediaTab.Voice, TablerIcons.Microphone, R.string.media_tab_voice),
            ).forEachIndexed { i, (tab, icon, label) ->
                OptionRow(icon, stringResource(label), sub = null, value = counts?.of(tab)?.toString(), divider = i > 0) { onMedia(tab) }
            }
        }
        Text(
            stringResource(R.string.search_local_note),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(horizontal = Tokens.Space.panelText, vertical = Tokens.Space.m),
        )
    }
}

/** Строка-уточнение: иконка без плашки, название с пояснением, шеврон. */
@Composable
private fun OptionRow(icon: ImageVector, label: String, sub: String?, value: String? = null, divider: Boolean, onClick: () -> Unit) {
    if (divider) {
        HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = OptionInset))
    }
    Row(
        Modifier.fillMaxWidth().clickable(role = Role.Button) { onClick() }
            .heightIn(min = Tokens.Size.cardRow)
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs, vertical = Tokens.Space.rowV),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Icon(icon, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
        Column(Modifier.weight(1f)) {
            Text(label, color = Color.White, fontSize = Tokens.Font.name)
            if (sub != null) Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
        }
        if (value != null) Text(value, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
        Icon(TablerIcons.ChevronRight, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconS))
    }
}

/** Отступ разделителя строки-уточнения: от края до текста. */
private val OptionInset = Tokens.Space.m + Tokens.Space.xxs + Tokens.Size.iconRow + Tokens.Space.m + Tokens.Space.xxs

/** Фильтры над находками: «От участника» (группа) и «Дата». */
@Composable
private fun SearchChips(model: AppModel, s: ChatSearch) {
    Row(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.m),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        if (s.chat.group) {
            if (s.sender.isEmpty()) {
                FilterChip(stringResource(R.string.search_from_member), TablerIcons.ChevronDown, on = false) { s.sheet = SearchSheet.Member }
            } else {
                FilterChip(stringResource(R.string.search_from_chip, senderName(model, s.sender)), TablerIcons.X, on = true) { s.sender = "" }
            }
        }
        FilterChip(stringResource(R.string.search_date_chip), TablerIcons.ChevronDown, on = false) { s.sheet = SearchSheet.Date }
    }
}

@Composable
private fun FilterChip(text: String, trailing: ImageVector, on: Boolean, onClick: () -> Unit) {
    val fg = if (on) Tokens.Colors.accentSoft else Tokens.Colors.chipText
    Row(
        Modifier.height(Tokens.Size.searchChip).clip(RoundedCornerShape(percent = 50))
            .background(if (on) Tokens.Colors.accentMuted else Tokens.Colors.group)
            .clickable(role = Role.Button) { onClick() }
            .padding(start = Tokens.Space.m + Tokens.Space.xxs, end = Tokens.Space.s + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
    ) {
        Text(text, color = fg, fontSize = Tokens.Font.preview, maxLines = 1)
        Icon(trailing, null, tint = fg, modifier = Modifier.size(Tokens.Size.iconXs))
    }
}

/** Ничего не найдено (макет `chatsearch/Empty`); с фильтром — предложение искать у всех. */
@Composable
private fun SearchNothing(model: AppModel, s: ChatSearch) {
    Column(
        Modifier.fillMaxSize().padding(horizontal = Tokens.Space.xxxl + Tokens.Space.s),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(TablerIcons.Search, null, tint = Tokens.Colors.faint, modifier = Modifier.size(Tokens.Size.iconHero))
        Spacer(Modifier.height(Tokens.Space.m))
        Text(stringResource(R.string.search_nothing), color = Color.White, fontSize = Tokens.Font.name, fontWeight = FontWeight.Medium)
        if (s.sender.isNotEmpty()) {
            Spacer(Modifier.height(Tokens.Space.s))
            Text(
                stringResource(R.string.search_nothing_from, senderName(model, s.sender)),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                textAlign = TextAlign.Center,
            )
            Spacer(Modifier.height(Tokens.Space.m))
            SearchPill(stringResource(R.string.search_all_members)) { s.sender = "" }
        }
        Spacer(Modifier.height(Tokens.Size.fabClearance))
    }
}

/** Пилюля-действие поиска («Списком», «Искать у всех участников»). */
@Composable
private fun SearchPill(text: String, icon: ImageVector? = null, bg: Color = Tokens.Colors.group, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Row(
        modifier.height(Tokens.Size.ib).clip(RoundedCornerShape(Tokens.Radius.button)).background(bg)
            .clickable(role = Role.Button) { onClick() }
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s, Alignment.CenterHorizontally),
    ) {
        if (icon != null) Icon(icon, null, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconS))
        Text(text, color = Color.White, fontSize = Tokens.Font.preview, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** Строка ленивого списка находок: подпись дня или сама находка (её место в группе дня). */
private sealed interface HitEntry {
    data class Day(val label: Long) : HitEntry
    data class Hit(val index: Int, val first: Boolean, val last: Boolean) : HitEntry
}

/** Находки списком по дням (макет `chatsearch/List`); тап — находка в ленте. */
@Composable
private fun SearchHitList(model: AppModel, s: ChatSearch) {
    val state = rememberLazyListState()
    val entries = buildList {
        s.hits.forEachIndexed { i, m ->
            val day = dayKey(m.time)
            val prevDay = s.hits.getOrNull(i - 1)?.let { dayKey(it.time) }
            val nextDay = s.hits.getOrNull(i + 1)?.let { dayKey(it.time) }
            if (prevDay != day) add(HitEntry.Day(m.time))
            add(HitEntry.Hit(i, first = prevDay != day, last = nextDay != day))
        }
    }
    LaunchedEffect(s) {
        snapshotFlow { state.layoutInfo.visibleItemsInfo.lastOrNull()?.index to state.layoutInfo.totalItemsCount }
            .collect { (last, count) -> if (last != null && last >= count - 4) model.moreChatHits(s) }
    }
    val marks = SearchMarks(s.words, null)
    LazyColumn(state = state, modifier = Modifier.fillMaxSize()) {
        items(entries, key = { e -> if (e is HitEntry.Hit) s.hits[e.index].let { "${it.sender}:${it.id}" } else "d${(e as HitEntry.Day).label}" }) { e ->
            when (e) {
                is HitEntry.Day -> SectionTitle(dayLabel(e.label))
                is HitEntry.Hit -> CardBlock(first = e.first, last = e.last) {
                    if (!e.first) HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = HitInset))
                    HitRow(model, s.hits[e.index], marks) {
                        s.asList = false
                        model.goToHit(s, e.index)
                    }
                }
            }
        }
        item { Spacer(Modifier.height(Tokens.Space.m)) }
    }
}

private val HitInset = Tokens.Space.m + Tokens.Space.xxs + Tokens.Size.avatarHit + Tokens.Space.m

@Composable
private fun HitRow(model: AppModel, m: Message, marks: SearchMarks, onClick: () -> Unit) {
    val mine = m.mine
    val name = if (mine) stringResource(R.string.common_you) else model.peerName(m.sender)
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }
            .padding(horizontal = Tokens.Space.m + Tokens.Space.xxs, vertical = Tokens.Space.panel),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        SquareAvatar(
            if (mine) model.selfName else name,
            size = Tokens.Size.avatarHit,
            avatarBytes = if (mine) model.selfAvatar else model.peerAvatar(m.sender),
            circle = true,
        )
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                Text(
                    name,
                    color = Color.White,
                    fontSize = Tokens.Font.label,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(fmtMsgTime(m.time), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.small, style = Tokens.Text.tabular)
            }
            // Нашлось по имени файла — показываем его: текста у вложения может не быть.
            Text(
                AnnotatedString(m.text.ifBlank { m.name }).marked(marks, m),
                color = Tokens.Colors.snippet,
                fontSize = Tokens.Font.preview,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

// ── Нижняя полоса ─────────────────────────────────────────────────────────────

/** Счётчик и переключатель вида; в ленте — ещё ↑ старее и ↓ новее (макет `chatsearch/Feed`). */
@Composable
internal fun ChatSearchBar(model: AppModel, s: ChatSearch) {
    if (s.query.isBlank() || s.total == 0) return
    Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
        HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider)
        Row(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.m, end = Tokens.Space.m, top = Tokens.Space.s, bottom = Tokens.Space.l),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            Text(
                if (s.asList) pluralOf(R.plurals.search_found, s.total.toLong()) else stringResource(R.string.search_count, s.current + 1, s.total),
                color = Color.White,
                fontSize = Tokens.Font.label,
                style = Tokens.Text.tabular,
                modifier = Modifier.weight(1f),
            )
            if (s.asList) {
                SearchPill(stringResource(R.string.search_in_feed), TablerIcons.FeedLines) {
                    s.asList = false
                    model.goToHit(s, s.current)
                }
            } else {
                SearchPill(stringResource(R.string.search_as_list), TablerIcons.ListLines) { s.asList = true }
                StepButton(TablerIcons.ChevronUp, stringResource(R.string.search_prev), enabled = s.current + 1 < s.total) {
                    model.goToHit(s, s.current + 1)
                }
                StepButton(TablerIcons.ChevronDown, stringResource(R.string.search_next), enabled = s.current > 0) {
                    model.goToHit(s, s.current - 1)
                }
            }
        }
    }
}

@Composable
private fun StepButton(icon: ImageVector, desc: String, enabled: Boolean, onClick: () -> Unit) {
    Box(
        Modifier.size(Tokens.Size.ib).clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.group)
            .clickable(enabled = enabled, role = Role.Button, onClickLabel = desc) { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, desc, tint = if (enabled) Tokens.Colors.iconChrome else Tokens.Colors.faint, modifier = Modifier.size(Tokens.Size.iconRow))
    }
}

// ── Шторки ────────────────────────────────────────────────────────────────────

/** Календарь и выбор участника — по [ChatSearch.sheet]. */
@Composable
internal fun ChatSearchSheets(model: AppModel, s: ChatSearch) {
    when (s.sheet) {
        SearchSheet.Date -> DateSheet(
            model,
            s.chat,
            onClose = { s.sheet = null },
            onPick = { time ->
                // Переход к дате — навигация по переписке: поиск закрывается, лента встаёт на день.
                model.chatSearch = null
                model.showAt(s.chat, time, 0)
            },
            onLast = {
                model.chatSearch = null
                model.toTail(s.chat)
            },
        )
        SearchSheet.Member -> MemberSheet(model, s.chat.id, onClose = { s.sheet = null }) { key ->
            s.sheet = null
            s.sender = key
        }
        null -> {}
    }
}

@Composable
private fun SearchSheetFrame(title: String, onClose: () -> Unit, content: @Composable () -> Unit) {
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Tokens.Colors.group,
        tonalElevation = 0.dp,
        shape = RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel),
        dragHandle = { SheetHandle() },
    ) {
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(start = Tokens.Space.l, end = Tokens.Space.l, bottom = Tokens.Space.xl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.panel),
        ) {
            Text(
                title,
                color = Color.White,
                fontSize = Tokens.Font.sheet,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs).semantics { heading() },
            )
            content()
        }
    }
}

/** «Перейти к дате» (макет `chatsearch/Date`): месяц, дни без сообщений приглушены. */
@Composable
private fun DateSheet(model: AppModel, chat: ChatTarget, onClose: () -> Unit, onPick: (Long) -> Unit, onLast: () -> Unit) {
    // Месяц — как (год, месяц 0..11): Calendar, потому что java.time на minSdk 24 без desugaring нет.
    val today = remember { Calendar.getInstance() }
    var year by remember { mutableStateOf(today.get(Calendar.YEAR)) }
    var month by remember { mutableStateOf(today.get(Calendar.MONTH)) }
    var pickMonth by remember { mutableStateOf(false) }
    var days by remember { mutableStateOf(emptySet<String>()) }
    LaunchedEffect(year, month) {
        val from = monthStart(year, month)
        val to = monthStart(year + (month + 1) / 12, (month + 1) % 12)
        val offset = TimeZone.getDefault().getOffset(from.timeInMillis) / 1000L
        days = emptySet()
        model.messageDays(chat, from.timeInMillis / 1000, to.timeInMillis / 1000, offset) { days = it }
    }
    val step = { delta: Int ->
        val m = year * 12 + month + delta
        year = m / 12
        month = m % 12
    }
    SearchSheetFrame(stringResource(R.string.search_go_date), onClose) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            PlainIcon(TablerIcons.ChevronLeft, stringResource(R.string.date_prev)) { if (pickMonth) year-- else step(-1) }
            Row(
                Modifier.weight(1f).height(Tokens.Size.touch).clip(RoundedCornerShape(Tokens.Radius.button))
                    .clickable(role = Role.Button) { pickMonth = !pickMonth },
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs, Alignment.CenterHorizontally),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    if (pickMonth) "$year" else monthTitle(year, month),
                    color = Color.White,
                    fontSize = Tokens.Font.name,
                    fontWeight = FontWeight.Medium,
                )
                Icon(
                    if (pickMonth) TablerIcons.ChevronUp else TablerIcons.ChevronDown,
                    null,
                    tint = Color.White,
                    modifier = Modifier.size(Tokens.Size.iconXs),
                )
            }
            PlainIcon(TablerIcons.ChevronRight, stringResource(R.string.date_next)) { if (pickMonth) year++ else step(1) }
        }
        if (pickMonth) {
            MonthGrid(year, month) {
                month = it
                pickMonth = false
            }
        } else {
            DayGrid(year, month, days, today) { day -> onPick(dayStart(year, month, day)) }
        }
        Row(Modifier.padding(top = Tokens.Space.xs), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
            SearchPill(stringResource(R.string.date_first), bg = Tokens.Colors.raised, modifier = Modifier.weight(1f)) { onPick(0) }
            SearchPill(stringResource(R.string.date_last), bg = Tokens.Colors.raised, modifier = Modifier.weight(1f)) { onLast() }
        }
    }
}

private fun monthStart(year: Int, month: Int): Calendar = Calendar.getInstance().apply {
    clear()
    set(year, month, 1)
}

/** Начало дня по местному времени, секунды. */
private fun dayStart(year: Int, month: Int, day: Int): Long = Calendar.getInstance().run {
    clear()
    set(year, month, day)
    timeInMillis / 1000
}

private fun monthTitle(year: Int, month: Int): String =
    monthName(month) + " " + year

private fun monthName(month: Int): String =
    Calendar.getInstance().apply {
        clear()
        set(2000, month, 1)
    }.getDisplayName(Calendar.MONTH, Calendar.LONG_STANDALONE, Locale.getDefault())
        .orEmpty().replaceFirstChar { it.titlecase(Locale.getDefault()) }

/** Сетка дней месяца с первого дня недели локали. */
@Composable
private fun DayGrid(year: Int, month: Int, days: Set<String>, today: Calendar, onDay: (Int) -> Unit) {
    val first = monthStart(year, month)
    val weekStart = first.firstDayOfWeek
    val lead = (first.get(Calendar.DAY_OF_WEEK) - weekStart + 7) % 7
    val count = first.getActualMaximum(Calendar.DAY_OF_MONTH)
    val isThisMonth = today.get(Calendar.YEAR) == year && today.get(Calendar.MONTH) == month
    Column {
        Row(Modifier.fillMaxWidth()) {
            for (i in 0 until 7) {
                val dow = (weekStart - 1 + i) % 7 + 1
                val name = Calendar.getInstance().apply { set(Calendar.DAY_OF_WEEK, dow) }
                    .getDisplayName(Calendar.DAY_OF_WEEK, Calendar.SHORT_STANDALONE, Locale.getDefault())
                    .orEmpty().replaceFirstChar { it.titlecase(Locale.getDefault()) }
                Text(
                    name,
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.small,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.weight(1f).padding(vertical = Tokens.Space.xs + Tokens.Space.xxs),
                )
            }
        }
        val cells = lead + count
        for (week in 0 until (cells + 6) / 7) {
            Row(Modifier.fillMaxWidth()) {
                for (col in 0 until 7) {
                    val day = week * 7 + col - lead + 1
                    Box(Modifier.weight(1f).height(Tokens.Size.dayCell), contentAlignment = Alignment.Center) {
                        if (day in 1..count) {
                            val has = "%04d-%02d-%02d".format(Locale.ROOT, year, month + 1, day) in days
                            val sel = isThisMonth && today.get(Calendar.DAY_OF_MONTH) == day
                            Box(
                                Modifier.size(Tokens.Size.dayDot).clip(CircleShape)
                                    .background(if (sel) Tokens.Colors.accent else Color.Transparent)
                                    .clickable(enabled = has, role = Role.Button) { onDay(day) },
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(
                                    "$day",
                                    color = when {
                                        sel -> Tokens.Colors.accentDark
                                        has -> Color.White
                                        else -> Tokens.Colors.faint
                                    },
                                    fontSize = Tokens.Font.label,
                                    fontWeight = if (sel) FontWeight.Medium else FontWeight.Normal,
                                    style = Tokens.Text.tabular,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Выбор месяца года (тап по заголовку календаря). */
@Composable
private fun MonthGrid(year: Int, current: Int, onMonth: (Int) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
        for (row in 0 until 4) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                for (col in 0 until 3) {
                    val m = row * 3 + col
                    SearchPill(
                        monthName(m),
                        bg = if (m == current) Tokens.Colors.accentMuted else Tokens.Colors.raised,
                        modifier = Modifier.weight(1f),
                    ) { onMonth(m) }
                }
            }
        }
    }
}

/** «От участника»: кого из группы искать (себя тоже). */
@Composable
private fun MemberSheet(model: AppModel, gid: String, onClose: () -> Unit, onPick: (String) -> Unit) {
    val me = model.me?.pubkey
    // Ростер группы от движка — без себя; свои сообщения тоже ищут, «Вы» — первой строкой.
    val members = listOfNotNull(me) +
        model.groupMemberKeys[gid].orEmpty().filter { it != me }.sortedBy { model.peerName(it).lowercase() }
    SearchSheetFrame(stringResource(R.string.search_from_member), onClose) {
        LazyColumn(Modifier.heightIn(max = Tokens.Size.sheetList)) {
            items(members, key = { it }) { key ->
                val mine = key == me
                val name = if (mine) stringResource(R.string.common_you) else model.peerName(key)
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.button))
                        .clickable(role = Role.Button) { onPick(key) }
                        .padding(horizontal = Tokens.Space.xs, vertical = Tokens.Space.s),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
                ) {
                    SquareAvatar(
                        if (mine) model.selfName else name,
                        size = Tokens.Size.avatarList,
                        avatarBytes = if (mine) model.selfAvatar else model.peerAvatar(key),
                        circle = true,
                        ring = Tokens.Colors.group,
                    )
                    Text(name, color = Color.White, fontSize = Tokens.Font.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

// ── Лента не с хвоста ─────────────────────────────────────────────────────────

/**
 * Подгрузка вниз, когда лента открыта окном посреди переписки (переход к находке или к
 * дате): взгляд у нижнего края — следующая страница. У хвоста ничего не делает.
 */
@Composable
internal fun PagingNewer(model: AppModel, listState: LazyListState, chat: ChatTarget) {
    LaunchedEffect(chat) {
        snapshotFlow { listState.firstVisibleItemIndex to listState.layoutInfo.totalItemsCount }
            .collect { (first, _) -> if (first <= 3) model.loadNewer(chat) }
    }
}

/**
 * Кнопка «вниз» (макет `special/Unread`, пока без счётчика непрочитанных): видна, когда
 * лента прокручена от низа или открыта окном посреди переписки. Окно — возврат к хвосту.
 */
@Composable
internal fun JumpDownButton(model: AppModel, listState: LazyListState, chat: ChatTarget, modifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    val detached = model.detached[model.chatKey(chat.id, chat.group)] == true
    val away by remember { derivedStateOf { listState.firstVisibleItemIndex > 2 } }
    AnimatedVisibility(away || detached, modifier, enter = fadeIn(), exit = fadeOut()) {
        val desc = stringResource(R.string.jump_down)
        Box(
            Modifier.padding(Tokens.Space.l).size(Tokens.Size.jumpDown)
                .shadow(Tokens.Size.menuShadow, CircleShape)
                .clip(CircleShape)
                .background(Tokens.Colors.raised)
                .clickable(role = Role.Button, onClickLabel = desc) {
                    if (detached) model.toTail(chat) else scope.launch { listState.animateScrollToItem(0) }
                },
            contentAlignment = Alignment.Center,
        ) {
            Icon(TablerIcons.ChevronDown, desc, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconM))
        }
    }
}
