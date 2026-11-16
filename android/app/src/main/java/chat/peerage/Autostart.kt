package chat.peerage

import android.content.ComponentName
import android.content.Context
import android.content.Intent

/**
 * Экран «Автозапуск» прошивок, которые без него останавливают приложение в фоне (Xiaomi,
 * Huawei, Oppo, Vivo). В стандартном Android такого экрана нет, и строки в разрешениях тогда
 * нет тоже. Узнать, включён ли автозапуск, нельзя — только открыть экран.
 *
 * Пакеты перечислены в `<queries>` манифеста: без этого с Android 11 чужие экраны не находятся.
 */
internal object Autostart {
    private val screens = listOf(
        "com.miui.securitycenter" to "com.miui.permcenter.autostart.AutoStartManagementActivity",
        "com.huawei.systemmanager" to "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity",
        "com.huawei.systemmanager" to "com.huawei.systemmanager.optimize.process.ProtectActivity",
        "com.coloros.safecenter" to "com.coloros.safecenter.permission.startup.StartupAppListActivity",
        "com.coloros.safecenter" to "com.coloros.safecenter.startupapp.StartupAppListActivity",
        "com.oppo.safe" to "com.oppo.safe.permission.startup.StartupAppListActivity",
        "com.vivo.permissionmanager" to "com.vivo.permissionmanager.activity.BgStartUpManagerActivity",
        "com.iqoo.secure" to "com.iqoo.secure.ui.phoneoptimize.AddWhiteListActivity",
    )

    /** Экран автозапуска этой прошивки; null — его нет. Спрашивать PackageManager: у явного
     *  компонента `Intent.resolveActivity` возвращает его же, есть он на телефоне или нет. */
    fun intent(ctx: Context): Intent? = screens.asSequence()
        .map { (pkg, cls) -> Intent().setComponent(ComponentName(pkg, cls)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
        .firstOrNull { ctx.packageManager.resolveActivity(it, 0) != null }
}
