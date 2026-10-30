package chat.peerage

import android.media.AudioRecord
import chat.peerage.MicRecovery.Step
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Захват звука после ошибки чтения микрофона: умерший (отвалилась гарнитура,
 * перезапустился аудио-сервер) переоткрывается с растущей паузой, прочие ошибки —
 * пауза в кадр, а не холостой цикл.
 */
class MicRecoveryTest {
    private val dead = AudioRecord.ERROR_DEAD_OBJECT

    @Test fun dead_mic_is_reopened_with_growing_pause() {
        val r = MicRecovery()
        val pauses = List(7) { (r.after(dead) as Step.Reopen).afterMs }
        assertEquals(listOf(100L, 200L, 400L, 800L, 1600L, 2000L, 2000L), pauses)
    }

    @Test fun working_mic_resets_the_pause() {
        val r = MicRecovery()
        repeat(3) { r.after(dead) }
        assertEquals(Step.Go, r.after(960))
        assertEquals(Step.Reopen(100), r.after(dead))
    }

    @Test fun other_errors_wait_a_frame() {
        val r = MicRecovery()
        assertEquals(Step.Wait(20), r.after(AudioRecord.ERROR_INVALID_OPERATION))
        assertEquals(Step.Wait(20), r.after(0))
    }
}
