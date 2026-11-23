package chat.peerage

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

/**
 * Постоянный foreground-сервис, удерживающий процесс (а с ним и узел Yggdrasil,
 * который крутится в ЭТОМ ЖЕ процессе) живым и подключённым, когда экран погас.
 * Иначе MIUI/Doze замораживают фоновое приложение → goroutine'ы ygg встают →
 * пир-соединение отваливается по таймауту → «телефон пропал из сети» (звонок
 * рвётся, входящие не доходят). Это «режим A» Фазы D (serverless): без своего
 * сервера, ценой батареи.
 *
 * Держит PARTIAL_WAKE_LOCK — CPU не засыпает, потоки ygg продолжают работать.
 * Wi-Fi-лок FULL_HIGH_PERF НЕ берём в фоне (пинит радио на полную → лишний жор):
 * для keepalive хватает обычного Wi-Fi (power-save добавляет лишь задержку,
 * Yggdrasil ретраит). Высокую производительность Wi-Fi берёт [CallService] только
 * на время звонка, когда это реально нужно.
 *
 * Кнопка «Отключить» в уведомлении останавливает сервис → локи отпускаются, CPU
 * засыпает, фоновый расход прекращается. Повторный вход в приложение (onStart)
 * поднимает сервис заново.
 */
class ConnService : Service() {
    private var wake: PowerManager.WakeLock? = null

