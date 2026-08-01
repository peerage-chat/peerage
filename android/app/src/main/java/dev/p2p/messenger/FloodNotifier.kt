package dev.p2p.messenger

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build

/**
 * Уведомления о чужом потоке трафика и об офлайне (ADR-007).
 *
 * Зачем вообще: остановить чужие пакеты на устройстве нельзя, и деньги за трафик
 * тратятся ПРЯМО СЕЙЧАС. Ждать, пока пользователь сам заглянет в «Сеть», — значит
 * молчать о том, что его касается немедленно.
 *
 * Два разных уведомления:
 *   * **эпизод флуда** — один раз на эпизод, не каждую секунду, иначе само уведомление
 *     превращается во второй флуд;
 *   * **«вы офлайн»** — постоянное, пока пользователь вне сети. Состояние «мне не
 *     приходят сообщения» слишком легко забыть, а цена забывчивости велика.
 */
class FloodNotifier : BroadcastReceiver() {

    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            ACTION_OFFLINE -> {
                AppModel.instance.goOffline()
                cancelFlood(ctx)
                showOffline(ctx)
            }
            ACTION_ONLINE -> {
                AppModel.instance.goOnline()
                cancelOffline(ctx)
            }
            ACTION_ROTATE -> {
                // Смена адреса — не «одна кнопка»: пользователь должен выбрать, кому
                // сообщить новый. Поэтому открываем экран, а не запускаем молча.
                cancelFlood(ctx)
                ctx.startActivity(
                    Intent(ctx, MainActivity::class.java).apply {
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                        putExtra(EXTRA_OPEN_ROTATE, true)
                    },
                )
            }
        }
    }

    companion object {
        private const val CHANNEL = "p2p_flood"
        const val NOTIF_FLOOD = 4
        const val NOTIF_OFFLINE = 5
        const val ACTION_OFFLINE = "dev.p2p.messenger.GO_OFFLINE"
        const val ACTION_ONLINE = "dev.p2p.messenger.GO_ONLINE"
        const val ACTION_ROTATE = "dev.p2p.messenger.ROTATE_ADDRESS"
        const val EXTRA_OPEN_ROTATE = "open_rotate"

        private fun ensureChannel(ctx: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(
                    NotificationChannel(
                        CHANNEL,
                        ctx.getString(R.string.flood_title),
                        NotificationManager.IMPORTANCE_DEFAULT,
                    ),
                )
            }
        }

        private fun broadcast(ctx: Context, code: Int, action: String): PendingIntent =
            PendingIntent.getBroadcast(
                ctx,
                code,
                Intent(action).apply { setPackage(ctx.packageName) },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )

        /** Показать уведомление о потоке. `who` — имя/ключ источника, `pps` — темп. */
        fun showFlood(ctx: Context, who: String, pps: Int) {
            ensureChannel(ctx)
            val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val open = PendingIntent.getActivity(
                ctx,
                20,
                Intent(ctx, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            val n = Notification.Builder(ctx, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_warning)
                .setContentTitle(ctx.getString(R.string.notif_flood_title))
                .setContentText(ctx.getString(R.string.notif_flood_body, who, pps))
                .setStyle(
                    Notification.BigTextStyle()
                        .bigText(ctx.getString(R.string.notif_flood_body, who, pps)),
                )
                .setContentIntent(open)
                .setAutoCancel(true)
                .addAction(
                    Notification.Action.Builder(
                        null,
                        ctx.getString(R.string.notif_action_offline),
                        broadcast(ctx, 21, ACTION_OFFLINE),
                    ).build(),
                )
                .addAction(
                    Notification.Action.Builder(
                        null,
                        ctx.getString(R.string.notif_action_rotate),
                        broadcast(ctx, 22, ACTION_ROTATE),
                    ).build(),
                )
                .build()
            nm.notify(NOTIF_FLOOD, n)
        }

        /** Постоянное «вы офлайн» с кнопкой возврата. */
        fun showOffline(ctx: Context) {
            ensureChannel(ctx)
            val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            val n = Notification.Builder(ctx, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_lock_power_off)
                .setContentTitle(ctx.getString(R.string.notif_offline_title))
                .setContentText(ctx.getString(R.string.notif_offline_body))
                .setOngoing(true) // не смахнуть: состояние важнее аккуратной шторки
                .addAction(
                    Notification.Action.Builder(
                        null,
                        ctx.getString(R.string.notif_action_online),
                        broadcast(ctx, 23, ACTION_ONLINE),
                    ).build(),
                )
                .build()
            nm.notify(NOTIF_OFFLINE, n)
        }

        fun cancelFlood(ctx: Context) =
            (ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(NOTIF_FLOOD)

        fun cancelOffline(ctx: Context) =
            (ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager).cancel(NOTIF_OFFLINE)
    }
}
