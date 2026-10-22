package dev.p2p.messenger

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
    ).map { AppModel.SearchHit(it.chat, it.group, it.id, it.sender, it.text, it.mine, it.time) }
    searchHits.clear()
    searchHits.addAll(hits)
}
