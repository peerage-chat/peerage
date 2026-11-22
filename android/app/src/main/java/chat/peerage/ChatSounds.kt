package chat.peerage

import android.app.NotificationManager
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.SoundPool

/**
 * Звуки в чате (`docs/design/notifications.md` → «Звуки в приложении»): капля на приход в
 * открытый чат и на отправку. Файлы — `res/raw/chat_in.wav`, `chat_out.wav`, их делает
 * `scripts/gen-chat-sounds.py`; всё остальное (рингтон, звук шторки) — системное.
 *
 * Громкость — системных звуков, как у звуков нажатия: это отклик интерфейса, а не уведомление.
 */
object ChatSounds {
    enum class Kind { In, Out }

    /** Досланное старше этого — без звука: пачка после офлайна не звенит (п. 3). */
    const val BACKLOG_MS = 120_000L

    /** Пачка приходов или серия вложений — не чаще одного звука за это время. */
    const val THROTTLE_MS = 1_500L

    /**
     * Звучать ли — всё правило в одном месте. [sentMs] — время отправки по часам автора
     * (0 — неизвестно: старый клиент, считаем свежим); [lastMs] — когда звучал этот же звук.
     * Заглушённый чат глушит только приход: отправка — отклик на своё действие.
     */
    fun allowed(
        kind: Kind,
        enabled: Boolean,
        quiet: Boolean,
        chatMuted: Boolean,
        sentMs: Long,
        nowMs: Long,
        lastMs: Long,
    ): Boolean {
        if (!enabled || quiet) return false
        if (nowMs - lastMs < THROTTLE_MS) return false
        if (kind == Kind.In) {
            if (chatMuted) return false
            if (sentMs > 0 && nowMs - sentMs > BACKLOG_MS) return false
        }
        return true
    }

    private var pool: SoundPool? = null
    private val ids = IntArray(2)
    private val last = LongArray(2) { Long.MIN_VALUE / 2 }

    /** Загрузить звуки заранее: первая загрузка SoundPool асинхронна. */
    @Synchronized
    fun init(ctx: Context) {
        if (pool != null) return
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()
        pool = SoundPool.Builder().setMaxStreams(2).setAudioAttributes(attrs).build().also {
            ids[Kind.In.ordinal] = it.load(ctx, R.raw.chat_in, 1)
            ids[Kind.Out.ordinal] = it.load(ctx, R.raw.chat_out, 1)
        }
    }

    @Synchronized
    fun play(ctx: Context, kind: Kind, enabled: Boolean, chatMuted: Boolean = false, sentMs: Long = 0) {
        val now = System.currentTimeMillis()
        if (!allowed(kind, enabled, quiet(ctx), chatMuted, sentMs, now, last[kind.ordinal])) return
        val p = pool ?: return
        last[kind.ordinal] = now
        p.play(ids[kind.ordinal], 1f, 1f, 1, 0, 1f)
    }

    /** Беззвучный режим, вибрация или «Не беспокоить» — тихо. */
    private fun quiet(ctx: Context): Boolean {
        val am = ctx.getSystemService(AudioManager::class.java) ?: return true
        if (am.ringerMode != AudioManager.RINGER_MODE_NORMAL) return true
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return false
        val f = nm.currentInterruptionFilter
        return f != NotificationManager.INTERRUPTION_FILTER_ALL && f != NotificationManager.INTERRUPTION_FILTER_UNKNOWN
    }
}
