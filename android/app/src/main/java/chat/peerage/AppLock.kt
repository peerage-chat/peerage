package chat.peerage

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import android.hardware.biometrics.BiometricManager.Authenticators
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
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
 * ради одного диалога. С Android 10 — платформенный `BiometricPrompt` ([prompt]): сразу
 * отпечаток или лицо, PIN устройства — запасным. Ниже (minSdk 24) — экран подтверждения
 * `KeyguardManager` ([credentialIntent]), он покрывает весь диапазон.
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

    private const val PREFS = "settings"
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
        return km.createConfirmDeviceCredentialIntent(promptTitle(ctx), null)
    }

    /**
     * Системный запрос на Android 10+: биометрия, PIN/ключ устройства — запасным, оба рисует
     * система. false — запроса не будет (старый Android или нет блокировки экрана): звать
     * [credentialIntent]. [onResult] — на главном потоке; false — запрос закрыли.
     */
    fun prompt(activity: Activity, onResult: (Boolean) -> Unit): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q || !deviceSecure(activity)) return false
        val builder = BiometricPrompt.Builder(activity).setTitle(promptTitle(activity))
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            builder.setAllowedAuthenticators(Authenticators.BIOMETRIC_WEAK or Authenticators.DEVICE_CREDENTIAL)
        } else {
            @Suppress("DEPRECATION") // на Android 10 PIN запасным включается только так
            builder.setDeviceCredentialAllowed(true)
        }
        builder.build().authenticate(
            CancellationSignal(),
            activity.mainExecutor,
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) = onResult(true)

                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) = onResult(false)
            },
        )
        return true
    }

    private fun promptTitle(ctx: Context) = ctx.getString(R.string.lock_prompt, ctx.getString(R.string.app_name))

    private fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}
