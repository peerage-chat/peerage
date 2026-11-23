package chat.peerage

import android.content.Context
import android.util.Log
import java.io.File

/**
 * Раскладка профилей на диске (ADR-015 F4, шаг беты 8.7): всё, что относится к профилю, —
 * база, блобы, ключ хранилища, паспорт `profile.json` — внутри `files/profiles/<слот>/`.
 * Общего списка профилей нет. Дополнительные профили (после беты) получат непрозрачные
 * имена из пароля; главный — постоянное `main`: что он есть, и так видно по установленному
 * приложению, а о дополнительных его имя ничего не говорит.
 *
 * Вне профилей — только общее для приложения (файл последнего падения, маркеры библиотек).
 */
object Profiles {
    private const val ROOT = "profiles"
    private const val MAIN = "main"
    private const val PART = "$MAIN.part"

    /** Не данные профиля: остаются в корне `files/` (`profileInstalled` — маркер AndroidX). */
    private val APP_WIDE = setOf(ROOT, LogExport.CRASH_FILE, "profileInstalled")

    @Volatile private var main: File? = null

    /** Каталог главного профиля. Первое обращение переносит туда прежние файлы. */
    fun main(ctx: Context): File = main ?: synchronized(this) {
        main ?: migrate(ctx.applicationContext.filesDir).also { main = it }
    }

    /**
     * Перенести данные из корня `files` в `profiles/main/`. Каждый файл — переименованием
     * в `main.part`, затем `main.part` → `main` одним переименованием: убитый посреди процесс
     * на следующем старте продолжит с того же места, а наполовину собранный профиль
     * под именем `main` не появится.
     */
    internal fun migrate(files: File): File {
        val dir = File(File(files, ROOT), MAIN)
        if (dir.isDirectory) return dir
        val part = File(File(files, ROOT), PART).apply { mkdirs() }
        val stuck = files.listFiles()?.filter { it.name !in APP_WIDE }?.count { !it.renameTo(File(part, it.name)) } ?: 0
        if (stuck > 0) Log.w("Profiles", "не перенесено в профиль: $stuck")
        check(part.renameTo(dir)) { "profiles/$PART → $MAIN" }
        return dir
    }
}
