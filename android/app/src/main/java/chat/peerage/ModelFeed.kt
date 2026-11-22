package chat.peerage

import androidx.compose.runtime.mutableStateListOf
import kotlinx.serialization.Serializable

/**
 * Переходы по ленте открытого чата (UI 5.7, макет `jump/Capsule`): что ждёт под капсулой
 * «❤ · @ · вниз» и куда вернуться после перехода по цитате. Живёт, пока чат открыт.
 *
 * Отметка уходит из очереди, когда её сообщение попало на экран ([AppModel.sawMessages]):
 * счётчик обещает то, чего человек ещё не видел, а не то, к чему он не переходил кнопкой.
 */
class FeedMarks {
    /** Мои сообщения с новыми чужими реакциями — по порядку ленты (движок, признак просмотра). */
    val reactions = mutableStateListOf<FeedSpot>()

    /** Упоминания меня, ещё не попавшие на экран (только группы). */
    val mentions = mutableStateListOf<FeedSpot>()

    /** Чужое, пришедшее, пока лента прокручена от хвоста: число на «вниз». */
    val below = mutableStateListOf<MsgRef>()

    /** Стек возвратов: ответы, с которых уходили по цитате. «Вниз» снимает верхний. */
    val quoteBack = mutableStateListOf<FeedSpot>()
}

/** Место в ленте: ссылка и где оно в переписке — чтобы открыть окно, если его нет в ленте. */
data class FeedSpot(val ref: MsgRef, val time: Long, val row: Long)

/** Подсветка ряда после перехода; [reaction] — обвести и чужие реакции на нём. */
data class Flash(val ref: MsgRef, val reaction: Boolean = false)

@Serializable
internal data class UnseenReactionDto(val id: Long, val sender: String, val time: Long = 0, val row: Long = 0)

private fun Message.spot() = FeedSpot(MsgRef(id, sender), time, row)

/** Непросмотренные реакции открытого чата — из движка, на [AppModel.ops]. */
internal fun AppModel.loadUnseenReactions(chat: ChatTarget) = ops.execute {
    val list = decodeList<UnseenReactionDto>(Native.unseenReactions(sessionPtr, chat.id, chat.group))
        .map { FeedSpot(MsgRef(it.id, it.sender), it.time, it.row) }
    runOnMain {
        if (openChatTarget() != chat) return@runOnMain
        feedMarks.reactions.clear()
        feedMarks.reactions.addAll(list)
    }
}

/**
 * Упоминания меня среди [unread] последних чужих сообщений группы — то, что пришло, пока
 * чат был закрыт. Зовётся после чтения истории, на [AppModel.ops].
 */
internal fun AppModel.collectUnreadMentions(gid: String, unread: Int) {
    if (unread <= 0) return
    val me = me?.pubkey ?: return
    val spots = groupLog(gid).filter { !it.mine && !it.system }.takeLast(unread)
        .filter { m -> m.mentions.any { it.key == me } }
        .map { it.spot() }
    runOnMain {
        if (currentGroup != gid) return@runOnMain
        feedMarks.mentions.clear()
        feedMarks.mentions.addAll(spots)
    }
}

/** Пришло в открытый чат: ниже экрана — в «вниз», упоминание меня — в «@». */
internal fun AppModel.noteIncoming(m: Message, group: Boolean) {
    if (feedAtBottom) return
    feedMarks.below += MsgRef(m.id, m.sender)
    if (group && m.mentions.any { it.key == me?.pubkey }) feedMarks.mentions += m.spot()
}

/** Сообщения попали на экран: снять их с капсулы; реакции — просмотрены и в движке. */
internal fun AppModel.sawMessages(chat: ChatTarget, refs: Set<MsgRef>) {
    val marks = feedMarks
    marks.below.removeAll { it in refs }
    marks.mentions.removeAll { it.ref in refs }
    val seen = marks.reactions.filter { it.ref in refs }
    if (seen.isEmpty()) return
    marks.reactions.removeAll(seen)
    ops.execute { seen.forEach { Native.markReactionsSeen(sessionPtr, chat.id, chat.group, it.ref.id, it.ref.sender) } }
}

/** «❤» — к самой ранней непросмотренной реакции, ряд подсвечен, реакции обведены. */
internal fun AppModel.jumpToReaction(chat: ChatTarget) {
    val s = feedMarks.reactions.firstOrNull() ?: return
    goToSpot(chat, s, reaction = true)
}

/** «@» — к самому раннему невиденному упоминанию. */
internal fun AppModel.jumpToMention(chat: ChatTarget) {
    val s = feedMarks.mentions.firstOrNull() ?: return
    goToSpot(chat, s)
}

/**
 * Тап по цитате: место ответа — в стек, лента — к исходному. Исходного нет в ленте
 * (удалено у себя) — переходить некуда.
 */
internal fun AppModel.followQuote(chat: ChatTarget, from: Message, to: Message) {
    feedMarks.quoteBack += from.spot()
    goToSpot(chat, to.spot())
}

/** «Вниз» со стеком — назад к ответу, с которого ушли. false — стек пуст, дело ленты. */
internal fun AppModel.popQuoteBack(chat: ChatTarget): Boolean {
    val s = feedMarks.quoteBack.removeLastOrNull() ?: return false
    goToSpot(chat, s)
    return true
}

private fun AppModel.goToSpot(chat: ChatTarget, s: FeedSpot, reaction: Boolean = false) {
    flash = Flash(s.ref, reaction)
    showAt(chat, s.time, s.row, s.ref)
}

/** Чат, открытый сейчас (личный или группа); null — ни одного. */
internal fun AppModel.openChatTarget(): ChatTarget? =
    currentGroup?.let { ChatTarget(it, group = true) } ?: currentChat?.let { ChatTarget(it, group = false) }

/**
 * Лента чата появилась на экране: накопленное за время, пока его не было видно (профиль,
 * главная), прочитано; упоминания из него — на «@».
 */
internal fun AppModel.startViewing(chat: ChatTarget) {
    viewing = chat
    if (foreground) appContext?.let { MessageNotifier.cancel(it, chat) }
    if (chat.group) {
        val n = groupUnread[chat.id] ?: 0
        if (n == 0) return
        groupUnread[chat.id] = 0
        groupMentioned.remove(chat.id)
        ops.execute { collectUnreadMentions(chat.id, n) }
    } else {
        unread[chat.id] = 0
        if (foreground) ops.execute { Native.markRead(sessionPtr, chat.id) }
    }
}

/** Лента ушла с экрана. Чужой [chat] не трогаем: новая лента могла появиться раньше. */
internal fun AppModel.stopViewing(chat: ChatTarget) {
    if (viewing == chat) viewing = null
}
