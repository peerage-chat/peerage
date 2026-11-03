package chat.peerage

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.Serializable

/**
 * «Медиа и файлы» чата (app-ui.md, шаг 3.2).
 *
 * Движок отдаёт вкладку страницами — свежие первыми — и счётчики всех вкладок разом;
 * модель только держит загруженное. Лента чата тут ни при чём: экран листает всю
 * историю вложений, а не то, что успело подгрузиться в ленту.
 */

/** Вкладка экрана; [wire] — имя вкладки в движке (`MediaTab::parse`). */
enum class MediaTab(val wire: String) {
    Media("media"),
    Files("files"),
    Voice("voice"),
    Links("links"),
}

@Serializable
internal data class MediaPageDto(
    val total: Int = 0,
    val items: List<MessageDto> = emptyList(),
)

/** Сколько в каждой вкладке. */
@Serializable
data class MediaCounts(
    val media: Int = 0,
    val files: Int = 0,
    val voice: Int = 0,
    val links: Int = 0,
) {
    val all: Int get() = media + files + voice + links

    fun of(tab: MediaTab) = when (tab) {
        MediaTab.Media -> media
        MediaTab.Files -> files
        MediaTab.Voice -> voice
        MediaTab.Links -> links
    }
}

/** Загруженная часть одной вкладки. */
class MediaList {
    val items = mutableStateListOf<Message>()
    var total by mutableIntStateOf(0)

    /** Первая страница уже пришла (иначе «пусто» мигало бы до ответа). */
    var loaded by mutableStateOf(false)
    internal var loading = false
}

/** Открытый экран «Медиа и файлы»: чат, вкладка и загруженное по вкладкам. */
class MediaBrowse(val chat: ChatTarget, tab: MediaTab) {
    var tab by mutableStateOf(tab)
    val lists = MediaTab.entries.associateWith { MediaList() }
    var counts by mutableStateOf<MediaCounts?>(null)
}

/** Сколько сообщений просить у движка за раз. */
private const val MEDIA_PAGE = 60

/** Плотность сетки «Медиа»: столбцов в ряду (щипок и «⋮» → «Плотность»). */
val MEDIA_COLUMNS = listOf(2, 3, 5)

fun AppModel.openMedia(chat: ChatTarget, tab: MediaTab = MediaTab.Media) {
    val b = MediaBrowse(chat, tab)
    mediaBrowse = b
    refreshMediaCounts(chat) { b.counts = it }
    moreMedia(b, tab)
}

/** Следующая страница вкладки (первая — если ещё не было). */
fun AppModel.moreMedia(b: MediaBrowse, tab: MediaTab) {
    val list = b.lists.getValue(tab)
    if (list.loading || (list.loaded && list.items.size >= list.total)) return
    list.loading = true
    val from = list.items.size
    reads.execute {
        val page = decodeOrNull<MediaPageDto>(
            Native.mediaPage(sessionPtr, b.chat.id, b.chat.group, tab.wire, from, MEDIA_PAGE),
        ) ?: MediaPageDto()
        val shown = list.items.mapTo(HashSet()) { it.id to it.sender }
        list.items.addAll(page.items.map { it.toModel() }.filter { (it.id to it.sender) !in shown })
        list.total = page.total
        list.loaded = true
        list.loading = false
    }
}

/** Счётчики вкладок чата (шапка экрана, строка профиля, разделы пустого поиска). */
fun AppModel.refreshMediaCounts(chat: ChatTarget, done: (MediaCounts) -> Unit) = reads.execute {
    done(decodeOrNull<MediaCounts>(Native.mediaCounts(sessionPtr, chat.id, chat.group)) ?: MediaCounts())
}

/** Первые снимки чата для ряда превью в профиле. */
fun AppModel.mediaPreview(chat: ChatTarget, count: Int, done: (List<Message>) -> Unit) = reads.execute {
    val page = decodeOrNull<MediaPageDto>(
        Native.mediaPage(sessionPtr, chat.id, chat.group, MediaTab.Media.wire, 0, count),
    ) ?: MediaPageDto()
    done(page.items.map { it.toModel() })
}

/** Сменить плотность сетки и запомнить её. */
fun AppModel.setMediaColumns(n: Int) {
    if (n !in MEDIA_COLUMNS) return
    mediaColumns = n
    appContext?.getSharedPreferences("settings", android.content.Context.MODE_PRIVATE)
        ?.edit()?.putInt("media_columns", n)?.apply()
}
