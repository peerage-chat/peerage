package chat.peerage

import android.media.AudioRecord

/**
 * Что делать захвату звука после очередного чтения микрофона (R-110). `AudioRecord`,
 * вернувший `ERROR_DEAD_OBJECT` (отвалилась BT-гарнитура, перезапустился аудио-сервер),
 * мёртв навсегда: каждое следующее чтение сразу вернёт ту же ошибку. Его надо
 * пересоздать — с растущей паузой, чтобы не молотить, пока устройство не вернулось.
 * Прочие ошибки — пауза в кадр вместо холостого цикла.
 */
class MicRecovery {
    sealed interface Step {
        /** Читать дальше. */
        data object Go : Step

        /** Подождать и читать снова. */
        data class Wait(val ms: Long) : Step

        /** Закрыть микрофон, через [afterMs] открыть заново. */
        data class Reopen(val afterMs: Long) : Step
    }

    private var deadStreak = 0

    /** [read] — что вернул `AudioRecord.read`. */
    fun after(read: Int): Step = when {
        read > 0 -> {
            deadStreak = 0
            Step.Go
        }
        read == AudioRecord.ERROR_DEAD_OBJECT ->
            Step.Reopen((FIRST_REOPEN_MS shl deadStreak++.coerceAtMost(MAX_SHIFT)).coerceAtMost(MAX_REOPEN_MS))
        else -> Step.Wait(FRAME_MS)
    }

    private companion object {
        const val FRAME_MS = 20L
        const val FIRST_REOPEN_MS = 100L
        const val MAX_REOPEN_MS = 2_000L

        /** Дальше сдвиг всё равно упрётся в [MAX_REOPEN_MS] — и не переполнит Long. */
        const val MAX_SHIFT = 5
    }
}
