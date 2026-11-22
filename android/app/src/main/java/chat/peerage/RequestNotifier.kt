package chat.peerage

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * Запросы в контакты и приглашения в группы (notifications.md, решение 10): свежие — в шторку с
 * «Принять / Отклонить» и «Вступить / Отклонить». Решённое в приложении снимается по списку
 * ([sync]). Выключатель — «Запросы и приглашения» в «Уведомлениях».
 */
internal object RequestNotifier {
    const val CHANNEL = "requests"
    private const val ID_REQUEST = 30
    private const val ID_INVITE = 31
    const val ACTION_ACCEPT = "chat.peerage.REQ_ACCEPT"
    const val ACTION_DECLINE = "chat.peerage.REQ_DECLINE"
    const val ACTION_JOIN = "chat.peerage.INV_JOIN"
    const val ACTION_REFUSE = "chat.peerage.INV_DECLINE"
    const val EXTRA_ID = "chat.peerage.ID"

    /** Новый запрос: имя — самоназвание отправителя, приветствие — по правилам текста шторки. */
    fun request(ctx: Context, r: ContactRequest, hideText: Boolean) {
        val name = r.name.ifBlank { r.fingerprint }
        val text = if (hideText || r.greeting.isBlank()) name else "$name: ${r.greeting}"
        post(
            ctx,
            tag = r.key,
            id = ID_REQUEST,
            title = ctx.getString(R.string.notif_request_title),
            text = text,
            actions = listOf(R.string.notif_accept to ACTION_ACCEPT, R.string.notif_decline to ACTION_DECLINE),
        )
    }

    fun invite(ctx: Context, i: GroupInvite, inviterName: String) {
        post(
            ctx,
            tag = i.gid,
            id = ID_INVITE,
            title = ctx.getString(R.string.notif_invite_title),
            text = ctx.getString(R.string.notif_invite_text, i.name, inviterName),
            actions = listOf(R.string.group_invite_join to ACTION_JOIN, R.string.notif_decline to ACTION_REFUSE),
        )
    }

    /** Снять уведомления о запросах и приглашениях, которых в списках больше нет. */
    fun sync(ctx: Context, requests: Set<String>, invites: Set<String>) {
        val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
        nm.activeNotifications.forEach { n ->
            val gone = (n.id == ID_REQUEST && n.tag !in requests) || (n.id == ID_INVITE && n.tag !in invites)
            if (gone) nm.cancel(n.tag, n.id)
        }
    }

    fun cancel(ctx: Context, action: String, tag: String) {
        val id = if (action == ACTION_ACCEPT || action == ACTION_DECLINE) ID_REQUEST else ID_INVITE
        NotificationManagerCompat.from(ctx).cancel(tag, id)
    }

    private fun post(ctx: Context, tag: String, id: Int, title: String, text: String, actions: List<Pair<Int, String>>) {
        ensureChannel(ctx)
        val open = PendingIntent.getActivity(
            ctx,
            0,
            Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val public = NotificationCompat.Builder(ctx, CHANNEL).setSmallIcon(R.drawable.ic_glyph).setContentTitle(title).build()
        val b = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_glyph)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setCategory(NotificationCompat.CATEGORY_SOCIAL)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(public)
            .setContentIntent(open)
            .setAutoCancel(true)
        actions.forEach { (label, action) ->
            val pi = PendingIntent.getBroadcast(
                ctx,
                (action + tag).hashCode(),
                Intent(action).setClass(ctx, MessageActionReceiver::class.java).putExtra(EXTRA_ID, tag),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            b.addAction(0, ctx.getString(label), pi)
        }
        runCatching { NotificationManagerCompat.from(ctx).notify(tag, id, b.build()) }
    }

    private fun ensureChannel(ctx: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = ctx.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL) != null) return
        nm.createNotificationChannel(NotificationChannel(CHANNEL, ctx.getString(R.string.notif_ch_requests), NotificationManager.IMPORTANCE_DEFAULT))
    }
}
