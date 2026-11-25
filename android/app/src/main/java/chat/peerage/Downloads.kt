package chat.peerage

import android.content.ContentValues
import android.content.Context
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import java.io.File

/**
 * Файл из чата — в общие «Загрузки» (п. 60), в папку приложения: Download/Peerage. С Android 10
 * MediaStore кладёт туда без разрешений; раньше нужна запись в общее хранилище (её спрашивает
 * меню до вызова).
 */
object Downloads {
    /** Скопировать [src] в «Загрузки» под именем [name]; true — легло. */
    fun save(ctx: Context, src: File, name: String, mime: String): Boolean = runCatching {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) viaMediaStore(ctx, src, name, mime) else viaFile(ctx, src, name, mime)
    }.getOrDefault(false)

    private fun folder(ctx: Context) = ctx.getString(R.string.app_name)

    @androidx.annotation.RequiresApi(Build.VERSION_CODES.Q)
    private fun viaMediaStore(ctx: Context, src: File, name: String, mime: String): Boolean {
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime.ifBlank { "application/octet-stream" })
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${Environment.DIRECTORY_DOWNLOADS}/${folder(ctx)}")
            // Пока пишется — не виден другим приложениям: оборванная запись не выглядит файлом.
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val resolver = ctx.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: return false
        val ok = runCatching {
            resolver.openOutputStream(uri)?.use { out -> src.inputStream().use { it.copyTo(out) } } != null
        }.getOrDefault(false)
        if (!ok) {
            resolver.delete(uri, null, null)
            return false
        }
        resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        return true
    }

    @Suppress("DEPRECATION") // до Android 10 другого пути в общие «Загрузки» нет
    private fun viaFile(ctx: Context, src: File, name: String, mime: String): Boolean {
        val dir = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), folder(ctx))
        if (!dir.isDirectory && !dir.mkdirs()) return false
        val dst = freeName(dir, name)
        src.copyTo(dst)
        android.media.MediaScannerConnection.scanFile(ctx, arrayOf(dst.absolutePath), arrayOf(mime.ifBlank { null }), null)
        return true
    }

    /** «отчёт.pdf» занят — «отчёт (1).pdf», как делает MediaStore с Android 10. */
    internal fun freeName(dir: File, name: String): File {
        val first = File(dir, name)
        if (!first.exists()) return first
        val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
        val (base, ext) = name.substring(0, dot) to name.substring(dot)
        return generateSequence(1) { it + 1 }.map { File(dir, "$base ($it)$ext") }.first { !it.exists() }
    }
}
