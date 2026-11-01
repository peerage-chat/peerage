package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Полосы-напоминания о копиях на главной (app-ui.md → «Напоминания о копиях»). */
class BackupRemindersTest {
    private val day = 86_400L
    private val t0 = 1_000L * day

    /** Человек пользуется давно, переписка есть, копии данных свежие — мешает только ключ. */
    private fun facts(
        keyDone: Boolean = true,
        checkedAt: Long = t0,
        checks: Int = 1,
        failed: Boolean = false,
        dataAt: Long = t0,
    ) = BackupFacts(
        keyDone = keyDone,
        keyCheckedAt = checkedAt,
        keyChecks = checks,
        keyFailed = failed,
        dataBackupAt = dataAt,
        firstUseAt = t0 - 100 * day,
    )

    private fun BackupFacts.at(dt: Long, chats: Boolean = true) = strip(t0 + dt, chats)

    @Test
    fun missingKeyShowsNowAndLaterHidesItForADay() {
        val f = facts(keyDone = false)
        assertEquals(BackupStrip.KEY_MISSING, f.at(0))
        val later = f.later(BackupStrip.KEY_MISSING, t0)
        assertNull(later.at(day - 1))
        assertEquals(BackupStrip.KEY_MISSING, later.at(day))
    }

    @Test
    fun checkComesDueAfter7Then14Then30ThenEvery30() {
        var f = facts(checks = 1)
        var now = t0
        for (gap in listOf(7, 14, 30, 30, 30)) {
            assertNull(f.strip(now + gap * day - 1, false))
            assertEquals(BackupStrip.KEY_CHECK_DUE, f.strip(now + gap * day, false))
            now += gap * day
            f = f.checkPassed(now)
        }
    }

    @Test
    fun laterOnCheckWaitsTheCurrentInterval() {
        val f = facts(checks = 2) // следующий интервал — 14 дней
        val due = t0 + 14 * day
        val later = f.later(BackupStrip.KEY_CHECK_DUE, due)
        assertNull(later.strip(due + 14 * day - 1, true))
        assertEquals(BackupStrip.KEY_CHECK_DUE, later.strip(due + 14 * day, true))
    }

    @Test
    fun failedCheckStaysUntilReconciledThenDailyAndResetsTheLadderOnSuccess() {
        val f = facts(checks = 3).checkFailed(t0)
        assertEquals(BackupStrip.KEY_CHECK_FAILED, f.at(0))
        val seen = f.reconciled(t0)
        assertNull(seen.at(day - 1))
        assertEquals(BackupStrip.KEY_CHECK_FAILED, seen.at(day))
        // Успех после провала — лестница заново: следующая проверка через 7 дней.
        val ok = seen.checkPassed(t0 + day)
        assertNull(ok.strip(t0 + 8 * day - 1, true))
        assertEquals(BackupStrip.KEY_CHECK_DUE, ok.strip(t0 + 8 * day, true))
    }

    @Test
    fun failedFirstCheckOutranksMissingKey() {
        val f = facts(keyDone = false).checkFailed(t0)
        assertEquals(BackupStrip.KEY_CHECK_FAILED, f.at(0))
    }

    @Test
    fun dataReminderNeedsAWeekOfUseAndChats() {
        val fresh = BackupFacts(keyDone = true, keyCheckedAt = t0, keyChecks = 3, firstUseAt = t0)
        assertNull(fresh.at(7 * day - 1))
        assertEquals(BackupStrip.DATA_STALE, fresh.at(7 * day))
        assertNull(fresh.at(7 * day, chats = false))
    }

    @Test
    fun dataReminderAfter30DaysAndLaterIsAWeek() {
        val f = facts(checkedAt = t0 + 10 * day, checks = 3) // проверка ключа — на 40-й день
        assertNull(f.at(30 * day - 1))
        assertEquals(BackupStrip.DATA_STALE, f.at(30 * day))
        val later = f.later(BackupStrip.DATA_STALE, t0 + 30 * day)
        assertNull(later.at(37 * day - 1))
        assertEquals(BackupStrip.DATA_STALE, later.at(37 * day))
    }

    @Test
    fun keyGoesFirstAndTheOtherFamilyWaitsThreeDays() {
        val f = facts(keyDone = false, dataAt = 0)
        assertEquals(BackupStrip.KEY_MISSING, f.at(0))
        // Ключ отложен на день: данные в этот день молчат, через день — снова ключ.
        val later = f.later(BackupStrip.KEY_MISSING, t0)
        assertNull(later.at(day - 1))
        assertEquals(BackupStrip.KEY_MISSING, later.at(day))
        // Ключ проверен — копия данных ждёт 3 дня после этого.
        val ok = later.checkPassed(t0 + day)
        assertNull(ok.at(4 * day - 1))
        assertEquals(BackupStrip.DATA_STALE, ok.at(4 * day))
    }

    @Test
    fun dataDoneHidesDataAndHoldsKeyForThreeDays() {
        val f = facts(checkedAt = t0 - 7 * day, dataAt = 0).dataDone(t0)
        assertNull(f.at(3 * day - 1))
        assertEquals(BackupStrip.KEY_CHECK_DUE, f.at(3 * day))
    }
}
