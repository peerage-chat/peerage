package chat.peerage

import android.content.SharedPreferences

/** Полоса-напоминание о копиях над списком главной (app-ui.md → «Напоминания о копиях»). */
enum class BackupStrip(val family: Family) {
    KEY_MISSING(Family.KEY),
    KEY_CHECK_DUE(Family.KEY),
    KEY_CHECK_FAILED(Family.KEY),
    DATA_STALE(Family.DATA),
    ;

    /** Ключ и данные — разные «семейства»: между ними пауза, внутри — своё расписание. */
    enum class Family { KEY, DATA }
}

/**
 * Что известно о копиях — всё, от чего зависит полоса. Время — unix-секунды. Правила
 * живут здесь чистыми функциями, а модель только хранит факты и зовёт переходы.
 *
 * @property keyDone копия ключа хоть раз прошла проверку трёх слов.
 * @property keyChecks успешных проверок с последнего провала — ступень лестницы 7 → 14 → 30.
 * @property closedFamily чью полосу последней закрыли «Позже» или выполнили; другое
 *   семейство ждёт [GAP_DAYS] от [closedAt], чтобы напоминания не шли подряд.
 */
data class BackupFacts(
    val keyDone: Boolean = false,
    val keyCheckedAt: Long = 0,
    val keyChecks: Int = 0,
    val keyFailed: Boolean = false,
    val keySnoozedUntil: Long = 0,
    val dataBackupAt: Long = 0,
    val dataSnoozedUntil: Long = 0,
    val firstUseAt: Long = 0,
    val closedFamily: BackupStrip.Family? = null,
    val closedAt: Long = 0,
) {
    /** Какую полосу показать сейчас; null — никакую. Ключ важнее данных, видна одна. */
    fun strip(now: Long, hasChats: Boolean): BackupStrip? =
        keyStrip(now)?.takeIf { open(it, now) } ?: dataStrip(now, hasChats)?.takeIf { open(it, now) }

    private fun keyStrip(now: Long): BackupStrip? = when {
        now < keySnoozedUntil -> null
        keyFailed -> BackupStrip.KEY_CHECK_FAILED
        !keyDone -> BackupStrip.KEY_MISSING
        now >= keyCheckedAt + checkInterval() -> BackupStrip.KEY_CHECK_DUE
        else -> null
    }

    private fun dataStrip(now: Long, hasChats: Boolean): BackupStrip? = when {
        !hasChats || firstUseAt == 0L || now < firstUseAt + FIRST_DATA_DAYS * DAY -> null
        now < dataSnoozedUntil -> null
        dataBackupAt == 0L || now >= dataBackupAt + DATA_STALE_DAYS * DAY -> BackupStrip.DATA_STALE
        else -> null
    }

    private fun open(s: BackupStrip, now: Long) =
        closedFamily == null || closedFamily == s.family || now >= closedAt + GAP_DAYS * DAY

    /** Интервал до следующей проверки по ступени лестницы. */
    private fun checkInterval(): Long = CHECK_LADDER_DAYS[(keyChecks - 1).coerceIn(0, CHECK_LADDER_DAYS.lastIndex)] * DAY

    /** «Позже»: полоса уходит на свой срок; у проверки — на текущий интервал. */
    fun later(s: BackupStrip, now: Long): BackupFacts = when (s) {
        BackupStrip.KEY_MISSING -> copy(keySnoozedUntil = now + DAY)
        BackupStrip.KEY_CHECK_DUE -> copy(keySnoozedUntil = now + checkInterval())
        BackupStrip.KEY_CHECK_FAILED -> copy(keySnoozedUntil = now + DAY)
        BackupStrip.DATA_STALE -> copy(dataSnoozedUntil = now + DATA_LATER_DAYS * DAY)
    }.closed(s.family, now)

    /** Три слова совпали: копия есть, лестница — дальше или заново после провала. */
    fun checkPassed(now: Long) = copy(
        keyDone = true,
        keyCheckedAt = now,
        keyChecks = if (keyFailed) 1 else keyChecks + 1,
        keyFailed = false,
        keySnoozedUntil = 0,
    ).closed(BackupStrip.Family.KEY, now)

    /** Три попытки не совпали: полоса «не пройдена» — сразу и до успеха. */
    fun checkFailed(now: Long) = copy(keyFailed = true, keySnoozedUntil = 0).closed(BackupStrip.Family.KEY, now)

    /** Ключ сверен глазами: полоса «не пройдена» вернётся завтра, пока проверка не пройдёт. */
    fun reconciled(now: Long) = copy(keySnoozedUntil = now + DAY).closed(BackupStrip.Family.KEY, now)

    /** Копия данных сохранена. */
    fun dataDone(now: Long) = copy(dataBackupAt = now, dataSnoozedUntil = 0).closed(BackupStrip.Family.DATA, now)

    private fun closed(f: BackupStrip.Family, now: Long) = copy(closedFamily = f, closedAt = now)

    /** Сохранить в prefs целиком. */
    fun save(p: SharedPreferences) {
        p.edit()
            .putBoolean(K_DONE, keyDone)
            .putLong(K_CHECKED, keyCheckedAt)
            .putInt(K_CHECKS, keyChecks)
            .putBoolean(K_FAILED, keyFailed)
            .putLong(K_SNOOZE, keySnoozedUntil)
            .putLong(D_AT, dataBackupAt)
            .putLong(D_SNOOZE, dataSnoozedUntil)
            .putLong(FIRST_USE, firstUseAt)
            .putString(CLOSED_FAMILY, closedFamily?.name ?: "")
            .putLong(CLOSED_AT, closedAt)
            .apply()
    }

    companion object {
        const val DAY = 86_400L
        private val CHECK_LADDER_DAYS = longArrayOf(7, 14, 30)
        private const val FIRST_DATA_DAYS = 7L
        private const val DATA_STALE_DAYS = 30L
        private const val DATA_LATER_DAYS = 7L
        private const val GAP_DAYS = 3L

        private const val K_DONE = "key_backup_done"
        private const val K_CHECKED = "key_checked_at"
        private const val K_CHECKS = "key_checks"
        private const val K_FAILED = "key_check_failed"
        private const val K_SNOOZE = "key_snoozed_until"
        private const val D_AT = "data_backup_at"
        private const val D_SNOOZE = "data_snoozed_until"
        private const val FIRST_USE = "first_use_at"
        private const val CLOSED_FAMILY = "backup_closed_family"
        private const val CLOSED_AT = "backup_closed_at"

        /**
         * Прочитать из prefs. Отметка «копия сделана» от старых версий ставилась нажатием
         * «Записал», без проверки, — такой копии отсчитываем первую проверку с этого запуска.
         * Первый запуск без отметки начинает отсчёт «недели использования» для копии данных.
         */
        fun load(p: SharedPreferences, now: Long): BackupFacts {
            val f = BackupFacts(
                keyDone = p.getBoolean(K_DONE, false),
                keyCheckedAt = p.getLong(K_CHECKED, 0),
                keyChecks = p.getInt(K_CHECKS, 0),
                keyFailed = p.getBoolean(K_FAILED, false),
                keySnoozedUntil = p.getLong(K_SNOOZE, 0),
                dataBackupAt = p.getLong(D_AT, 0),
                dataSnoozedUntil = p.getLong(D_SNOOZE, 0),
                firstUseAt = p.getLong(FIRST_USE, 0),
                closedFamily = BackupStrip.Family.entries.firstOrNull { it.name == p.getString(CLOSED_FAMILY, "") },
                closedAt = p.getLong(CLOSED_AT, 0),
            )
            val fixed = f.copy(
                keyCheckedAt = if (f.keyDone && f.keyCheckedAt == 0L) now else f.keyCheckedAt,
                keyChecks = if (f.keyDone && f.keyChecks == 0) 1 else f.keyChecks,
                firstUseAt = if (f.firstUseAt == 0L) now else f.firstUseAt,
            )
            if (fixed != f) fixed.save(p)
            return fixed
        }
    }
}
