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

/**
 * Foreground-сервис активного звонка/комнаты.
 *
 * Показывает ongoing-уведомление с хронометром и кнопкой «Завершить».
 * Тип микрофон (API 30+) — Android знает, что мы законно пишем звук.
 * Без этого сервиса Android (особенно MIUI/Doze) убьёт процесс через ~1 мин
 * после погашения экрана → аудио стопится посреди разговора.
 */
class CallService : Service() {
    private var startMs = 0L
    private var peerName = ""
    private var wifi: WifiManager.WifiLock? = null

    private val endReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            AppModel.instance.leaveRoom()
        }
    }

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(endReceiver, IntentFilter(ACTION_END_CALL), RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(endReceiver, IntentFilter(ACTION_END_CALL))
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
        peerName = intent?.getStringExtra(EXTRA_PEER_NAME) ?: ""
        startMs = intent?.getLongExtra(EXTRA_START_MS, System.currentTimeMillis())
            ?: System.currentTimeMillis()
        startForegroundCompat()
        return START_STICKY
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

    private fun buildNotification(): Notification {
        val title = if (peerName.isNotEmpty()) getString(R.string.notif_call_with, peerName) else getString(R.string.notif_voice_chat)

        val openPi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).apply { flags = Intent.FLAG_ACTIVITY_SINGLE_TOP },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val endPi = PendingIntent.getBroadcast(
            this,
            0,
            Intent(ACTION_END_CALL).apply { setPackage(packageName) },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        @Suppress("DEPRECATION")
        val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL)
        } else {
            Notification.Builder(this)
        }

        b.setContentTitle(title)
            .setSmallIcon(android.R.drawable.stat_sys_phone_call)
            .setOngoing(true)
            .setWhen(startMs)
            .setUsesChronometer(true)
            .setContentIntent(openPi)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            b.addAction(
                Notification.Action.Builder(
                    android.graphics.drawable.Icon.createWithResource(
                        this,
                        android.R.drawable.ic_delete,
                    ),
                    getString(R.string.notif_end),
                    endPi,
                ).build(),
            )
        } else {
            @Suppress("DEPRECATION")
            b.addAction(0, getString(R.string.notif_end), endPi)
        }

        return b.build()
    }

    companion object {
        private const val CHANNEL = "peerage_call"
        const val NOTIF_ID = 2
        private const val ACTION_END_CALL = "chat.peerage.END_CALL"
        private const val EXTRA_PEER_NAME = "peer_name"
        private const val EXTRA_START_MS = "start_ms"

        fun start(ctx: Context, peerName: String) {
            val i = Intent(ctx, CallService::class.java).apply {
                putExtra(EXTRA_PEER_NAME, peerName)
                putExtra(EXTRA_START_MS, System.currentTimeMillis())
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ctx.startForegroundService(i)
            } else {
                ctx.startService(i)
            }
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, CallService::class.java))
        }
    }
}
