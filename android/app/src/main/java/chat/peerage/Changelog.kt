package chat.peerage

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** Одна версия журнала: что заметно человеку, без техники. */
data class ChangelogEntry(
    val version: String,
    val code: Long,
    val date: String,
    val summary: String,
    val new: List<String>,
    val fixed: List<String>,
)

/**
 * Журнал изменений в приложении (п. 51, app-ui.md → «Что нового»). Источник один —
 * `assets/changelog/<язык>.json`; из английского `scripts/release.sh` собирает заметки
 * релиза GitHub. Язык файла — строка `changelog_lang` из ресурсов: какой язык выбрали
 * ресурсы, тот и журнал; нет файла — английский.
 *
 * После обновления — полоса «Обновлено до X» один раз ([pending]); на свежей установке её нет.
 */
object Changelog {
    private val json = Json { ignoreUnknownKeys = true }
    private const val K_SEEN = "changelog_seen_code"

    /** Запись для полосы на главной; null — показывать нечего. */
    var pending by mutableStateOf<ChangelogEntry?>(null)
        private set
    private var app: Context? = null
    private var installed = 0L

    fun init(ctx: Context) {
        if (app != null) return
        val c = ctx.applicationContext
        app = c
        val info = c.packageManager.getPackageInfo(c.packageName, 0)
        installed = androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(info)
        val fresh = info.firstInstallTime == info.lastUpdateTime
        val prefs = c.getSharedPreferences("settings", Context.MODE_PRIVATE)
        val seen = prefs.getLong(K_SEEN, 0)
        pending = toShow(seen, installed, fresh, load(c))
        // Свежая установка или показать нечего — запомнить сразу: полоса не всплывёт потом.
        if (pending == null && seen < installed) prefs.edit().putLong(K_SEEN, installed).apply()
    }

    /** Полосу закрыли или открыли «Что нового» — больше для этой версии не показывать. */
    fun seen() {
        pending = null
        app?.getSharedPreferences("settings", Context.MODE_PRIVATE)?.edit()?.putLong(K_SEEN, installed)?.apply()
    }

    /** Журнал до установленной версии включительно, свежая сверху. */
    fun load(ctx: Context): List<ChangelogEntry> {
        val lang = ctx.getString(R.string.changelog_lang)
        val text = runCatching { read(ctx, lang) }.getOrNull() ?: runCatching { read(ctx, "en") }.getOrNull() ?: return emptyList()
        val code = androidx.core.content.pm.PackageInfoCompat.getLongVersionCode(ctx.packageManager.getPackageInfo(ctx.packageName, 0))
        return upTo(parse(text), code)
    }

    private fun read(ctx: Context, lang: String) =
        ctx.assets.open("changelog/$lang.json").use { String(it.readBytes(), Charsets.UTF_8) }

    // ── Чистые правила ──────────────────────────────────────────────────────────

    /** versionCode из versionName — та же формула, что в build.gradle.kts: M·10000 + m·100 + p. */
    fun versionCode(v: String): Long? {
        val p = v.split('.').map { it.toLongOrNull() ?: return null }
        if (p.size != 3) return null
        return p[0] * 10_000 + p[1] * 100 + p[2]
    }

    /** Разбор файла журнала; запись без версии или полей пропускается. Свежая — сверху. */
    fun parse(text: String): List<ChangelogEntry> = runCatching {
        json.parseToJsonElement(text).jsonObject["versions"]!!.jsonArray.mapNotNull { el ->
            runCatching { entry(el.jsonObject) }.getOrNull()
        }.sortedByDescending { it.code }
    }.getOrDefault(emptyList())

    private fun entry(o: JsonObject): ChangelogEntry? {
        val version = o.str("version") ?: return null
        return ChangelogEntry(
            version = version,
            code = versionCode(version) ?: return null,
            date = o.str("date") ?: return null,
            summary = o.str("summary") ?: return null,
            new = o.list("new"),
            fixed = o.list("fixed"),
        )
    }

    private fun JsonObject.str(k: String) = this[k]?.jsonPrimitive?.contentOrNull
    private fun JsonObject.list(k: String) = this[k]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }.orEmpty()

    /** Только версии, которые у человека уже есть: сборка 0.3.x не рассказывает о 0.4.0. */
    fun upTo(entries: List<ChangelogEntry>, installed: Long) = entries.filter { it.code <= installed }

    /**
     * Что показать полосой после запуска: свежую запись новее [seen] и не новее [installed].
     * Свежая установка — ничего (знакомиться с приложением журнал не помогает).
     */
    fun toShow(seen: Long, installed: Long, freshInstall: Boolean, entries: List<ChangelogEntry>): ChangelogEntry? {
        if (seen == 0L && freshInstall) return null
        return upTo(entries, installed).firstOrNull { it.code > seen }
    }
}
