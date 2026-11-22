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
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.Person

/**
 * Foreground-сервис активного звонка/комнаты.
 *
 * Уведомление (notifications.md, п. 7): личный — системный шаблон звонка с секундомером,
 * «Выкл. микрофон» и «Завершить»; комната группы — секундомер, кто внутри, микрофон и «Выйти».
 * Состояние (микрофон, участники) берётся у [AppModel]; меняется — зовут [refresh].
 * Тип микрофон (API 30+) — Android знает, что мы законно пишем звук.
 * Без этого сервиса Android (особенно MIUI/Doze) убьёт процесс через ~1 мин
 * после погашения экрана → аудио стопится посреди разговора.
 */
class CallService : Service() {
    private var startMs = 0L
    private var peerName = ""
    private var room = false
    private var started = false
    private var wifi: WifiManager.WifiLock? = null

    private val endReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            when (intent.action) {
                ACTION_END_CALL -> AppModel.instance.leaveRoom()
                ACTION_MUTE -> {
                    AppModel.instance.toggleMute()
                    post()
                }
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(endReceiver, filter(), RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(endReceiver, filter())
        }
        // Высокопроизводительный Wi-Fi-лок ТОЛЬКО на время звонка: радио без
        // power-save → меньше потеря/джиттер медиа-пакетов. Вне звонка ConnService
        // его не держит (экономия батареи). LOW_LATENCY (API 29+) оптимизирует
        // задержку; на старых — FULL_HIGH_PERF.
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            WifiManager.WIFI_MODE_FULL_LOW_LATENCY
        } else {
            @Suppress("DEPRECATION")
            WifiManager.WIFI_MODE_FULL_HIGH_PERF
        }
        wifi = wm.createWifiLock(mode, "app:call").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Перезапуск системой после убийства процесса приходит без intent — звонка к
        // этому моменту уже нет (медиа жило в убитом процессе). Раньше START_STICKY
        // поднимал сервис заново, и висело вечное «Голосовой чат» без звонка.
        if (intent == null) {
            stopSelf()
            return START_NOT_STICKY
        }
        // Обновление вида (микрофон, участники) — только у уже запущенного звонка.
        if (intent.action == ACTION_REFRESH) {
            if (started) post()
            return START_NOT_STICKY
        }
        peerName = intent.getStringExtra(EXTRA_PEER_NAME) ?: ""
        room = intent.getBooleanExtra(EXTRA_ROOM, false)
        startMs = intent.getLongExtra(EXTRA_START_MS, System.currentTimeMillis())
        startForegroundCompat()
        started = true
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        runCatching { unregisterReceiver(endReceiver) }
        runCatching { wifi?.takeIf { it.isHeld }?.release() }
        wifi = null
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startForegroundCompat() {
        ensureChannel()
        val notif = buildNotification()
        // Android 14 бросает SecurityException, если тип «микрофон» заявлен без
        // выданного разрешения, а 12+ — если старт пришёлся на фон. Исключение из
        // сервиса роняет весь процесс; без переднего плана звонок проживёт меньше,
        // но это лучше, чем упасть посреди него.
        runCatching { startForegroundTyped(notif) }.onFailure { stopSelf() }
    }

    private fun startForegroundTyped(notif: Notification) {
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.R ->
                startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q ->
                startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            else -> startForeground(NOTIF_ID, notif)
        }
    }

    private fun ensureChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(
                    NotificationChannel(
                        CHANNEL,
                        getString(R.string.notif_call_channel),
                        NotificationManager.IMPORTANCE_LOW,
                    ).apply {
                        description = getString(R.string.notif_call_channel_desc)
                        setShowBadge(false)
                    },
                )
            }
        }
    }

    private fun filter() = IntentFilter().apply {
        addAction(ACTION_END_CALL)
        addAction(ACTION_MUTE)
    }

    /** Перерисовать уведомление: состояние микрофона или состав комнаты сменились. */
    private fun post() {
        runCatching { NotificationManagerCompat.from(this).notify(NOTIF_ID, buildNotification()) }
    }

    private fun buildNotification(): Notification {
        val openPi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).apply { flags = Intent.FLAG_ACTIVITY_SINGLE_TOP },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        fun broadcast(action: String, code: Int) = PendingIntent.getBroadcast(
            this,
            code,
            Intent(action).apply { setPackage(packageName) },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val endPi = broadcast(ACTION_END_CALL, 0)
        val model = AppModel.instance
        val mic = getString(if (model.muted) R.string.notif_mic_on else R.string.notif_mic_off)
        val b = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_glyph)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setWhen(startMs)
            .setShowWhen(true)
            .setUsesChronometer(true)
            .setContentIntent(openPi)
        if (room) {
            // Комната: шаблон звонка подписал бы кнопку «Завершить», а из комнаты выходят.
            val inside = model.roomMembers.map { model.peerName(it.peer) }
            b.setContentTitle(peerName)
                .setContentText(getString(R.string.notif_voice_room, (listOf(getString(R.string.notif_room_you)) + inside).joinToString(", ")))
                .addAction(0, mic, broadcast(ACTION_MUTE, 1))
                .addAction(0, getString(R.string.notif_leave), endPi)
        } else {
            val peer = Person.Builder().setName(peerName.ifEmpty { getString(R.string.common_contact) }).setImportant(true).build()
            b.setStyle(NotificationCompat.CallStyle.forOngoingCall(peer, endPi))
                .setContentText(getString(R.string.notif_ongoing))
                .addAction(0, mic, broadcast(ACTION_MUTE, 1))
        }
        return b.build()
    }

    companion object {
        private const val CHANNEL = "peerage_call"
        const val NOTIF_ID = 2
        private const val ACTION_END_CALL = "chat.peerage.END_CALL"
        private const val ACTION_MUTE = "chat.peerage.CALL_MUTE"
        private const val ACTION_REFRESH = "chat.peerage.CALL_REFRESH"
        private const val EXTRA_PEER_NAME = "peer_name"
        private const val EXTRA_ROOM = "room"
        private const val EXTRA_START_MS = "start_ms"

        /** `name` — собеседник личного звонка или название группы (`room = true`). */
        fun start(ctx: Context, name: String, room: Boolean = false) {
            val i = Intent(ctx, CallService::class.java).apply {
                putExtra(EXTRA_PEER_NAME, name)
                putExtra(EXTRA_ROOM, room)
                putExtra(EXTRA_START_MS, System.currentTimeMillis())
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ctx.startForegroundService(i)
            } else {
                ctx.startService(i)
            }
        }

        /** Микрофон или состав комнаты сменились — перерисовать (не запущен — ничего). */
        fun refresh(ctx: Context) {
            runCatching { ctx.startService(Intent(ctx, CallService::class.java).setAction(ACTION_REFRESH)) }
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, CallService::class.java))
        }
    }
}
