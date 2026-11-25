package chat.peerage

import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Всё, от чего зависит, доживёт ли узел до следующего сообщения при погасшем экране
 * (п. 50, app-ui.md → «Работа в фоне»): системная оптимизация батареи Android, батарея
 * приложения в прошивке и её автозапуск ([Autostart]).
 *
 * Системное «не оптимизировать» на Xiaomi не меняет настройку MIUI «Контроль активности»:
 * прошивка останавливает приложение по своей, пока там не выбрано «Нет ограничений».
 *
 * Напоминает полоса на главной ([BgFacts]); экраны прошивки считаются пройденными, как только
 * их открыли — отсюда или из онбординга ([opened]).
 */
internal object Background {
    var facts by mutableStateOf(BgFacts())
        private set
    private var app: Context? = null

    fun init(ctx: Context) {
        if (app != null) return
        app = ctx.applicationContext
        facts = BgFacts.load(prefs(ctx))
    }

    /** Экран открыли — полоса про него уходит (включено ли там, прошивка не говорит). */
    fun opened(s: BgStrip) = change { opened(s) }

    fun later() = change { later(System.currentTimeMillis() / 1000) }

    private fun change(f: BgFacts.() -> BgFacts) {
        facts = facts.f()
        app?.let { facts.save(prefs(it)) }
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("settings", Context.MODE_PRIVATE)

    /** Приложение уже выведено из-под оптимизации батареи Android? */
    fun ignoresBatteryOptimizations(ctx: Context): Boolean {
        val pm = ctx.getSystemService(Context.POWER_SERVICE) as PowerManager
        return pm.isIgnoringBatteryOptimizations(ctx.packageName)
    }

    /**
     * Прямой диалог «не оптимизировать» (требует REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).
     *
     * На части прошивок (проверено на MIUI) этот экшен ловят ДВА обработчика, и вместо
     * диалога система показывает выбор «Что использовать?». Поэтому целимся явно в
     * системные настройки; если их нет — отдаём неявный интент, а в крайнем случае
     * общий список батарейных исключений (он есть всегда).
     */
    fun batteryOptimizationIntent(ctx: Context): Intent {
        val direct = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${ctx.packageName}"))
        val settings = Intent(direct).setPackage("com.android.settings")
        return when {
            settings.resolveActivity(ctx.packageManager) != null -> settings
            direct.resolveActivity(ctx.packageManager) != null -> direct
            else -> Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
        }
    }

    /** Экран батареи приложения в прошивке: на MIUI — свой, иначе — страница приложения. */
    enum class Vendor { MIUI, APP_INFO }

    /**
     * Где в этой прошивке разрешают работу в фоне; null — в стандартном Android, где хватает
     * системной оптимизации. Включено ли, прошивка не говорит — только открыть экран.
     */
    fun vendor(ctx: Context): Vendor? = when {
        ctx.packageManager.resolveActivity(miuiIntent(ctx), 0) != null -> Vendor.MIUI
        Autostart.intent(ctx) != null -> Vendor.APP_INFO
        else -> null
    }

    /**
     * Открыть экран батареи приложения. MIUI «Контроль активности» (`powerkeeper`) сразу на
     * Peerage — там один тап «Нет ограничений». Не открылся (HyperOS закрыла экран) —
     * страница приложения в настройках: батарея там в одном тапе.
     */
    fun openVendor(ctx: Context, v: Vendor) {
        val shown = v == Vendor.MIUI && runCatching { ctx.startActivity(miuiIntent(ctx)) }.isSuccess
        if (!shown) runCatching { ctx.startActivity(appInfoIntent(ctx)) }
        opened(BgStrip.VENDOR)
    }

    /** Экран автозапуска прошивки ([Autostart]). */
    fun openAutostart(ctx: Context, intent: Intent) {
        runCatching { ctx.startActivity(intent) }
        opened(BgStrip.AUTOSTART)
    }

    private fun miuiIntent(ctx: Context) = Intent("miui.intent.action.HIDDEN_APPS_CONFIG_ACTIVITY")
        .setClassName("com.miui.powerkeeper", "com.miui.powerkeeper.ui.HiddenAppsConfigActivity")
        .putExtra("package_name", ctx.packageName)
        .putExtra("package_label", ctx.getString(R.string.app_name))
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    private fun appInfoIntent(ctx: Context) =
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", ctx.packageName, null))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
}

/** Полоса «работа в фоне» на главной: что мешает узлу жить при погасшем экране. */
enum class BgStrip { ANDROID, VENDOR, AUTOSTART }

/**
 * Что известно о работе в фоне. Оптимизацию Android система сообщает — её здесь нет, она
 * приходит аргументом и держит полосу, пока не снята. Экраны прошивки состояния не сообщают:
 * пройдены, когда их открыли. Время — unix-секунды.
 */
data class BgFacts(
    val vendorOpened: Boolean = false,
    val autostartOpened: Boolean = false,
    val snoozedUntil: Long = 0,
) {
    /** Какую полосу показать; null — никакую. По одной: Android, батарея прошивки, автозапуск. */
    fun strip(now: Long, androidLimited: Boolean, hasVendor: Boolean, hasAutostart: Boolean): BgStrip? = when {
        now < snoozedUntil -> null
        androidLimited -> BgStrip.ANDROID
        hasVendor && !vendorOpened -> BgStrip.VENDOR
        hasAutostart && !autostartOpened -> BgStrip.AUTOSTART
        else -> null
    }

    fun opened(s: BgStrip) = when (s) {
        BgStrip.ANDROID -> this
        BgStrip.VENDOR -> copy(vendorOpened = true)
        BgStrip.AUTOSTART -> copy(autostartOpened = true)
    }

    /** «Позже» — на [LATER_DAYS] дня. */
    fun later(now: Long) = copy(snoozedUntil = now + LATER_DAYS * DAY)

    fun save(p: SharedPreferences) = p.edit()
        .putBoolean(K_VENDOR, vendorOpened)
        .putBoolean(K_AUTOSTART, autostartOpened)
        .putLong(K_SNOOZED, snoozedUntil)
        .apply()

    companion object {
        const val DAY = 86_400L
        const val LATER_DAYS = 3L
        private const val K_VENDOR = "bg_vendor_opened"
        private const val K_AUTOSTART = "bg_autostart_opened"
        private const val K_SNOOZED = "bg_snoozed_until"

        fun load(p: SharedPreferences) = BgFacts(
            vendorOpened = p.getBoolean(K_VENDOR, false),
            autostartOpened = p.getBoolean(K_AUTOSTART, false),
            snoozedUntil = p.getLong(K_SNOOZED, 0),
        )
    }
}
