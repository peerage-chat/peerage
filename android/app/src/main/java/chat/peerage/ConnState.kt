package chat.peerage

/**
 * Что показывает постоянное уведомление «связи» (notifications.md, решения 2, 3, 10): состояние
 * узла и всё непрочитанное — в том числе досланное после офлайна, которое в шторку не идёт.
 * Сравнивается целиком: [ConnService.refresh] перестраивает уведомление только при смене.
 */
data class ConnState(
    val offline: Boolean = false,
    val searching: Boolean = false,
    val messages: Int = 0,
    val chats: Int = 0,
    val mention: Boolean = false,
    val requests: Int = 0,
    val invites: Int = 0,
) {
    sealed interface Part {
        data class Messages(val count: Int, val chats: Int) : Part
        data object Mention : Part
        data class Requests(val count: Int) : Part
        data class Invites(val count: Int) : Part
    }

    /** Части строки непрочитанного по порядку; пусто — нечего показывать. */
    fun parts(): List<Part> = buildList {
        if (messages > 0) add(Part.Messages(messages, chats))
        if (mention) add(Part.Mention)
        if (requests > 0) add(Part.Requests(requests))
        if (invites > 0) add(Part.Invites(invites))
    }

    val hasUnread: Boolean get() = parts().isNotEmpty()
}
