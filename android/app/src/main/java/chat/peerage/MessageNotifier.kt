package chat.peerage

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.Person
import androidx.core.app.RemoteInput
import androidx.core.graphics.drawable.IconCompat

/**
 * Уведомления о сообщениях (`docs/design/notifications.md` → «Итог»): разговор на чат
 * (MessagingStyle), кнопки «Ответить» и «Прочитано». Каналы — личные, группы, упоминания:
 * звук и вибрацию каждого человек меняет в системных настройках канала (решение 9 — звуки
 * системные). Строка уведомления — тег [shortcutIdOf] чата, по нему же её снимают.
 *
 * Строки держатся в памяти процесса: убитый процесс уведомлений не строит (пушей нет), а
 * после запуска досланное старше [ChatSounds.BACKLOG_MS] в шторку не идёт (п. 3).
 */
internal object MessageNotifier {
    const val CH_DM = "msg_dm"
    const val CH_GROUP = "msg_group"
    const val CH_MENTION = "msg_mention"
    private const val NOTIF_ID = 20
    private const val MAX_LINES = 7
    private const val KEY_REPLY = "reply"

    /** Строка разговора: автор (hex) и id — ссылка для отзыва и правки (ADR-006). */
    data class Line(
        val id: Long,
        val sender: String,
        val name: String,
        val text: String,
        val time: Long,
        val avatar: ByteArray?,
        val mine: Boolean = false,
        /** Пропущенный звонок: у личного чата под ним «Перезвонить». */
        val missedCall: Boolean = false,
    )

    /** Всё, из чего строится уведомление чата, — чтобы перестроить его при отзыве или правке. */
    data class Chat(val title: String, val group: Boolean, val channel: String, val lines: List<Line>)

    /**
     * Поднимать ли шторку — всё правило в одном месте. Своё и служебное — нет; из звонков —
     * только пропущенный, и его режим чата не глушит (п. 7: звонки режим не глушит); лента этого чата на экране — нет, там звук (п. 4); режим чата не пускает
     * ([NotifyMode.lets]: «упоминания» — только с упоминанием меня) — нет;
     * досланное старше [ChatSounds.BACKLOG_MS] по времени отправки — нет (п. 3).
     */
    fun shouldNotify(
        mine: Boolean,
        system: Boolean,
        call: Boolean,
        missed: Boolean,
        onScreen: Boolean,
        mode: NotifyMode,
        mentionsMe: Boolean,
        sentMs: Long,
        nowMs: Long,
    ): Boolean = !mine && !system && !onScreen && (if (call) missed else mode.lets(mentionsMe)) &&
        !(sentMs > 0 && nowMs - sentMs > ChatSounds.BACKLOG_MS)

    private val chats = HashMap<ChatTarget, Chat>()

    @Synchronized
    fun post(ctx: Context, chat: ChatTarget, title: String, mention: Boolean, line: Line) {
        val prev = chats[chat]
        val channel = when {
            mention -> CH_MENTION
            chat.group -> CH_GROUP
            else -> CH_DM
        }
        val c = Chat(title, chat.group, channel, ((prev?.lines ?: emptyList()) + line).takeLast(MAX_LINES))
        chats[chat] = c
        show(ctx, chat, c, alert = true)
    }

    /**
     * Ответ из шторки — своей строкой в том же разговоре, без звука (п. 10 анатомии: «отправленное
     * появляется строкой»). Снять уведомление после прямого ответа Android 15+ и не даёт: оно живёт,
     * пока его не смахнут. После перезапуска процесса строк нет — разговор из одного ответа.
     */
    @Synchronized
    fun appendMine(ctx: Context, chat: ChatTarget, title: String, text: String) {
        val prev = chats[chat] ?: Chat(title, chat.group, if (chat.group) CH_GROUP else CH_DM, emptyList())
        val line = Line(0, "me", "", text, System.currentTimeMillis(), null, mine = true)
        val c = prev.copy(lines = (prev.lines + line).takeLast(MAX_LINES))
        chats[chat] = c
        show(ctx, chat, c, alert = false)
    }

