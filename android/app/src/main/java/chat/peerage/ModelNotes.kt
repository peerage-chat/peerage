package chat.peerage

/**
 * «Сохранённое» — заметки самому себе.
 *
 * Вынесено из `AppModel` не по формальному признаку, а потому что это отдельная
 * тема: свой экран, свой путь в движке (`notes`/`saveNote`), и с перепиской она
 * пересекается только типом строки. Само состояние ([AppModel.notes]) осталось в
 * модели — снапшот-список обязан жить в объекте, за которым наблюдает Compose,
 * а сюда переехало поведение.
 */

fun AppModel.refreshNotes() = ops.execute {
    val list = decodeList<MessageDto>(Native.notes(sessionPtr)).map { it.toModel() }
    notes.replaceWith(list)
    atChatStart -= "notes"
}

/**
 * Догрузить заметки ВЫШЕ показанных. «Сохранённое» — тот же чат (с самим собой),
 * поэтому и страницы те же: движок отдаёт хвост, остальное просим по прокрутке.
 */
fun AppModel.loadOlderNotes(done: (Int) -> Unit) {
    val meKey = me?.pubkey
    val top = notes.firstOrNull { it.row != 0L }
    if (meKey == null || top == null || "notes" in atChatStart || !loadingOlder.add("notes")) {
        done(0)
        return
    }
    ops.execute {
        try {
            val page = decodeList<MessageDto>(
                Native.historyPage(sessionPtr, meKey, top.time, top.row),
            ).map { it.toModel() }
            if (page.isEmpty()) atChatStart += "notes"
            if (page.isNotEmpty()) notes.addAll(0, page)
            done(page.size)
        } finally {
            loadingOlder -= "notes"
        }
    }
}

fun AppModel.saveNote(text: String) = ops.execute {
    Native.saveNote(sessionPtr, text)
    refreshNotes()
}

fun AppModel.deleteNote(id: Long) = ops.execute {
    Native.deleteNote(sessionPtr, id)
    refreshNotes()
}

/** Удалить выделенные заметки (чат с самим собой). */
fun AppModel.deleteSelectedNotes() {
    val refs = selection.toList()
    if (refs.isEmpty()) return
    selection = emptySet()
    val key = me?.pubkey ?: return
    val json = wireDmRefs(refs)
    ops.execute {
        Native.deleteMessages(sessionPtr, key, json)
        val list = decodeList<MessageDto>(Native.notes(sessionPtr)).map { it.toModel() }
        notes.replaceWith(list)
    }
}
