package chat.peerage

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
        if (intent.action == ACTION_DECLINE) AppModel.instance.declineDmCall()
    }

    companion object {
        private const val CHANNEL = "incoming_call"
        const val NOTIF_ID = 3
        const val ACTION_ACCEPT = "chat.peerage.ACCEPT_CALL"
        const val ACTION_DECLINE = "chat.peerage.DECLINE_CALL"
        const val EXTRA_TOKEN = "accept_token"

        /**
         * Одноразовый токен «Принять». MainActivity экспортирована (её открывает
         * лаунчер), и без проверки любое приложение на телефоне могло бы прислать ей
         * ACTION_ACCEPT и принять звонок за человека — то есть включить его микрофон.
         * Токен знает только наш PendingIntent; новый на каждый входящий.
         */
        @Volatile var acceptToken: String = ""
            private set

        /** Токен из интента совпал с выданным — и сгорает. */
        fun consumeToken(t: String?): Boolean {
            val ok = !t.isNullOrEmpty() && t == acceptToken
            if (ok) acceptToken = ""
            return ok
        }

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
            // «Принять» ведёт прямо в активити, а не через receiver: открыть активити
            // из receiver кнопки уведомления («трамплин») Android 12+ не даёт, и
            // звонок принимался при закрытом приложении. Активити и примет, и
            // проверит микрофон.
            val acceptPi = PendingIntent.getActivity(
                ctx,
                11,
                Intent(ctx, MainActivity::class.java).apply {
                    action = ACTION_ACCEPT
                    acceptToken = java.util.UUID.randomUUID().toString()
                    putExtra(EXTRA_TOKEN, acceptToken)
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                },
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

            b.setContentTitle(ctx.getString(R.string.notif_incoming_call))
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
                        ctx.getString(R.string.notif_accept),
                        acceptPi,
                    ).build(),
                )
                b.addAction(
                    Notification.Action.Builder(
                        android.graphics.drawable.Icon.createWithResource(ctx, android.R.drawable.ic_delete),
                        ctx.getString(R.string.notif_decline),
                        declinePi,
                    ).build(),
                )
            } else {
                @Suppress("DEPRECATION")
                b.addAction(0, ctx.getString(R.string.notif_accept), acceptPi)
                @Suppress("DEPRECATION")
                b.addAction(0, ctx.getString(R.string.notif_decline), declinePi)
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
                        NotificationChannel(CHANNEL, ctx.getString(R.string.notif_incoming_channel), NotificationManager.IMPORTANCE_HIGH).apply {
                            description = ctx.getString(R.string.notif_incoming_channel_desc)
                            enableVibration(true)
                            setShowBadge(false)
                        },
                    )
                }
            }
        }
    }
}
