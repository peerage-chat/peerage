package chat.peerage

import android.content.Context
import android.os.Build
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * «Сохранить лог» (план беты, шаг 1): текстовый файл, который человек сам
 * пересылает разработчику. Сервера нет, падения никуда не уходят — это
 * единственный путь узнать, что случилось у бета-юзера.
 *
 * Источник — logcat своего процесса: туда пишут и Rust (`android_logger`, тег
 * `core`), и Kotlin, а свои записи приложение читает без разрешений. Падение
 * прошлого запуска в logcat уже не видно — его сохраняет [install] в файл.
 *
 * Чего в файле быть не должно: содержимого сообщений, ключей и сидов — в лог их не
 * пишет никто; на выходе ещё и [sanitize] — длинные hex (ключи, id групп)
 * укорачиваются, адреса сети вырезаются: адрес — ровно то, что смена адреса
 * (ADR-007) прячет, и в пересланном файле ему не место.
 */
object LogExport {
    private const val CRASH_FILE = "last-crash.txt"

    @Volatile private var installed = false

    /** Записывать падение процесса в файл, прежде чем его увидит система. */
    fun install(ctx: Context) {
        if (installed) return
        installed = true
        val dir = ctx.applicationContext.filesDir
        val prev = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching {
                val text = "${stamp()} поток ${t.name}\n${e.stackTraceToString()}"
                StagedFiles.writeAtomic(File(dir, CRASH_FILE), text.toByteArray())
            }
            prev?.uncaughtException(t, e)
        }
    }

    /** Текст лога целиком, уже очищенный. Читает logcat — звать не с UI-потока. */
    fun collect(ctx: Context): String {
        val sb = StringBuilder()
        val ver = runCatching {
            ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName
        }.getOrNull() ?: "?"
        sb.append("${ctx.getString(R.string.app_name)} $ver · Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            .append(" · ${Build.MANUFACTURER} ${Build.MODEL} · ${Build.SUPPORTED_ABIS.firstOrNull()}\n")
            .append("снят ${stamp()}\n\n")
        File(ctx.filesDir, CRASH_FILE).takeIf { it.exists() }?.let {
            sb.append("== прошлое падение ==\n").append(it.readText()).append("\n\n")
        }
        sb.append("== журнал этого запуска ==\n")
        sb.append(readOwnLogcat())
        return sanitize(sb.toString())
    }

    private fun readOwnLogcat(): String = runCatching {
        val p = ProcessBuilder(
            "logcat",
            "-d",
            "-v",
            "threadtime",
            "--pid=${android.os.Process.myPid()}",
        ).redirectErrorStream(true).start()
        p.inputStream.bufferedReader().use { it.readText() }.also { p.waitFor() }
    }.getOrElse { "logcat недоступен: ${it.message}\n" }

    private val longHex = Regex("""(?i)\b([0-9a-f]{6})[0-9a-f]{10,}\b""")
    private val ipv6 = Regex("""(?i)\b[0-9a-f]{1,4}(?::[0-9a-f]{0,4}){3,7}\b""")

    /** Укоротить ключи и id (первые 6 знаков — хватит, чтобы различать строки
     *  лога между собой) и вырезать адреса сети. */
    fun sanitize(text: String): String = text
        .replace(ipv6, "[адрес]")
        .replace(longHex) { it.groupValues[1] + "…" }

    /** Имя файла по умолчанию в окне «Сохранить как». */
    fun fileName(): String =
        "peerage-log-" + SimpleDateFormat("yyyyMMdd-HHmm", Locale.ROOT).format(Date()) + ".txt"

    private fun stamp(): String = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT).format(Date())
}
