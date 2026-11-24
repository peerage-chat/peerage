package chat.peerage

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors

/** Новая версия из `update.json` (docs/release.md, формат 1), уже для ABI этого устройства. */
data class UpdateInfo(
    val versionName: String,
    val versionCode: Long,
    val tag: String,
    val file: String,
    val size: Long,
    val sha256: String,
    val certSha256: String,
)

/** Разбор ответов GitHub — на входе, один раз; дальше данным верят. */
object UpdateManifest {
    private val json = Json { ignoreUnknownKeys = true }
    private val safeName = Regex("[A-Za-z0-9._-]+")
    private val hex64 = Regex("[0-9a-f]{64}")

    /** Тег новейшего релиза, у которого есть `update.json` (pre-release тоже: до 1.0 все такие). */
    fun releaseTag(releasesJson: String): String? {
        val list = runCatching { json.parseToJsonElement(releasesJson) as? JsonArray }.getOrNull() ?: return null
        return list.asSequence()
            .mapNotNull { it as? JsonObject }
            .filter { it["draft"]?.jsonPrimitive?.booleanOrNull != true }
            .firstOrNull { r -> r["assets"]?.jsonArray?.any { it.jsonObject.str("name") == "update.json" } == true }
            ?.str("tag_name")
            ?.takeIf { safeName.matches(it) }
    }

    /** Манифест → обновление для первого подходящего ABI из [abis]; неизвестный формат и мусор — null. */
    fun parse(manifestJson: String, abis: List<String>): UpdateInfo? = runCatching {
        val m = json.parseToJsonElement(manifestJson).jsonObject
        if (m["format"]?.jsonPrimitive?.intOrNull != 1) return null
        val apks = m["apks"]?.jsonArray?.map { it.jsonObject }.orEmpty()
        val apk = abis.firstNotNullOfOrNull { abi -> apks.firstOrNull { it.str("abi") == abi } } ?: return null
        val tag = m.str("tag")?.takeIf { safeName.matches(it) } ?: return null
        val file = apk.str("file")?.takeIf { safeName.matches(it) && it.endsWith(".apk") } ?: return null
        UpdateInfo(
            versionName = m.str("version_name") ?: return null,
            versionCode = m["version_code"]?.jsonPrimitive?.longOrNull ?: return null,
            tag = tag,
            file = file,
            size = apk["size"]?.jsonPrimitive?.longOrNull?.takeIf { it > 0 } ?: return null,
            sha256 = apk.str("sha256")?.takeIf { hex64.matches(it) } ?: return null,
            certSha256 = m.str("cert_sha256")?.takeIf { hex64.matches(it) } ?: return null,
        )
    }.getOrNull()

    private fun JsonObject.str(key: String): String? = this[key]?.jsonPrimitive?.contentOrNull
}

/**
 * Что решено о проверке: согласие (null — ещё не спрашивали), когда проверяли, какую
 * версию отложили «Позже» и до каких пор. Время — unix-секунды.
 */
data class UpdateFacts(
    val consent: Boolean? = null,
    val checkedAt: Long = 0,
    val laterCode: Long = 0,
    val laterUntil: Long = 0,
) {
    /** Пора проверить сама: согласие есть, с прошлой проверки прошли сутки. */
    fun due(now: Long) = consent == true && now >= checkedAt + DAY

    /** Показывать ли полосу про [u]: «Позже» прячет только эту версию и только на сутки. */
    fun shows(u: UpdateInfo, now: Long) = u.versionCode != laterCode || now >= laterUntil

    fun later(u: UpdateInfo, now: Long) = copy(laterCode = u.versionCode, laterUntil = now + DAY)

    fun save(p: SharedPreferences) = p.edit()
        .putInt(K_CONSENT, consent?.let { if (it) 1 else 0 } ?: -1)
        .putLong(K_CHECKED, checkedAt)
        .putLong(K_LATER_CODE, laterCode)
        .putLong(K_LATER_UNTIL, laterUntil)
        .apply()

    companion object {
        const val DAY = 86_400L
        private const val K_CONSENT = "update_consent"
        private const val K_CHECKED = "update_checked_at"
        private const val K_LATER_CODE = "update_later_code"
        private const val K_LATER_UNTIL = "update_later_until"

        fun load(p: SharedPreferences) = UpdateFacts(
            consent = when (p.getInt(K_CONSENT, -1)) {
                1 -> true
                0 -> false
                else -> null
            },
            checkedAt = p.getLong(K_CHECKED, 0),
            laterCode = p.getLong(K_LATER_CODE, 0),
            laterUntil = p.getLong(K_LATER_UNTIL, 0),
        )
    }
}

/** Где сейчас обновление. Полосы на главной и строка в «О приложении» читают только это. */
sealed interface UpdateState {
    data object Idle : UpdateState
    data object Checking : UpdateState
    data object UpToDate : UpdateState

    /** Проверка не дошла до GitHub. На главной не видно — только в «О приложении». */
    data object CheckFailed : UpdateState
    data class Available(val info: UpdateInfo) : UpdateState
    data class Downloading(val info: UpdateInfo, val done: Long) : UpdateState

