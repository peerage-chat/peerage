package chat.peerage

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.Person

/**
 * Уведомление о входящем звонке: системный шаблон звонка + полноэкранный интент.
 *
 * Когда [AppModel] получает событие `room_invite`, он вызывает [show] — человек видит
 * звонок, даже если приложение свёрнуто. «Отклонить» приходит сюда (receiver делегирует в
 * [AppModel] и гасит уведомление), «Ответить» и «Без видео» — сразу в активити.
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
        /** Новый id: звук канала после создания не меняется, а прежний «incoming_call»
         *  звенел звуком уведомления один раз вместо рингтона по кругу (7b). */
        private const val CHANNEL = "call_ring"
        private const val OLD_CHANNEL = "incoming_call"

        /** Звонит не дольше этого: потерянный сброс звонящего не должен звенеть вечно. */
        private const val RING_MS = 60_000L
        const val NOTIF_ID = 3
        const val ACTION_ACCEPT = "chat.peerage.ACCEPT_CALL"
        const val ACTION_DECLINE = "chat.peerage.DECLINE_CALL"
        const val EXTRA_TOKEN = "accept_token"

        /** Принять с камерой (у видеозвонка «Ответить»; «Без видео» — false). */
        const val EXTRA_VIDEO = "accept_video"

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

        /**
         * Входящий в системном шаблоне звонка (notifications.md, п. 7): «Отклонить / Ответить»,
         * у видео ещё «Без видео». Полноэкранный интент — наш экран входящего на блокировке.
         */
        fun show(ctx: Context, callerName: String, video: Boolean, avatar: ByteArray?) {
            ensureChannel(ctx)
            val openPi = PendingIntent.getActivity(
                ctx,
                10,
                Intent(ctx, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            // «Ответить» ведёт прямо в активити, а не через receiver: открыть активити
            // из receiver кнопки уведомления («трамплин») Android 12+ не даёт, и
            // звонок принимался при закрытом приложении. Активити и примет, и
            // проверит микрофон. Токен один на обе кнопки ответа — сгорает с первой.
            acceptToken = java.util.UUID.randomUUID().toString()
            fun accept(code: Int, withVideo: Boolean) = PendingIntent.getActivity(
                ctx,
                code,
                Intent(ctx, MainActivity::class.java).apply {
                    action = ACTION_ACCEPT
                    putExtra(EXTRA_TOKEN, acceptToken)
                    putExtra(EXTRA_VIDEO, withVideo)
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
            val caller = Person.Builder()
                .setName(callerName)
                .setIcon(MessageNotifier.avatarIcon(avatar))
                .setImportant(true)
                .build()
            val b = NotificationCompat.Builder(ctx, CHANNEL)
                .setSmallIcon(R.drawable.ic_glyph)
                .setStyle(NotificationCompat.CallStyle.forIncomingCall(caller, declinePi, accept(11, video)).setIsVideo(video))
                .setContentText(ctx.getString(if (video) R.string.notif_incoming_video else R.string.notif_incoming_audio))
                .setCategory(NotificationCompat.CATEGORY_CALL)
                .setPriority(NotificationCompat.PRIORITY_MAX)
                .setOngoing(true)
                .setContentIntent(openPi)
                .setFullScreenIntent(openPi, true)
                .setTimeoutAfter(RING_MS)
            if (video) b.addAction(0, ctx.getString(R.string.notif_no_video), accept(13, false))
            // Рингтон по кругу, пока уведомление не снято (ответ, сброс, тайм-аут).
            val n = b.build().apply { flags = flags or Notification.FLAG_INSISTENT }
            runCatching { NotificationManagerCompat.from(ctx).notify(NOTIF_ID, n) }
        }

        fun cancel(ctx: Context) {
            (ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
                .cancel(NOTIF_ID)
        }

        private fun ensureChannel(ctx: Context) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
                if (nm.getNotificationChannel(CHANNEL) == null) {
                    nm.deleteNotificationChannel(OLD_CHANNEL)
                    // Рингтон телефона; громкость и беззвучный режим — как у обычного звонка.
                    val ring = android.media.AudioAttributes.Builder()
                        .setUsage(android.media.AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
                        .setContentType(android.media.AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build()
                    nm.createNotificationChannel(
                        NotificationChannel(CHANNEL, ctx.getString(R.string.notif_incoming_channel), NotificationManager.IMPORTANCE_HIGH).apply {
                            description = ctx.getString(R.string.notif_incoming_channel_desc)
                            setSound(android.media.RingtoneManager.getDefaultUri(android.media.RingtoneManager.TYPE_RINGTONE), ring)
                            enableVibration(true)
                            setShowBadge(false)
                        },
                    )
                }
            }
        }
    }
}
