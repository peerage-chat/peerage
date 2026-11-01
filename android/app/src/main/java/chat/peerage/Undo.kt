package chat.peerage

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** Действие, которое ещё можно отменить: [text] — строка снекбара. */
class PendingUndo internal constructor(val text: String, internal val commit: () -> Unit, internal val revert: () -> Unit)

/**
 * Слот «Отменить» (app-ui.md → «Общие элементы»): действие только на этом телефоне
 * откладывается на несколько секунд, а видимый результат наступает сразу. Действие
 * выполняется ровно один раз — по истечении, досрочно (новое действие, уход в фон) или
 * никогда (отменили). Слот один: снекбары не копятся.
 */
class UndoSlot {
    var current by mutableStateOf<PendingUndo?>(null)
        private set

    /** Предложить отмену; ожидающее предыдущее выполняется сейчас же. */
    fun offer(text: String, commit: () -> Unit, revert: () -> Unit) {
        flush()
        current = PendingUndo(text, commit, revert)
    }

    /** «Отменить». */
    fun undo() {
        current?.revert?.invoke()
        current = null
    }

    /** Отсчёт [u] истёк. Опоздавший таймер чужого действия ничего не делает. */
    fun expire(u: PendingUndo) {
        if (current === u) flush()
    }

    /** Выполнить ожидающее сейчас. */
    fun flush() {
        val u = current ?: return
        current = null
        u.commit()
    }
}
