package chat.peerage

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings

/**
 * Всё, от чего зависит, доживёт ли узел до следующего сообщения при погасшем экране
 * (п. 50, app-ui.md → «Работа в фоне»): системная оптимизация батареи Android, батарея
 * приложения в прошивке и её автозапуск ([Autostart]).
 *
 * Системное «не оптимизировать» на Xiaomi не меняет настройку MIUI «Контроль активности»:
 * прошивка останавливает приложение по своей, пока там не выбрано «Нет ограничений».
 */
internal object Background {
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
        val opened = v == Vendor.MIUI && runCatching { ctx.startActivity(miuiIntent(ctx)) }.isSuccess
        if (!opened) runCatching { ctx.startActivity(appInfoIntent(ctx)) }
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
