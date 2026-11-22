package chat.peerage

import android.media.AudioManager
import android.media.ToneGenerator

/**
 * Тоны звонка (`docs/design/notifications.md` → «Звуки в приложении»): гудки, пока дозваниваемся,
 * короткое «занято», когда собеседник сбросил, и сигнал конца разговора. Системные тоны в поток
 * разговора — тихо и туда же, куда звук звонка. Рингтон входящего — не здесь: его играет канал
 * уведомления ([IncomingCallReceiver]), так он слушается беззвучного режима сам.
 */
object CallTones {
    private const val VOLUME = 60 // из 100: гудки тише голоса
    private const val BUSY_MS = 1_800
    private const val END_MS = 300

    private var gen: ToneGenerator? = null
    private var ringing = false

    /** Гудки по кругу до [stop]. Повторный вызов ничего не делает. */
    @Synchronized
    fun ringback() {
        if (ringing) return
        ringing = true
        tone(ToneGenerator.TONE_SUP_RINGTONE, -1)
    }

    /** Собеседник сбросил, пока дозванивались. */
    @Synchronized
    fun busy() = tone(ToneGenerator.TONE_SUP_BUSY, BUSY_MS)

    /** Разговор окончен. */
    @Synchronized
    fun ended() = tone(ToneGenerator.TONE_PROP_PROMPT, END_MS)

    @Synchronized
    fun stop() {
        ringing = false
        gen?.stopTone()
    }

    private fun tone(type: Int, ms: Int) {
        if (type != ToneGenerator.TONE_SUP_RINGTONE) ringing = false
        // ToneGenerator держит нативный ресурс; создаём один раз. Сбой (нет аудио) — без тона.
        val g = gen ?: runCatching { ToneGenerator(AudioManager.STREAM_VOICE_CALL, VOLUME) }.getOrNull()?.also { gen = it } ?: return
        g.stopTone()
        g.startTone(type, ms)
    }
}
