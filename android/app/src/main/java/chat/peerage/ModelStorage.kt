package chat.peerage

/**
 * Хранилище и очередь исходящих: сколько занято, что не доставлено, что можно
 * почистить.
 *
 * Это «хозяйство», а не переписка: цифры для экрана «Хранилище», список ждущих
 * пиров и кнопки очистки. Живёт отдельно, чтобы не мешаться под руками там, где
 * речь о сообщениях.
 */

fun AppModel.refreshStorage() = ops.execute {
    val st = decodeOrNull<StorageStatsDto>(Native.storageStats(sessionPtr)) ?: return@execute
    storageBlobs = st.blobs
    storageBlobBytes = st.blobBytes
    storageOutboxPending = st.outboxPending
    // Самый тяжёлый тип первым — полоса состава читается сверху вниз.
    storageByKind = st.byKind.toList().sortedByDescending { it.second }
    storageChats = st.chats.map { StorageChat(it.chat, it.group, it.files, it.bytes) }
}

// ── Автоудаление (5.4) ───────────────────────────────────────────────────────
// Только на этом телефоне: правило — движка, здесь лишь его снимок для экранов.

fun AppModel.refreshAutodel() = ops.execute {
    autodel = decodeOrNull<AutodelStateDto>(Native.autodelState(sessionPtr)) ?: AutodelStateDto()
}

/** Своё правило чата: код возраста или `null` — как в общих. */
internal fun AppModel.chatAutodel(chat: ChatTarget): String? =
    autodel.chats.firstOrNull { it.chat == chat.id && it.group == chat.group }?.age

/**
 * Что удалит новое правило [rule] (или своё правило [own] чата [chat] поверх общего).
 * Ответ — на главном потоке: экран по нему решает, нужна ли шторка подтверждения.
 */
internal fun AppModel.previewAutodel(
    rule: AutodelRuleDto,
    chat: ChatTarget? = null,
    own: String? = null,
    done: (PrunedDto) -> Unit,
) = ops.execute {
    val json = WireJson.encodeToString(AutodelRuleDto.serializer(), rule)
    val p = decodeOrNull<PrunedDto>(
        Native.autodelPreview(sessionPtr, json, chat?.id.orEmpty(), chat?.group == true, own.orEmpty()),
    ) ?: PrunedDto()
    android.os.Handler(android.os.Looper.getMainLooper()).post { done(p) }
}

/** Записать общее правило; удаление по нему идёт сразу. */
internal fun AppModel.setAutodel(rule: AutodelRuleDto) = ops.execute {
    Native.setAutodel(sessionPtr, WireJson.encodeToString(AutodelRuleDto.serializer(), rule))
    autodel = decodeOrNull<AutodelStateDto>(Native.autodelState(sessionPtr)) ?: autodel
}

/** Записать своё правило чата ([own] `null` — как в общих). */
internal fun AppModel.setChatAutodel(chat: ChatTarget, own: String?) = ops.execute {
    Native.setChatAutodel(sessionPtr, chat.id, chat.group, own.orEmpty())
    autodel = decodeOrNull<AutodelStateDto>(Native.autodelState(sessionPtr)) ?: autodel
}

private fun ClearScope.json(): String = WireJson.encodeToString(
    ClearFilterDto.serializer(),
    ClearFilterDto(chats.map { ClearChatDto(it.id, it.group) }, kinds.toList()),
)

/** Сколько освободит очистка [scope]; ответ — на главном потоке. */
internal fun AppModel.previewClear(scope: ClearScope, done: (Long) -> Unit) = ops.execute {
    val bytes = Native.clearBlobsPreview(sessionPtr, scope.json())
    android.os.Handler(android.os.Looper.getMainLooper()).post { done(bytes) }
}

/** Удалить сохранённые вложения под [scope]. История и ссылки остаются — байты можно
 *  скачать у собеседника заново (блобы контент-адресные). Аватары и миниатюры
 *  движок оставляет; файл, который показывает и чат вне выбора, — тоже. */
internal fun AppModel.clearAttachments(scope: ClearScope) = ops.execute {
    Native.clearBlobsWhere(sessionPtr, scope.json())
    // Кэш байт — только кэш: сбросить целиком дешевле, чем выяснять, что из него ушло.
    mediaBlobs.clear()
    savedBlobs.clear()
    // Докачки движок бросает лишь при полной очистке; при частичной их прогресс живой.
    if (scope.all) downloads.clear()
    // Пометки «скачано» в открытых лентах — по факту: движок снял их в истории,
    // а уже показанные строки иначе считали бы себя скачанными до перезахода.
    for (list in messages.values) {
        for (i in list.indices) {
            val m = list[i]
            if (m.downloaded && m.blobId.isNotEmpty() && !Native.blobExists(sessionPtr, m.blobId)) {
                list[i] = m.copy(downloaded = false)
            }
        }
    }
    // Учёт объёма и порядок обращений — часть того же кэша: без сброса они
    // остались бы считать давно удалённые байты.
    synchronized(blobOrder) {
        blobOrder.clear()
        blobBytes = 0
    }
    chat.peerage.ui.Bitmaps.clear()
    refreshStorage()
}

fun AppModel.refreshOutbox() = ops.execute {
    outboxByPeer = decodeList<OutboxPeerDto>(Native.outboxByPeer(sessionPtr))
        .map { OutboxPeer(it.peer, it.count, it.bytes, it.oldest) }
    outboxPending = outboxByPeer.sumOf { it.count }
}

/** Сбросить очередь одному получателю (после подтверждения в UI). */
fun AppModel.clearOutboxFor(peer: String) = ops.execute {
    Native.clearOutboxFor(sessionPtr, peer)
    refreshOutbox()
}

/** Сбросить очередь всем (после двойного подтверждения в UI). */
fun AppModel.clearOutboxAll() = ops.execute {
    Native.clearOutboxAll(sessionPtr)
    refreshOutbox()
}