    /** Загрузка оборвалась: «Повторить» качает заново. */
    data class DownloadFailed(val info: UpdateInfo) : UpdateState

    /** Файл сверен; установщик закрыли без установки — «Установить» откроет его снова. */
    data class Ready(val info: UpdateInfo, val file: File) : UpdateState

    /** Файл не совпал с манифестом или с подписью установленного — удалён. */
    data object Rejected : UpdateState
}

/**
 * Проверка обновлений (п. 46, docs/release.md → «Проверка обновлений в приложении»).
 * Сеть и диск — на своём потоке; состояние пишется на главном. Источник — релизы
 * GitHub; молча ничего не ставится, установку подтверждает системный установщик.
 */
object Updates {
    private const val REPO = "peerage-chat/peerage"
    private const val MAX_TEXT = 1 shl 20

    var state by mutableStateOf<UpdateState>(UpdateState.Idle)
        private set
    var facts by mutableStateOf(UpdateFacts())
        private set

    private var app: Context? = null
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor { Thread(it, "updates").apply { isDaemon = true } }

    @Volatile private var cancelled = false

    private fun now() = System.currentTimeMillis() / 1000
    private fun prefs(ctx: Context) = ctx.getSharedPreferences("settings", Context.MODE_PRIVATE)
    private fun dir(ctx: Context) = File(ctx.cacheDir, "updates")

    /** Процесс поднялся: факты с диска, недокачанное и уже поставленное — прочь. */
    fun init(ctx: Context) {
        if (app != null) return
        val c = ctx.applicationContext
        app = c
        facts = UpdateFacts.load(prefs(c))
        worker.execute { dir(c).listFiles()?.forEach { it.delete() } }
    }

    /** Приложение на переднем плане: проверить, если пора. */
    fun onForeground() {
        if (facts.due(now())) check()
    }

    /** Ответ на разовый вопрос или тумблер в «О приложении». */
    fun setConsent(on: Boolean) {
        change { copy(consent = on) }
        if (on && facts.due(now())) check()
    }

    /** «Позже» на полосе: эта версия — через сутки. */
    fun later() {
        val info = when (val s = state) {
            is UpdateState.Available -> s.info
            is UpdateState.Ready -> s.info
            is UpdateState.DownloadFailed -> s.info
            else -> null
        }
        if (info != null) change { later(info, now()) }
        if (state is UpdateState.Rejected || state is UpdateState.DownloadFailed) state = UpdateState.Idle
    }

    /** Проверить сейчас. Ручная проверка — сама по себе согласие на этот запрос. */
    fun check() {
        val ctx = app ?: return
        if (state is UpdateState.Checking || state is UpdateState.Downloading) return
        state = UpdateState.Checking
        worker.execute {
            val next = runCatching { fetch(ctx) }.getOrElse { UpdateState.CheckFailed }
            main.post {
                change { copy(checkedAt = now()) }
                // Пока шла проверка, могли начать загрузку — её не перебиваем.
                if (state is UpdateState.Checking) state = next
            }
        }
    }

    /** «Обновить» / «Повторить»: скачать, сверить, отдать установщику. */
    fun download() {
        val ctx = app ?: return
        val info = when (val s = state) {
            is UpdateState.Available -> s.info
            is UpdateState.DownloadFailed -> s.info
            else -> return
        }
        cancelled = false
        state = UpdateState.Downloading(info, 0)
        worker.execute {
            val file = File(dir(ctx), info.file)
            val next = runCatching {
                if (fetchApk(info, file) && verify(ctx, file, info)) {
                    install(ctx, file)
                    UpdateState.Ready(info, file)
                } else if (cancelled) {
                    file.delete()
                    UpdateState.Available(info)
                } else {
                    file.delete()
                    UpdateState.Rejected
                }
            }.getOrElse {
                file.delete()
                if (cancelled) UpdateState.Available(info) else UpdateState.DownloadFailed(info)
            }
            main.post { state = next }
        }
    }

    fun cancel() {
        cancelled = true
    }

    /** «Установить» после закрытого установщика: та же сессия заново. */
    fun installAgain() {
        val ctx = app ?: return
        val s = state as? UpdateState.Ready ?: return
        worker.execute { runCatching { install(ctx, s.file) } }
    }

    /** Ответ установщика ([UpdateReceiver]). */
    internal fun onInstallerStatus(ctx: Context, status: Int, confirm: Intent?) {
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION && confirm != null) {
            // Человек смотрит на приложение — он только что нажал «Обновить». Если ушёл,
            // Android не даст открыть окно из фона, и останется полоса «Установить».
            runCatching { ctx.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
        }
        // Успех заменяет процесс; отмена и ошибка оставляют полосу «Установить».
    }

    private fun change(f: UpdateFacts.() -> UpdateFacts) {
        facts = facts.f()
        app?.let { facts.save(prefs(it)) }
    }

    // ── Поток "updates" ─────────────────────────────────────────────────────────

