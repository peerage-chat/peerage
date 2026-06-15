package dev.p2p.messenger

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
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
 * Держит два лока:
 *  - PARTIAL_WAKE_LOCK — CPU не засыпает, потоки ygg/звонка продолжают работать;
 *  - WifiLock(FULL_HIGH_PERF) — Wi-Fi не уходит в power-save, пакеты не теряются.
 */
class ConnService : Service() {
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null

    override fun onCreate() {
        super.onCreate()
        startForegroundCompat()
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "p2p:mesh").apply {
            setReferenceCounted(false)
            acquire()
        }
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        wifi = wm.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "p2p:mesh").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    // Перезапускаться, если система всё же убьёт сервис под давлением памяти.
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

    override fun onDestroy() {
        runCatching { wake?.takeIf { it.isHeld }?.release() }
        runCatching { wifi?.takeIf { it.isHeld }?.release() }
        wake = null
        wifi = null
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

    private fun buildNotification(): Notification {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val ch = NotificationChannel(CHANNEL, "Соединение", NotificationManager.IMPORTANCE_LOW).apply {
                description = "p2p в сети — mesh-узел активен"
                setShowBadge(false)
            }
            nm.createNotificationChannel(ch)
        }
        @Suppress("DEPRECATION")
        val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL)
        } else {
            Notification.Builder(this)
        }
        return b
            .setContentTitle("p2p messenger")
            .setContentText("В сети — держим mesh-соединение")
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setOngoing(true)
            .build()
    }

    companion object {
        private const val CHANNEL = "p2p_conn"
        private const val NOTIF_ID = 1

        fun start(ctx: Context) {
            val i = Intent(ctx, ConnService::class.java)
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
