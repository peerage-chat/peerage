package chat.peerage

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.text.format.Formatter
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * Уведомления о передаче файлов (notifications.md → «Передача файлов», макеты `notif/Xfer*`).
 *
 * Только докачки, заказанные кнопкой «Скачать» (авто-вложения уведомлений не дают). Одно тихое
 * уведомление с прогрессом: одна передача — имя, от кого, сколько из скольких и «Отмена»;
 * несколько — общий процент, строки ↓ и «Отменить все». Отправитель не в сети — «пауза».
 * Смахнутое молчит до готово / ошибки этой передачи. «Скачано» — только когда приложение
 * свёрнуто; ошибка — всегда. Отдача прогресса не имеет: файл тянет получатель (pull).
 */
object TransferNotifier {
    private const val CHANNEL = "xfer"
    private const val ID_PROGRESS = 40
    private const val ID_RESULT = 41

    const val ACTION_CANCEL = "chat.peerage.XFER_CANCEL"
    const val ACTION_CANCEL_ALL = "chat.peerage.XFER_CANCEL_ALL"
    const val ACTION_DISMISSED = "chat.peerage.XFER_DISMISSED"
    const val ACTION_RETRY = "chat.peerage.XFER_RETRY"
    const val ACTION_OPEN_FILE = "chat.peerage.XFER_OPEN"
    const val EXTRA_BLOB = "chat.peerage.BLOB"
    const val EXTRA_NAME = "chat.peerage.NAME"
    const val EXTRA_TOKEN = "chat.peerage.XFER_TOKEN"

    /** Заказанная докачка: что, у кого, куда и сколько пришло ([frac] < 0 — ещё не началась). */
    data class Item(
        val blobId: String,
        val name: String,
        val size: Long,
        val chat: ChatTarget,
        /** У кого файл (hex) и как его зовут. */
        val peer: String,
        val from: String,
        val frac: Float = -1f,
        /** Отправитель не в сети — докачка ждёт его. */
        val waiting: Boolean = false,
    )

    /** Смахнутые передачи: прогресс по ним заново не всплывает. */
    private val dismissed = HashSet<String>()
    private var shown = emptyList<Item>()
    private var lastKey: Any? = null

    /** «Открыть» ведёт в экспортированную активити — только с токеном из нашего уведомления. */
    private val openTokens = HashMap<String, String>()

    @Synchronized
    fun consumeOpenToken(blobId: String, token: String?): Boolean {
        val ok = !token.isNullOrEmpty() && openTokens[blobId] == token
        if (ok) openTokens.remove(blobId)
        return ok
    }

    /** Перерисовать уведомление о прогрессе по текущим докачкам. */
    @Synchronized
    fun show(ctx: Context, items: List<Item>) {
        dismissed.retainAll(items.map { it.blobId }.toSet())
        val visible = items.filter { it.blobId !in dismissed }
        // Тот же вид — не перерисовываем (тикер зовёт раз в несколько секунд).
        val key = visible.map { Triple(it.blobId, pct(it.frac), it.waiting) }
        if (key == lastKey) return
        lastKey = key
        shown = visible
        if (visible.isEmpty()) return NotificationManagerCompat.from(ctx).cancel(ID_PROGRESS)
        ensureChannel(ctx)
        val n = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_glyph)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setSilent(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setDeleteIntent(broadcast(ctx, ACTION_DISMISSED, ""))
        if (visible.size == 1) one(ctx, n, visible[0]) else many(ctx, n, visible)
        runCatching { NotificationManagerCompat.from(ctx).notify(ID_PROGRESS, n.build()) }
    }

    /** Смахнули — молчать по этим передачам до их готово / ошибки. */
    @Synchronized
    fun onDismissed() {
        shown.forEach { dismissed += it.blobId }
    }

    private fun one(ctx: Context, n: NotificationCompat.Builder, it: Item) {
        val done = size(ctx, (it.size * it.frac.coerceAtLeast(0f)).toLong())
        val total = size(ctx, it.size)
        n.setSubText(
            if (it.waiting) ctx.getString(R.string.xfer_paused) else ctx.getString(R.string.xfer_downloading, pct(it.frac)),
        )
            .setContentTitle(it.name)
            .setContentText(
                if (it.waiting) ctx.getString(R.string.xfer_offline, done, total) else ctx.getString(R.string.xfer_from, it.from, done, total),
            )
            .setProgress(100, pct(it.frac), it.frac < 0 && !it.waiting)
            .setContentIntent(openChat(ctx, it.chat))
            .addAction(0, ctx.getString(R.string.xfer_cancel), broadcast(ctx, ACTION_CANCEL, it.blobId))
    }