    private fun fetch(ctx: Context): UpdateState {
        val tag = UpdateManifest.releaseTag(text("https://api.github.com/repos/$REPO/releases?per_page=10"))
            ?: return UpdateState.UpToDate
        val info = UpdateManifest.parse(
            text("https://github.com/$REPO/releases/download/$tag/update.json"),
            Build.SUPPORTED_ABIS.toList(),
        ) ?: return UpdateState.UpToDate
        val installed = installedInfo(ctx)
        val newer = info.versionCode > androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(installed)
        // Чужая подпись (своя сборка, debug) — обновление всё равно не встанет; не предлагаем.
        val ours = certs(installed) == setOf(info.certSha256)
        return if (newer && ours) UpdateState.Available(info) else UpdateState.UpToDate
    }

    private fun open(url: String) = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 15_000
        readTimeout = 30_000
        setRequestProperty("User-Agent", "Peerage-Android")
    }

    private fun text(url: String): String {
        val c = open(url)
        try {
            if (c.responseCode != 200) error("HTTP ${c.responseCode}")
            val bytes = c.inputStream.use { it.readNBytesCompat(MAX_TEXT) }
            return String(bytes, Charsets.UTF_8)
        } finally {
            c.disconnect()
        }
    }

    /** Скачать APK, считая SHA-256 по ходу. false — отменили или не совпало с манифестом. */
    private fun fetchApk(info: UpdateInfo, file: File): Boolean {
        file.parentFile?.mkdirs()
        val c = open("https://github.com/$REPO/releases/download/${info.tag}/${info.file}")
        val md = MessageDigest.getInstance("SHA-256")
        var done = 0L
        try {
            if (c.responseCode != 200) error("HTTP ${c.responseCode}")
            c.inputStream.use { input ->
                file.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var shown = 0L
                    while (true) {
                        if (cancelled) return false
                        val n = input.read(buf)
                        if (n < 0) break
                        done += n
                        if (done > info.size) return false
                        md.update(buf, 0, n)
                        out.write(buf, 0, n)
                        if (done - shown >= 256 * 1024) {
                            shown = done
                            val d = done
                            main.post { if (state is UpdateState.Downloading) state = UpdateState.Downloading(info, d) }
                        }
                    }
                }
            }
        } finally {
            c.disconnect()
        }
        return done == info.size && md.digest().toHex() == info.sha256
    }

    /** APK — наш пакет, новее установленного, подписан тем же ключом (не полем манифеста). */
    private fun verify(ctx: Context, file: File, info: UpdateInfo): Boolean {
        val pm = ctx.packageManager
        val arch = archiveInfo(pm, file) ?: return false
        val installed = installedInfo(ctx)
        val code = androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(arch)
        val have = certs(installed)
        return arch.packageName == ctx.packageName &&
            code == info.versionCode &&
            code > androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(installed) &&
            have.isNotEmpty() && certs(arch) == have
    }

    private fun install(ctx: Context, file: File) {
        val pi = ctx.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
            setAppPackageName(ctx.packageName)
            setSize(file.length())
        }
        val id = pi.createSession(params)
        pi.openSession(id).use { s ->
            file.inputStream().use { input ->
                s.openWrite("peerage.apk", 0, file.length()).use { out ->
                    input.copyTo(out)
                    s.fsync(out)
                }
            }
            // MUTABLE: установщик дописывает в интент статус и окно подтверждения.
            val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
            val pending = PendingIntent.getBroadcast(ctx, id, Intent(ctx, UpdateReceiver::class.java), flags)
            s.commit(pending.intentSender)
        }
    }

    private fun installedInfo(ctx: Context): PackageInfo =
        ctx.packageManager.getPackageInfo(ctx.packageName, signingFlag())

    @Suppress("DEPRECATION")
    private fun archiveInfo(pm: PackageManager, file: File): PackageInfo? = pm.getPackageArchiveInfo(file.path, signingFlag())

    @Suppress("DEPRECATION")
    private fun signingFlag() =
        if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES

    /** SHA-256 сертификатов подписи — в той же записи, что `cert_sha256` манифеста. */
    @Suppress("DEPRECATION")
    private fun certs(p: PackageInfo): Set<String> {
        val sigs = if (Build.VERSION.SDK_INT >= 28) p.signingInfo?.apkContentsSigners else p.signatures
        return sigs.orEmpty().map { MessageDigest.getInstance("SHA-256").digest(it.toByteArray()).toHex() }.toSet()
    }

    private fun ByteArray.toHex() = joinToString("") { "%02x".format(it) }

    private fun java.io.InputStream.readNBytesCompat(limit: Int): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        val buf = ByteArray(8 * 1024)
        while (out.size() < limit) {
            val n = read(buf, 0, minOf(buf.size, limit - out.size()))
            if (n < 0) break
            out.write(buf, 0, n)
        }
        return out.toByteArray()
    }
}

/** Статус сессии установки → [Updates]. */
class UpdateReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        val confirm = androidx.core.content.IntentCompat.getParcelableExtra(intent, Intent.EXTRA_INTENT, Intent::class.java)
        Updates.onInstallerStatus(ctx, status, confirm)
    }
}
