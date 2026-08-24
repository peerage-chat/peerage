package dev.p2p.messenger

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
    storageChats = st.chats.map { Triple(it.peer, it.files, it.bytes) }
}

/** Удалить сохранённые вложения. История и ссылки остаются — байты можно
 *  скачать у собеседника заново (блобы контент-адресные). */
fun AppModel.clearAttachments() = ops.execute {
    Native.clearBlobs(sessionPtr)
    mediaBlobs.clear()
    // Учёт объёма и порядок обращений — часть того же кэша: без сброса они
    // остались бы считать давно удалённые байты.
    synchronized(blobOrder) {
        blobOrder.clear()
        blobBytes = 0
    }
    dev.p2p.messenger.ui.Bitmaps.clear()
    contactAvatars.clear()
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