    private val disconnectReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            stop(ctx)
        }
    }

    override fun onCreate() {
        super.onCreate()
        LogExport.install(applicationContext) // сервис живёт и без открытого экрана
        startForegroundCompat()
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "app:mesh").apply {
            setReferenceCounted(false)
            acquire()
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(disconnectReceiver, IntentFilter(ACTION_DISCONNECT), RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(disconnectReceiver, IntentFilter(ACTION_DISCONNECT))
        }
    }

    // Перезапускаться, если система всё же убьёт сервис под давлением памяти. Перезапуск
    // приходит с пустым интентом и в новом процессе — движок тогда поднимаем сами, иначе
    // служба держала бы процесс без узла.
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent == null || intent.getBooleanExtra(EXTRA_ENGINE, false)) startEngine()
        return START_STICKY
    }

    /** Движок без экрана (загрузка, обновление, перезапуск службы) — только на уровне «Ключ
     *  устройства»: пароль в фоне спросить не у кого. Повторный старт ничего не делает. */
    private fun startEngine() {
        val app = applicationContext
        Thread({ startEngineNow(app) }, "engine-boot").start()
    }

    override fun onDestroy() {
        runCatching { unregisterReceiver(disconnectReceiver) }
        runCatching { wake?.takeIf { it.isHeld }?.release() }
        wake = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startForegroundCompat() {
        val notif = buildNotification()
        when {
            // API 34+: обязателен конкретный тип; «specialUse» — наш кейс
            // (постоянный mesh-узел), не требует Play-ревью для сайдлоада.
            Build.VERSION.SDK_INT >= 34 ->
                startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
            // API 29..33: тип нужен, но без отдельного разрешения; dataSync подходит.
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q ->
                startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            else -> startForeground(NOTIF_ID, notif)
        }
    }

    private fun buildNotification(): Notification = build(this, last ?: ConnState())

    companion object {
        internal const val CHANNEL = "conn"

        /** Последнее показанное состояние: перестраиваем уведомление только при смене. */
        @Volatile private var last: ConnState? = null

        /** Привести «связь» к состоянию [state] (тикер модели, раз в секунду). */
        fun refresh(ctx: Context, state: ConnState) {
            if (state == last) return
            last = state
            runCatching { ctx.getSystemService(NotificationManager::class.java).notify(NOTIF_ID, build(ctx, state)) }
        }

        /**
         * «Связь» (notifications.md, решения 2 и 10): заголовок — состояние узла, текст — всё
         * непрочитанное одной строкой. Тап — приложение; «Прочитать всё», если есть что; в офлайне —
         * «Подключиться» вместо отдельного уведомления об офлайне.
         */
        private fun build(ctx: Context, state: ConnState): Notification {
            // Категория — здесь, а не у вызывающих: служба показывает уведомление при
            // старте, раньше первого тика модели, и на свежей установке категории ещё нет —
            // Android 8+ тогда роняет процесс («invalid channel for service notification»).
            ensureChannel(ctx)
            val open = PendingIntent.getActivity(
                ctx,
                0,
                Intent(ctx, MainActivity::class.java).apply { flags = Intent.FLAG_ACTIVITY_SINGLE_TOP },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            fun broadcast(code: Int, action: String, cls: Class<*>? = null): PendingIntent = PendingIntent.getBroadcast(
                ctx,
                code,
                Intent(action).apply { if (cls != null) setClass(ctx, cls) else setPackage(ctx.packageName) },
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            val title = ctx.getString(
                when {
                    state.offline -> R.string.notif_conn_offline
                    state.searching -> R.string.notif_conn_search
                    else -> R.string.notif_conn_online
                },
            )
            val text = if (state.offline) ctx.getString(R.string.notif_conn_offline_body) else summary(ctx, state)

            @Suppress("DEPRECATION")
            val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(ctx, CHANNEL) else Notification.Builder(ctx)
            b.setContentTitle(title)
                .setSmallIcon(R.drawable.ic_glyph)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setContentIntent(open)
            if (text.isNotEmpty()) b.setContentText(text)
            fun action(label: Int, pi: PendingIntent) = b.addAction(Notification.Action.Builder(null, ctx.getString(label), pi).build())
            when {
                state.offline -> action(R.string.notif_connect, broadcast(1, FloodNotifier.ACTION_ONLINE))
                state.hasUnread -> action(R.string.notif_read_all, broadcast(2, MessageNotifier.ACTION_READ_ALL, MessageActionReceiver::class.java))
            }
            action(R.string.notif_disconnect, broadcast(0, ACTION_DISCONNECT))
            return b.build()
        }

        /** «5 новых в 2 чатах · упоминание · 1 запрос»; пусто — нечего. */
        fun summary(ctx: Context, s: ConnState): String {
            val r = ctx.resources
            return s.parts().joinToString(" · ") { p ->
                when (p) {
                    is ConnState.Part.Messages ->
                        r.getQuantityString(R.plurals.notif_conn_msgs, p.count, p.count) + " " +
                            r.getQuantityString(R.plurals.notif_conn_chats, p.chats, p.chats)
                    ConnState.Part.Mention -> ctx.getString(R.string.notif_conn_mention)
                    is ConnState.Part.Requests -> r.getQuantityString(R.plurals.notif_conn_requests, p.count, p.count)
                    is ConnState.Part.Invites -> r.getQuantityString(R.plurals.notif_conn_invites, p.count, p.count)
                }
            }
        }

        /** Служба поднимает и движок (запуск без экрана — [BootReceiver]). */
        private const val EXTRA_ENGINE = "engine"

        /** Категория «связи» — общая для постоянного уведомления и тихих состояний узла. */
        fun ensureChannel(ctx: Context) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
            val ch = NotificationChannel(CHANNEL, ctx.getString(R.string.notif_conn_channel), NotificationManager.IMPORTANCE_LOW).apply {
                description = ctx.getString(R.string.notif_conn_channel_desc)
                setShowBadge(false)
            }
            ctx.getSystemService(NotificationManager::class.java).createNotificationChannel(ch)
        }

        /**
         * Поднять движок здесь же (звать с фонового потока): служба без экрана и кнопки шторки,
         * пришедшие в новый процесс. Только на уровне «Ключ устройства» — пароль в фоне спросить
         * не у кого. Повторный вызов ничего не делает. true — движок работает.
         */
        fun startEngineNow(app: Context): Boolean {
            if (!BootReceiver.isOnboarded(app) || VaultKeyManager.needsPassword(Profiles.main(app))) return false
            val model = AppModel.instance
            if (model.appContext == null) model.appContext = app
            AppLock.load(app)
            return model.start(Profiles.main(app).absolutePath)
        }

        /** Системные настройки категории (Android 8+: категория; раньше — уведомления приложения). */
        fun channelSettings(ctx: Context, channel: String = CHANNEL): Intent =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Intent(android.provider.Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS)
                    .putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, ctx.packageName)
                    .putExtra(android.provider.Settings.EXTRA_CHANNEL_ID, channel)
            } else {
                Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, android.net.Uri.fromParts("package", ctx.packageName, null))
            }
        private const val NOTIF_ID = 1
        private const val ACTION_DISCONNECT = "chat.peerage.DISCONNECT_MESH"

        fun start(ctx: Context, withEngine: Boolean = false) {
            val i = Intent(ctx, ConnService::class.java).putExtra(EXTRA_ENGINE, withEngine)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ctx.startForegroundService(i)
            } else {
                ctx.startService(i)
            }
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, ConnService::class.java))
        }
    }
}
