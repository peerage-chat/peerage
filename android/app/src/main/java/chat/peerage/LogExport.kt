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
    const val CRASH_FILE = "last-crash.txt"

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
        // Снимок — первым: по нему видно, в каком состоянии была сеть, когда сняли лог.
        sb.append("== снимок ==\n").append(snapshot(AppModel.instance.diagnosticsJson())).append("\n")
        File(ctx.filesDir, CRASH_FILE).takeIf { it.exists() }?.let {
            sb.append("== прошлое падение (Kotlin) ==\n").append(it.readText()).append("\n\n")
        }
        exits(ctx)?.let { sb.append("== завершения прошлых запусков ==\n").append(it).append("\n") }
        sb.append("== журнал этого запуска ==\n")
        sb.append(readOwnLogcat())
        return sanitize(sb.toString())
    }

    /**
     * Снимок движка построчно: «раздел: поле число · …». JSON разбирается без org.json, чтобы
     * функцию можно было проверить на JVM: вложенность — одна ступень, значения — числа.
     */
    fun snapshot(json: String?): String {
        if (json.isNullOrBlank() || json == "{}") return "движок не поднят\n"
        val sb = StringBuilder()
        val section = Regex(""""(\w+)"\s*:\s*\{([^{}]*)\}""")
        val field = Regex(""""(\w+)"\s*:\s*(-?[0-9.eE+]+|true|false)""")
        for (s in section.findAll(json)) {
            val fields = field.findAll(s.groupValues[2]).joinToString(" · ") { "${it.groupValues[1]} ${it.groupValues[2]}" }
            sb.append(s.groupValues[1]).append(": ").append(fields).append('\n')
        }
        return sb.toString()
    }

    /**
     * Почему завершались прошлые запуски (API 30+): здесь видно и нативное падение Rust/Go,
     * которого обработчик Kotlin не ловит, и ANR, и убийство системой по памяти.
     */
    private fun exits(ctx: Context): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        val am = ctx.getSystemService(android.app.ActivityManager::class.java) ?: return null
        val list = runCatching { am.getHistoricalProcessExitReasons(ctx.packageName, 0, EXITS_MAX) }.getOrNull()
        if (list.isNullOrEmpty()) return null
        val fmt = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT)
        return list.joinToString("\n") { e ->
            val head = "${fmt.format(Date(e.timestamp))} ${exitReason(e.reason)} · статус ${e.status}" +
                (e.description?.let { " · $it" } ?: "")
            // След ANR — текст стеков; нативное падение — protobuf-надгробие, без инструментов
            // его не разобрать, поэтому только размер: по нему видно, что оно было.
            val trace = when (e.reason) {
                android.app.ApplicationExitInfo.REASON_ANR ->
                    runCatching { e.traceInputStream?.bufferedReader()?.use { it.readText().take(TRACE_MAX) } }.getOrNull()
                android.app.ApplicationExitInfo.REASON_CRASH_NATIVE ->
                    runCatching { e.traceInputStream?.use { "надгробие ${it.readBytes().size} байт" } }.getOrNull()
                else -> null
            }
            if (trace.isNullOrBlank()) head else "$head\n$trace"
        } + "\n"
    }

    private fun exitReason(r: Int): String = when (r) {
        android.app.ApplicationExitInfo.REASON_CRASH -> "падение"
        android.app.ApplicationExitInfo.REASON_CRASH_NATIVE -> "нативное падение"
        android.app.ApplicationExitInfo.REASON_ANR -> "ANR"
        android.app.ApplicationExitInfo.REASON_LOW_MEMORY -> "нехватка памяти"
        android.app.ApplicationExitInfo.REASON_SIGNALED -> "сигнал"
        android.app.ApplicationExitInfo.REASON_EXIT_SELF -> "сам"
        android.app.ApplicationExitInfo.REASON_USER_REQUESTED -> "остановлен человеком"
        android.app.ApplicationExitInfo.REASON_USER_STOPPED -> "остановлен в настройках"
        android.app.ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE -> "перерасход ресурсов"
        android.app.ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "сбой запуска"
        android.app.ApplicationExitInfo.REASON_PERMISSION_CHANGE -> "смена разрешений"
        android.app.ApplicationExitInfo.REASON_DEPENDENCY_DIED -> "умерла зависимость"
        android.app.ApplicationExitInfo.REASON_FREEZER -> "заморожен системой"
        android.app.ApplicationExitInfo.REASON_PACKAGE_STATE_CHANGE -> "смена состояния пакета"
        android.app.ApplicationExitInfo.REASON_PACKAGE_UPDATED -> "обновление приложения"
        else -> "причина $r"
    }

    private const val EXITS_MAX = 8
    private const val TRACE_MAX = 6000

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
