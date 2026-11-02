package chat.peerage

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Поиск по переписке.
 *
 * Своя тема со своим экраном: движок ищет по всем чатам разом и отдаёт находки
 * вместе с адресом (какой чат, какое сообщение), а модель только раскладывает
 * результат. С лентой открытого чата это не пересекается никак.
 */

/**
 * Искать по переписке. Пустой запрос очищает результат: показывать «всё подряд»
 * на пустой строке — не поиск.
 */
fun AppModel.searchMessages(query: String, chat: String = "", group: Boolean = false) = reads.execute {
    if (query.isBlank()) {
        searchHits.clear()
        return@execute
    }
    val hits = decodeList<SearchHitDto>(
        Native.searchMessages(sessionPtr, query, chat, group, 100),
    ).map { AppModel.SearchHit(it.chat, it.group, it.id, it.sender, it.text, it.mine, it.time, it.row) }
    searchHits.replaceWith(hits)
}

// ── Поиск в чате (app-ui.md → «Поиск в чате», шаг 2.9) ──────────────────────

/** Находок за один запрос к движку; дальше — листанием. */
private const val CHAT_HITS_PAGE = 50

/**
 * Поиск в открытом чате. Находки — свежие первыми, приходят страницами; [current] —
 * какая из них показана в ленте («2 из 12» — `current + 1` из [total]).
 */
class ChatSearch(val chat: ChatTarget) {
    var query by mutableStateOf("")

    /** Только сообщения этого человека (hex ключа); пусто — от всех. */
    var sender by mutableStateOf("")

    /** «Списком» вместо «в ленте». */
    var asList by mutableStateOf(false)
    var total by mutableIntStateOf(0)
    val hits = mutableStateListOf<Message>()
    var current by mutableIntStateOf(0)

    /** Запрос и фильтр, на которые пришли [hits]; null — ещё не ответили. */
    var answered by mutableStateOf<Pair<String, String>?>(null)

    /** Ответ на ТЕКУЩИЙ запрос уже пришёл (иначе «ничего не найдено» мигало бы на вводе). */
    val settled: Boolean get() = answered == (query.trim() to sender)

    /** Слова запроса для подсветки — так же, как их ищет движок ([searchStem]). */
    val words: List<String>
        get() = query.split(' ').filter { it.isNotBlank() }.map { searchStem(it.lowercase()) }

    /** Находка, показанная в ленте. */
    val shown: MsgRef? get() = hits.getOrNull(current)?.let { MsgRef(it.id, it.sender) }

    /** Открытая шторка: календарь или выбор участника. */
    var sheet by mutableStateOf<SearchSheet?>(null)

    /** Вместо ленты — панель поиска: пустой запрос (уточнения), «списком» или пусто. */
    val coversFeed: Boolean get() = query.isBlank() || asList || (settled && total == 0)
}

enum class SearchSheet { Date, Member }

/**
 * Слово запроса без окончания — то же правило, что у движка (`search_stem` в
 * `core/src/store.rs`): у слов от пяти букв отброшена последняя гласная, «й» или «ь».
 * Клиенту оно нужно для подсветки: найденное движком должно и подсветиться.
 */
internal fun searchStem(word: String): String =
    if (word.codePointCount(0, word.length) >= 5 && word.last().lowercaseChar() in "аеёиоуыэюяйьaeiouy") {
        word.dropLast(1)
    } else {
        word
    }

fun AppModel.openChatSearch(chat: ChatTarget) {
    chatSearch = ChatSearch(chat)
}

/** Искать заново: сменился запрос или фильтр. В ленте сразу встаём на самую свежую находку. */
fun AppModel.runChatSearch(s: ChatSearch) {
    val q = s.query.trim()
    val sender = s.sender
    if (q.isEmpty()) {
        s.hits.clear()
        s.total = 0
        s.answered = null
        return
    }
    reads.execute {
        val page = decodeOrNull<SearchPageDto>(
            Native.searchChat(sessionPtr, s.chat.id, s.chat.group, q, sender, 0, CHAT_HITS_PAGE),
        ) ?: SearchPageDto()
        // Пока искали, человек мог дописать запрос — ответ на старый не нужен.
        if (s.query.trim() != q || s.sender != sender) return@execute
        s.hits.replaceWith(page.hits.map { it.toModel() })
        s.total = page.total
        s.current = 0
        s.answered = q to sender
        if (!s.asList) s.hits.firstOrNull()?.let { showAt(s.chat, it.time, it.row, MsgRef(it.id, it.sender)) }
    }
}

/** Следующая страница находок; [then] — уже после того, как она легла. */
fun AppModel.moreChatHits(s: ChatSearch, then: () -> Unit = {}) {
    if (s.hits.size >= s.total) return
    val (q, sender) = s.query.trim() to s.sender
    reads.execute {
        val page = decodeOrNull<SearchPageDto>(
            Native.searchChat(sessionPtr, s.chat.id, s.chat.group, q, sender, s.hits.size, CHAT_HITS_PAGE),
        ) ?: return@execute
        if (s.query.trim() != q || s.sender != sender) return@execute
        val shown = s.hits.mapTo(HashSet()) { it.id to it.sender }
        s.hits.addAll(page.hits.map { it.toModel() }.filter { (it.id to it.sender) !in shown })
        then()
    }
}

/** Перейти к находке [index] (0 — самая свежая) и показать её в ленте. */
fun AppModel.goToHit(s: ChatSearch, index: Int) {
    if (index < 0 || index >= s.total) return
    if (index >= s.hits.size) {
        moreChatHits(s) { if (index < s.hits.size) goToHit(s, index) }
        return
    }
    s.current = index
    val m = s.hits[index]
    showAt(s.chat, m.time, m.row, MsgRef(m.id, m.sender))
}

/** Дни месяца с сообщениями (календарь «Перейти к дате»): `ГГГГ-ММ-ДД` по местному времени. */
fun AppModel.messageDays(chat: ChatTarget, from: Long, to: Long, offset: Long, done: (Set<String>) -> Unit) =
    reads.execute { done(decodeList<String>(Native.messageDays(sessionPtr, chat.id, chat.group, from, to, offset)).toSet()) }