    /** Строку отозвали ([text] == null) или исправили: перестроить без звука. */
    @Synchronized
    fun replace(ctx: Context, chat: ChatTarget, sender: String, id: Long, text: String?) {
        val c = chats[chat] ?: return
        if (c.lines.none { it.id == id && it.sender == sender }) return
        val lines = if (text == null) {
            c.lines.filterNot { it.id == id && it.sender == sender }
        } else {
            c.lines.map { if (it.id == id && it.sender == sender) it.copy(text = text) else it }
        }
        if (lines.isEmpty()) return cancel(ctx, chat)
        chats[chat] = c.copy(lines = lines)
        show(ctx, chat, chats.getValue(chat), alert = false)
    }

    /** Снять уведомление чата — и то, что осталось в шторке от прошлого процесса. */
    @Synchronized
    fun cancel(ctx: Context, chat: ChatTarget) {
        chats.remove(chat)
        NotificationManagerCompat.from(ctx).cancel(shortcutIdOf(chat), NOTIF_ID)
    }

    private fun show(ctx: Context, chat: ChatTarget, c: Chat, alert: Boolean) {
        ensureChannels(ctx)
        val model = AppModel.instance
        // Блокировка приложения скрывает текст всегда (макет settings/Notifications); иначе — по
        // настройке «Текст в уведомлении».
        val hide = AppLock.enabled || !model.notifyText
        val hidden = ctx.getString(R.string.notif_msg_hidden)
        val me = Person.Builder().setName(model.selfName.ifBlank { ctx.getString(R.string.app_name) }).setKey("me").build()
        val style = NotificationCompat.MessagingStyle(me)
        if (c.group) style.setConversationTitle(c.title).setGroupConversation(true)
        c.lines.forEach { l ->
            val text = if (hide) hidden else l.text
            if (l.mine) {
                style.addMessage(text, l.time, null as Person?)
            } else {
                val who = Person.Builder().setName(l.name).setKey(l.sender)
                avatarIcon(l.avatar)?.let { who.setIcon(it) }
                style.addMessage(text, l.time, who.build())
            }
        }
        val tag = shortcutIdOf(chat)
        val open = PendingIntent.getActivity(
            ctx,
            tag.hashCode(),
            Intent(ACTION_OPEN_CHAT).setClass(ctx, MainActivity::class.java)
                .putExtra(EXTRA_SHORTCUT_CHAT, tag)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        // Экран блокировки телефона — по умолчанию только имя чата (решение 8).
        val public = NotificationCompat.Builder(ctx, c.channel)
            .setSmallIcon(R.drawable.ic_glyph)
            .setContentTitle(c.title)
            .setContentText(hidden)
            .build()
        val n = NotificationCompat.Builder(ctx, c.channel)
            .setSmallIcon(R.drawable.ic_glyph)
            .setStyle(style)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(public)
            .setContentIntent(open)
            .setAutoCancel(true)
            .setOnlyAlertOnce(!alert)
            .setWhen(c.lines.last().time)
            .setShowWhen(true)
        // Система показывает не больше трёх кнопок. Под пропущенным звонком первая —
        // «Перезвонить», и «Заглушить на час» уступает ей место: режим звонки всё равно не глушит.
        val missed = !c.group && c.lines.last().missedCall
        if (missed) n.addAction(callBackAction(ctx, tag))
        n.addAction(replyAction(ctx, tag))
            .addAction(
                NotificationCompat.Action.Builder(0, ctx.getString(R.string.notif_read), actionIntent(ctx, ACTION_READ, tag, mutable = false))
                    .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_MARK_AS_READ)
                    .setShowsUserInterface(false)
                    .build(),
            )
        // п. 5: в группе — только упоминания на час, в личном — ничего на час.
        if (!missed) {
            n.addAction(
                NotificationCompat.Action.Builder(0, ctx.getString(R.string.notif_mute_hour), actionIntent(ctx, ACTION_MUTE_HOUR, tag, mutable = false))
                    .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_MUTE)
                    .setShowsUserInterface(false)
                    .build(),
            )
        }
        // Разговор в «Разговорах» системы — только если ярлык чата опубликован (его имя и фото
        // уходят системе только по выбору: «Чаты в меню Поделиться», 5.6).
        if (model.shareChatsInMenu) n.setShortcutId(tag)
        // Без разрешения на уведомления (Android 13+) notify бросает — просто молчим.
        runCatching { NotificationManagerCompat.from(ctx).notify(tag, NOTIF_ID, n.build()) }
    }

    /**
     * «Перезвонить»: ведёт в активити (звонок без экрана не начать); активити экспортирована,
     * поэтому — с одноразовым токеном.
     */
    private fun callBackAction(ctx: Context, tag: String): NotificationCompat.Action {
        val token = java.util.UUID.randomUUID().toString()
        callTokens[tag] = token
        val back = PendingIntent.getActivity(
            ctx,
            (ACTION_CALL_BACK + tag).hashCode(),
            Intent(ACTION_CALL_BACK).setClass(ctx, MainActivity::class.java)
                .putExtra(EXTRA_SHORTCUT_CHAT, tag)
                .putExtra(EXTRA_CALL_TOKEN, token)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Action.Builder(0, ctx.getString(R.string.notif_call_back), back).build()
    }

    private fun replyAction(ctx: Context, tag: String): NotificationCompat.Action {
        val input = RemoteInput.Builder(KEY_REPLY).setLabel(ctx.getString(R.string.notif_reply)).build()
        return NotificationCompat.Action.Builder(0, ctx.getString(R.string.notif_reply), actionIntent(ctx, ACTION_REPLY, tag, mutable = true))
            .addRemoteInput(input)
            .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY)
            .setShowsUserInterface(false)
            .setAllowGeneratedReplies(false)
            .build()
    }

    /** Ответу нужен изменяемый интент: система дописывает в него введённый текст. */
    private fun actionIntent(ctx: Context, action: String, tag: String, mutable: Boolean): PendingIntent {
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or
            if (mutable && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else PendingIntent.FLAG_IMMUTABLE
        return PendingIntent.getBroadcast(
            ctx,
            (action + tag).hashCode(),
            Intent(action).setClass(ctx, MessageActionReceiver::class.java).putExtra(EXTRA_SHORTCUT_CHAT, tag),
            flags,
        )
    }

    internal fun avatarIcon(bytes: ByteArray?): IconCompat? = bytes?.let {
        runCatching { BitmapFactory.decodeByteArray(it, 0, it.size) }.getOrNull()?.let(IconCompat::createWithBitmap)
    }

    fun ensureChannels(ctx: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = ctx.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CH_MENTION) != null) return
        // Вибрация — как в макете: личные и упоминания с ней, группы без.
        fun ch(id: String, name: Int, vibrate: Boolean) = NotificationChannel(id, ctx.getString(name), NotificationManager.IMPORTANCE_HIGH).apply {
            enableVibration(vibrate)
            setShowBadge(true)
        }
        nm.createNotificationChannels(
            listOf(
                ch(CH_DM, R.string.notif_ch_dm, vibrate = true),
                ch(CH_GROUP, R.string.notif_ch_group, vibrate = false),
                ch(CH_MENTION, R.string.notif_ch_mention, vibrate = true),
            ),
        )
    }

    /** Текст из поля «Ответить» в шторке. */
    fun replyText(intent: Intent): String? =
        RemoteInput.getResultsFromIntent(intent)?.getCharSequence(KEY_REPLY)?.toString()?.takeIf { it.isNotBlank() }

    const val ACTION_REPLY = "chat.peerage.NOTIF_REPLY"
    const val ACTION_READ = "chat.peerage.NOTIF_READ"
    const val ACTION_MUTE_HOUR = "chat.peerage.NOTIF_MUTE_HOUR"
    const val ACTION_CALL_BACK = "chat.peerage.NOTIF_CALL_BACK"
    const val EXTRA_CALL_TOKEN = "chat.peerage.CALL_TOKEN"

    /** Одноразовые токены «Перезвонить» по чатам: чужое приложение не начнёт звонок за человека. */
    private val callTokens = HashMap<String, String>()

    @Synchronized
    fun consumeCallToken(tag: String, token: String?): Boolean {
        val ok = !token.isNullOrEmpty() && callTokens[tag] == token
        if (ok) callTokens.remove(tag)
        return ok
    }
    const val ACTION_READ_ALL = "chat.peerage.NOTIF_READ_ALL"
}

