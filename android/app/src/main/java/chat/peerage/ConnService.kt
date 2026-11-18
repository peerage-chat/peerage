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

    // Перезапускаться, если система всё же убьёт сервис под давлением памяти.
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

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

    private fun buildNotification(): Notification {
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val ch = NotificationChannel(CHANNEL, getString(R.string.notif_conn_channel), NotificationManager.IMPORTANCE_LOW).apply {
                description = getString(R.string.notif_conn_channel_desc)
                setShowBadge(false)
            }
            nm.createNotificationChannel(ch)
        }

        val openPi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).apply { flags = Intent.FLAG_ACTIVITY_SINGLE_TOP },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val disconnectPi = PendingIntent.getBroadcast(
            this,
            0,
            Intent(ACTION_DISCONNECT).apply { setPackage(packageName) },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        @Suppress("DEPRECATION")
        val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL)
        } else {
            Notification.Builder(this)
        }

        b.setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.notif_conn_text))
            .setSmallIcon(R.drawable.ic_glyph)
            .setOngoing(true)
            .setContentIntent(openPi)

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            b.addAction(
                Notification.Action.Builder(
                    android.graphics.drawable.Icon.createWithResource(this, android.R.drawable.ic_lock_power_off),
                    getString(R.string.notif_disconnect),
                    disconnectPi,
                ).build(),
            )
        } else {
            @Suppress("DEPRECATION")
            b.addAction(0, getString(R.string.notif_disconnect), disconnectPi)
        }

        return b.build()
    }

    companion object {
        private const val CHANNEL = "conn"

        /** Системные настройки категории «связи» (Android 8+: категория; раньше — уведомления приложения). */
        fun channelSettings(ctx: Context): Intent =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Intent(android.provider.Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS)
                    .putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, ctx.packageName)
                    .putExtra(android.provider.Settings.EXTRA_CHANNEL_ID, CHANNEL)
            } else {
                Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS, android.net.Uri.fromParts("package", ctx.packageName, null))
            }
        private const val NOTIF_ID = 1
        private const val ACTION_DISCONNECT = "chat.peerage.DISCONNECT_MESH"

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
