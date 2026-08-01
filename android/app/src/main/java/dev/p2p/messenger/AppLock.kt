package dev.p2p.messenger

import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Блокировка приложения (M4).
 *
 * **Чем подтверждаем — устройством, а не собой.** Свой PIN означал бы своё хранилище
 * секрета, свою проверку и свои ошибки в них; для мессенджера, который обещает
 * безопасность, это худший размен. Поэтому спрашиваем системный экран блокировки
 * ([KeyguardManager]) — там и биометрия, и PIN/графический ключ, всё уже проверенное
 * платформой. Ноль дополнительных зависимостей: важно для воспроизводимой сборки и
 * F-Droid.
 *
 * **Почему не androidx.biometric.** Он требует `FragmentActivity` и тянет фрагменты
 * ради одного диалога; платформенный `BiometricPrompt` доступен только с API 28, а у
 * нас minSdk 24. `KeyguardManager` покрывает весь диапазон и делает ровно то же.
 */
object AppLock {
    /**
     * Сколько секунд после ухода в фон блокировка НЕ срабатывает.
     *
     * Без этой форы приложение блокировалось бы при каждом выборе файла или снимке с
     * камеры — системный пикер это тоже «уход в фон». Полминуты закрывают такие
     * отлучки и не спасают того, кто отдал разблокированный телефон чужому.
     */
    private const val GRACE_SECONDS = 30L

    private const val PREFS = "p2p_settings"
    private const val KEY_ENABLED = "app_lock"

    /** Включена ли блокировка. */
    var enabled by mutableStateOf(false)
        private set

    /** Сейчас заблокировано — UI обязан показать замок вместо содержимого. */
    var locked by mutableStateOf(false)
        private set

    private var backgroundedAt = 0L

    fun load(ctx: Context) {
        enabled = prefs(ctx).getBoolean(KEY_ENABLED, false)
        // При старте процесса блокируем сразу: «уже разблокировано» после перезапуска
        // означало бы, что защита снимается простым убийством приложения.
        locked = enabled
    }

    /** Есть ли на устройстве экран блокировки. Без него включать нечего. */
    fun deviceSecure(ctx: Context): Boolean =
        (ctx.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager).isDeviceSecure

    fun setEnabled(ctx: Context, on: Boolean) {
        enabled = on
        prefs(ctx).edit().putBoolean(KEY_ENABLED, on).apply()
        if (!on) locked = false
    }

    /** Ушли в фон: засекаем время, чтобы отличить короткую отлучку от настоящей. */
    fun onBackground() {
        backgroundedAt = System.currentTimeMillis()
    }

    /** Вернулись: блокируем, если отсутствие было дольше форы. */
    fun onForeground() {
        if (!enabled || locked) return
        val away = (System.currentTimeMillis() - backgroundedAt) / 1000
        if (backgroundedAt > 0 && away >= GRACE_SECONDS) locked = true
    }

    fun unlock() {
        locked = false
        backgroundedAt = 0
    }

    /** Системный запрос подтверждения личности. `null` — устройство без блокировки. */
    fun credentialIntent(ctx: Context): Intent? {
        val km = ctx.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
        @Suppress("DEPRECATION") // единственный путь, покрывающий minSdk 24
        return km.createConfirmDeviceCredentialIntent(
            ctx.getString(R.string.lock_title),
            ctx.getString(R.string.lock_subtitle),
        )
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}
