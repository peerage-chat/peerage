package chat.peerage

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build

/**
 * Запуск при включении телефона и после обновления приложения (шаг беты 7h). Без него узел
 * не поднимался, пока человек сам не откроет приложение: сообщения ждали у отправителя, а
 * звонки пропадали.
 *
 * - Онбординг не пройден — ничего: личности ещё нет, создавать её в фоне нельзя.
 * - «Ключ устройства» — служба связи поднимает и движок ([ConnService.start] с `withEngine`).
 * - «Пароль» / «Ключ и пароль» — движок поднять нечем: тихое уведомление, тап открывает
 *   экран пароля. Снимается, когда движок стартовал ([dismissLocked]).
 *
 * `BOOT_COMPLETED` приходит после первой разблокировки — файлы приложения уже доступны.
 * Оба действия защищённые (их шлёт только система); всё прочее отбрасываем.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val app = ctx.applicationContext
        if (!isOnboarded(app)) return
        if (VaultKeyManager.needsPassword(Profiles.main(app))) {
            showLocked(app)
            return
        }
        // Android 12+ разрешает поднять службу переднего плана из этих двух рассылок; если
        // прошивка всё же откажет — узел поднимется при открытии, как раньше.
        runCatching { ConnService.start(app, withEngine = true) }
            .onFailure { android.util.Log.w("boot", "служба связи не поднялась: $it") }
    }

    companion object {
        private const val NOTIF_LOCKED = 7

        fun isOnboarded(ctx: Context): Boolean =
            ctx.getSharedPreferences("settings", Context.MODE_PRIVATE).getBoolean("onboarded", false)

        /** «Переписка закрыта паролем» — на категории «связи»: тихо, без всплытия. */
        private fun showLocked(ctx: Context) {
            ConnService.ensureChannel(ctx)
            val open = PendingIntent.getActivity(
                ctx,
                0,
                Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )

            @Suppress("DEPRECATION")
            val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Notification.Builder(ctx, ConnService.CHANNEL)
            } else {
                Notification.Builder(ctx)
            }
            val n = b.setSmallIcon(R.drawable.ic_glyph)
                .setContentTitle(ctx.getString(R.string.notif_locked_title))
                .setContentText(ctx.getString(R.string.notif_locked_body))
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
            runCatching { ctx.getSystemService(NotificationManager::class.java).notify(NOTIF_LOCKED, n) }
        }

        /** Движок стартовал — уведомление о закрытой паролем переписке больше не правда. */
        fun dismissLocked(ctx: Context) {
            runCatching { ctx.getSystemService(NotificationManager::class.java).cancel(NOTIF_LOCKED) }
        }
    }
}