/**
 * Кнопки уведомлений о сообщениях, запросах и приглашениях. Не экспортирован: интенты — только
 * наши PendingIntent. Шторка переживает процесс: кнопка могла прийти туда, где движка ещё нет, —
 * поднимаем его (и службу: узел должен жить, пока ответ уходит) вне главного потока.
 */
class MessageActionReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        val app = ctx.applicationContext
        val action = intent.action ?: return
        val work: (AppModel) -> Unit = when (action) {
            // «Связь» есть, только пока жив процесс со службой — движок уже работает.
            MessageNotifier.ACTION_READ_ALL -> return AppModel.instance.readAll()
            MessageNotifier.ACTION_MUTE_HOUR -> {
                val chat = intent.getStringExtra(EXTRA_SHORTCUT_CHAT)?.let(::chatOfShortcut) ?: return
                MessageNotifier.cancel(app, chat)
                val w: (AppModel) -> Unit = { m -> m.muteForHour(chat) }
                w
            }
            MessageNotifier.ACTION_REPLY, MessageNotifier.ACTION_READ -> {
                val chat = intent.getStringExtra(EXTRA_SHORTCUT_CHAT)?.let(::chatOfShortcut) ?: return
                val reply = if (action == MessageNotifier.ACTION_REPLY) MessageNotifier.replyText(intent) ?: return else null
                // «Прочитано» снимает уведомление; ответ дописывается в него строкой (см. appendMine).
                if (reply == null) MessageNotifier.cancel(app, chat)
                val w: (AppModel) -> Unit = { m ->
                    m.readFromNotification(chat)
                    reply?.let { m.replyFromNotification(chat, it) }
                }
                w
            }
            RequestNotifier.ACTION_ACCEPT, RequestNotifier.ACTION_DECLINE,
            RequestNotifier.ACTION_JOIN, RequestNotifier.ACTION_REFUSE,
            -> {
                val id = intent.getStringExtra(RequestNotifier.EXTRA_ID)?.takeIf { it.matches(HEX_ID) } ?: return
                RequestNotifier.cancel(app, action, id)
                val w: (AppModel) -> Unit = { m ->
                    when (action) {
                        RequestNotifier.ACTION_ACCEPT -> m.acceptRequest(id)
                        RequestNotifier.ACTION_DECLINE -> m.declineRequest(id)
                        RequestNotifier.ACTION_JOIN -> m.acceptGroupInvite(id)
                        else -> m.declineGroupInvite(id)
                    }
                }
                w
            }
            else -> return
        }
        val pending = goAsync()
        Thread({
            try {
                if (ConnService.startEngineNow(app)) {
                    ConnService.start(app)
                    work(AppModel.instance)
                }
            } finally {
                pending.finish()
            }
        }, "notif-action").start()
    }

    private companion object {
        val HEX_ID = Regex("[0-9a-f]{16,128}")
    }
}
