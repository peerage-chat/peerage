package dev.p2p.messenger

/**
 * Журнал звонков — история, а не сами звонки.
 *
 * Записи делает движок, здесь только чтение и очистка. Сама подсистема звонков
 * (микрофон, комната, входящее приглашение) живёт в модели и сюда не заходит.
 */

fun AppModel.refreshCallLog() = ops.execute {
    val list = decodeList<CallEntryDto>(Native.callLog(sessionPtr, 200))
        .map { CallEntry(it.peer, it.gid, it.incoming, it.outcome, it.started, it.duration) }
    callLog.replaceWith(list)
}

fun AppModel.clearCallLog() = ops.execute {
    Native.clearCallLog(sessionPtr)
    refreshCallLog()
}
