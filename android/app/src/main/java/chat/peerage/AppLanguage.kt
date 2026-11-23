package chat.peerage

import android.app.Activity
import android.app.LocaleManager
import android.content.Context
import android.content.res.Configuration
import android.content.res.Resources
import android.os.Build
import android.os.LocaleList
import android.os.SystemClock
import java.util.Locale

/**
 * Язык интерфейса: как в системе или выбранный в настройках.
 *
 * Android 13+: язык приложения хранит система (`LocaleManager`). Тот же выбор виден в системных
 * «Языках приложений» (`res/xml/locales_config.xml`), а окна с новым языком система пересоздаёт
 * сама. Ниже 13 такого API нет: выбор лежит в `settings`, а контексты приложения, окна и служб
 * проходят через [wrap].
 */
object AppLanguage {
    /** Языки, на которые переведён интерфейс: тег → самоназвание (на самом языке, не переводится). */
    val choices: List<Pair<String, String>> = listOf("en" to "English", "ru" to "Русский")

    private const val KEY = "language"

    /**
     * Окно пересоздаётся после смены языка, а стек экранов живёт в нём: без отметки человек
     * оказался бы на главной, а не в профиле, откуда выбирал язык. Только в памяти процесса —
     * после его смерти возвращаться некуда. Со сроком: если действующий язык не сменился
     * (системный совпал с выбранным), окно не пересоздаётся, и поздняя отметка не нужна.
     */
    @Volatile private var reopenUntil = 0L

    /** Окно пересоздано сменой языка — открыть профиль. Читается один раз. */
    fun consumeReopen(): Boolean = (SystemClock.elapsedRealtime() < reopenUntil).also { reopenUntil = 0L }

    /** Выбранный тег из [choices]; "" — как в системе. */
    fun current(ctx: Context): String {
        val tag = if (Build.VERSION.SDK_INT >= 33) {
            ctx.getSystemService(LocaleManager::class.java).applicationLocales.let { if (it.isEmpty) "" else it[0].language }
        } else {
            prefs(ctx).getString(KEY, "").orEmpty()
        }
        return if (choices.any { it.first == tag }) tag else ""
    }

    /** Самоназвание выбранного языка; null — как в системе. */
    fun currentName(ctx: Context): String? = current(ctx).let { tag -> choices.firstOrNull { it.first == tag }?.second }

    /** Сменить язык: "" — вернуть системный. Окно перерисуется на новом языке. */
    fun set(activity: Activity, tag: String) {
        reopenUntil = SystemClock.elapsedRealtime() + 5_000
        if (Build.VERSION.SDK_INT >= 33) {
            activity.getSystemService(LocaleManager::class.java).applicationLocales =
                if (tag.isEmpty()) LocaleList.getEmptyLocaleList() else LocaleList.forLanguageTags(tag)
            return
        }
        prefs(activity).edit().putString(KEY, tag).commit()
        // Контекст приложения обёрнут при старте процесса: уведомления и службы берут строки
        // из него, поэтому меняем и его, не дожидаясь перезапуска.
        val app = activity.applicationContext
        @Suppress("DEPRECATION")
        app.resources.updateConfiguration(configFor(app, tag), app.resources.displayMetrics)
        activity.recreate()
    }

    /** Ниже Android 13 — контекст с выбранным языком; 13+ и «как в системе» — тот же контекст. */
    fun wrap(base: Context): Context {
        if (Build.VERSION.SDK_INT >= 33) return base
        val tag = prefs(base).getString(KEY, "").orEmpty()
        if (tag.isEmpty()) return base
        return base.createConfigurationContext(configFor(base, tag))
    }

    private fun configFor(ctx: Context, tag: String): Configuration {
        val locale = if (tag.isEmpty()) Resources.getSystem().configuration.locales[0] else Locale.forLanguageTag(tag)
        // Даты и числа форматируются по Locale.getDefault() — он идёт за языком интерфейса.
        Locale.setDefault(locale)
        return Configuration(ctx.resources.configuration).apply { setLocale(locale) }
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("settings", Context.MODE_PRIVATE)
}