    private fun many(ctx: Context, n: NotificationCompat.Builder, items: List<Item>) {
        val total = items.sumOf { it.size }
        val done = items.sumOf { (it.size * it.frac.coerceAtLeast(0f)).toLong() }
        val all = if (total > 0) (done * 100 / total).toInt() else 0
        val title = ctx.resources.getQuantityString(R.plurals.xfer_files, items.size, items.size, size(ctx, done), size(ctx, total))
        val style = NotificationCompat.InboxStyle()
        items.forEach {
            val state = when {
                it.waiting -> ctx.getString(R.string.xfer_state_paused)
                it.frac < 0 -> ctx.getString(R.string.xfer_state_queued)
                else -> "${pct(it.frac)}%"
            }
            style.addLine(ctx.getString(R.string.xfer_line, it.name, it.from, state))
        }
        n.setSubText(ctx.getString(R.string.xfer_many, all))
            .setContentTitle(title)
            .setStyle(style.setBigContentTitle(title))
            .setProgress(100, all, false)
            .addAction(0, ctx.getString(R.string.xfer_cancel_all), broadcast(ctx, ACTION_CANCEL_ALL, ""))
    }

    /** «Скачано» (только ручное и только когда приложение свёрнуто — решает вызывающий). */
    @Synchronized
    fun done(ctx: Context, it: Item) {
        ensureChannel(ctx)
        val token = java.util.UUID.randomUUID().toString()
        openTokens[it.blobId] = token
        val open = PendingIntent.getActivity(
            ctx,
            (ACTION_OPEN_FILE + it.blobId).hashCode(),
            Intent(ACTION_OPEN_FILE).setClass(ctx, MainActivity::class.java)
                .putExtra(EXTRA_BLOB, it.blobId)
                .putExtra(EXTRA_NAME, it.name)
                .putExtra(EXTRA_TOKEN, token)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val n = result(ctx, it, ctx.getString(R.string.xfer_done))
            .setContentText(ctx.getString(R.string.xfer_done_text, it.from, size(ctx, it.size)))
            .addAction(0, ctx.getString(R.string.xfer_open), open)
            .addAction(0, ctx.getString(R.string.xfer_to_chat), openChat(ctx, it.chat))
        runCatching { NotificationManagerCompat.from(ctx).notify(it.blobId, ID_RESULT, n.build()) }
    }

    /** Не удалось — всегда; «Повторить», если повтор может помочь (место освободят). */
    @Synchronized
    fun failed(ctx: Context, it: Item, retry: Boolean, reason: String) {
        ensureChannel(ctx)
        val n = result(ctx, it, ctx.getString(R.string.xfer_failed)).setContentText(reason)
        if (retry) n.addAction(0, ctx.getString(R.string.xfer_retry), broadcast(ctx, ACTION_RETRY, it.blobId))
        n.addAction(0, ctx.getString(R.string.xfer_to_chat), openChat(ctx, it.chat))
        runCatching { NotificationManagerCompat.from(ctx).notify(it.blobId, ID_RESULT, n.build()) }
    }

    fun cancelResult(ctx: Context, blobId: String) = NotificationManagerCompat.from(ctx).cancel(blobId, ID_RESULT)

    private fun result(ctx: Context, it: Item, sub: String) = NotificationCompat.Builder(ctx, CHANNEL)
        .setSmallIcon(R.drawable.ic_glyph)
        .setSubText(sub)
        .setContentTitle(it.name)
        .setContentIntent(openChat(ctx, it.chat))
        .setAutoCancel(true)
        .setSilent(true)

    private fun pct(frac: Float) = (frac.coerceIn(0f, 1f) * 100).toInt()

    private fun size(ctx: Context, bytes: Long) = Formatter.formatShortFileSize(ctx, bytes)

    private fun openChat(ctx: Context, chat: ChatTarget): PendingIntent {
        val tag = shortcutIdOf(chat)
        return PendingIntent.getActivity(
            ctx,
            ("xfer" + tag).hashCode(),
            Intent(ACTION_OPEN_CHAT).setClass(ctx, MainActivity::class.java)
                .putExtra(EXTRA_SHORTCUT_CHAT, tag)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun broadcast(ctx: Context, action: String, blobId: String) = PendingIntent.getBroadcast(
        ctx,
        (action + blobId).hashCode(),
        Intent(action).setClass(ctx, MessageActionReceiver::class.java).putExtra(EXTRA_BLOB, blobId),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun ensureChannel(ctx: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = ctx.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL) != null) return
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, ctx.getString(R.string.xfer_channel), NotificationManager.IMPORTANCE_LOW).apply {
                setShowBadge(false)
            },
        )
    }
}
