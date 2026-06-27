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
 * Уведомление о входящем звонке (heads-up + fullScreenIntent).
 *
 * Когда [AppModel] получает событие `room_invite`, он вызывает [show] — пользователь
 * видит баннер даже если приложение свёрнуто. Кнопки «Принять» / «Отклонить» посылают
 * broadcast сюда; receiver делегирует в [AppModel] и гасит уведомление.
 *
 * Процесс жив благодаря [ConnService] (PARTIAL_WAKE_LOCK + WifiLock), поэтому
 * [AppModel.pollThread] успевает поймать событие без FCM.
 */
class IncomingCallReceiver : BroadcastReceiver() {

    override fun onReceive(ctx: Context, intent: Intent) {
        cancel(ctx)
        when (intent.action) {
            ACTION_ACCEPT -> {
                AppModel.instance.acceptDmCall()
                ctx.startActivity(
                    Intent(ctx, MainActivity::class.java).apply {
                        flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                    },
                )
            }
            ACTION_DECLINE -> AppModel.instance.declineDmCall()
        }
    }

    companion object {
        private const val CHANNEL = "p2p_incoming_call"
        const val NOTIF_ID = 3
        const val ACTION_ACCEPT = "dev.p2p.messenger.ACCEPT_CALL"
        const val ACTION_DECLINE = "dev.p2p.messenger.DECLINE_CALL"

        fun show(ctx: Context, callerName: String) {
            ensureChannel(ctx)
            val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

            val openPi = PendingIntent.getActivity(
                ctx,
                10,
                Intent(ctx, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            val acceptPi = PendingIntent.getBroadcast(
                ctx,
                11,
                Intent(ACTION_ACCEPT).apply { setPackage(ctx.packageName) },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            val declinePi = PendingIntent.getBroadcast(
                ctx,
                12,
                Intent(ACTION_DECLINE).apply { setPackage(ctx.packageName) },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )

            @Suppress("DEPRECATION")
            val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Notification.Builder(ctx, CHANNEL)
            } else {
                Notification.Builder(ctx)
            }

            b.setContentTitle("Входящий звонок")
                .setContentText(callerName)
                .setSmallIcon(android.R.drawable.stat_sys_phone_call)
                .setCategory(Notification.CATEGORY_CALL)
                .setAutoCancel(true)
                .setOngoing(true)
                .setContentIntent(openPi)
                .setFullScreenIntent(openPi, true)

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                b.addAction(
                    Notification.Action.Builder(
                        android.graphics.drawable.Icon.createWithResource(ctx, android.R.drawable.ic_menu_call),
                        "Принять",
                        acceptPi,
                    ).build(),
                )
                b.addAction(
                    Notification.Action.Builder(
                        android.graphics.drawable.Icon.createWithResource(ctx, android.R.drawable.ic_delete),
                        "Отклонить",
                        declinePi,
                    ).build(),
                )
            } else {
                @Suppress("DEPRECATION")
                b.addAction(0, "Принять", acceptPi)
                @Suppress("DEPRECATION")
                b.addAction(0, "Отклонить", declinePi)
            }

            nm.notify(NOTIF_ID, b.build())
        }

        fun cancel(ctx: Context) {
            (ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
                .cancel(NOTIF_ID)
        }

        private fun ensureChannel(ctx: Context) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
                if (nm.getNotificationChannel(CHANNEL) == null) {
                    nm.createNotificationChannel(
                        NotificationChannel(CHANNEL, "Входящие звонки", NotificationManager.IMPORTANCE_HIGH).apply {
                            description = "Уведомление о входящем звонке"
                            enableVibration(true)
                            setShowBadge(false)
                        },
                    )
                }
            }
        }
    }
}
