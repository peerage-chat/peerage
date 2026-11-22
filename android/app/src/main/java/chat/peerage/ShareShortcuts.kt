package chat.peerage

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.Typeface
import androidx.compose.ui.graphics.toArgb
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat
import chat.peerage.ui.avatarColor

/**
 * Последние чаты в системном меню «Поделиться» (sharing shortcuts, шаг 7d). Публикуются
 * ТОЛЬКО при включённом [AppModel.shareChatsInMenu]: ярлык несёт имя и фото чата, и система
 * показывает их вне приложения. Выключение снимает всё опубликованное.
 *
 * Ярлыки той же категории, что `share-target` в `res/xml/shortcuts.xml`: выбор такого чата
 * в меню присылает `ACTION_SEND` с id ярлыка, и приложение открывает чат сразу, без выбора.
 */
internal object ShareShortcuts {
    private const val CATEGORY = "chat.peerage.SHARE_TARGET"
    private const val MAX = 8
    private const val ICON_PX = 216

    /** Последнее опубликованное: менять ярлыки на каждом входящем незачем (система их ещё и ограничивает). */
    @Volatile private var published: String? = null

    /** Привести опубликованное к настройке и к списку чатов. Зовётся с фонового потока. */
    fun sync(ctx: Context, model: AppModel) {
        if (!model.shareChatsInMenu) {
            if (published != OFF) {
                runCatching {
                    // Id — ДО очистки и вместе с кэшированными: долговечный ярлык, который система
                    // успела закэшировать, переживает снятие динамических и иначе остался бы в меню.
                    val ids = ShortcutManagerCompat.getShortcuts(
                        ctx,
                        ShortcutManagerCompat.FLAG_MATCH_DYNAMIC or ShortcutManagerCompat.FLAG_MATCH_CACHED,
                    ).map { it.id }
                    ShortcutManagerCompat.removeAllDynamicShortcuts(ctx)
                    ShortcutManagerCompat.removeLongLivedShortcuts(ctx, ids)
                }
                published = OFF
            }
            return
        }
        val chats = recent(model)
        val signature = chats.joinToString("|") { "${it.id}:${it.name}:${it.avatar?.size ?: 0}" }
        if (signature == published) return
        val shortcuts = chats.mapIndexed { rank, c -> build(ctx, c, rank) }
        if (runCatching { ShortcutManagerCompat.setDynamicShortcuts(ctx, shortcuts) }.getOrDefault(false)) published = signature
    }

    private const val OFF = "off"

    private data class Entry(val id: String, val chat: ChatTarget, val name: String, val avatar: ByteArray?)

    /** Свежие чаты с перепиской: без «Сохранённого» и архивных групп (писать в них нельзя). */
    private fun recent(model: AppModel): List<Entry> =
        model.chatSummaries.values.toList()
            .filter { it.lastTime > 0 }
            .sortedByDescending { it.lastTime }
            .mapNotNull { s ->
                val chat = ChatTarget(s.id, s.group)
                if (s.group) {
                    val g = model.groups.firstOrNull { it.gid == s.id }?.takeUnless { it.archived } ?: return@mapNotNull null
                    Entry(shortcutIdOf(chat), chat, g.name, model.groupPhoto(g.gid))
                } else {
                    if (model.isSaved(s.id)) return@mapNotNull null
                    val c = model.contacts.firstOrNull { it.key == s.id } ?: return@mapNotNull null
                    Entry(shortcutIdOf(chat), chat, c.displayName, model.contactAvatars[c.avatarId])
                }
            }
            .take(MAX)

    private fun build(ctx: Context, e: Entry, rank: Int): ShortcutInfoCompat {
        // Тап по ярлыку в меню значка — открыть чат; выбор в «Поделиться» придёт ACTION_SEND.
        val open = Intent(ACTION_OPEN_CHAT).setClass(ctx, MainActivity::class.java)
            .putExtra(EXTRA_SHORTCUT_CHAT, e.id)
        return ShortcutInfoCompat.Builder(ctx, e.id)
            .setShortLabel(e.name.ifBlank { "…" })
            .setLongLived(true)
            .setRank(rank)
            .setCategories(setOf(CATEGORY))
            .setIcon(IconCompat.createWithAdaptiveBitmap(icon(e)))
            .setIntent(open)
            .build()
    }

    /** Знак во всю площадь: фото по центру или буква на цвете аватара (как в приложении). */
    private fun icon(e: Entry): Bitmap {
        val out = Bitmap.createBitmap(ICON_PX, ICON_PX, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(out)
        val photo = e.avatar?.let { runCatching { BitmapFactory.decodeByteArray(it, 0, it.size) }.getOrNull() }
        if (photo != null) {
            val side = minOf(photo.width, photo.height)
            val src = Rect((photo.width - side) / 2, (photo.height - side) / 2, (photo.width + side) / 2, (photo.height + side) / 2)
            canvas.drawBitmap(photo, src, Rect(0, 0, ICON_PX, ICON_PX), Paint(Paint.FILTER_BITMAP_FLAG))
            return out
        }
        canvas.drawColor(avatarColor(e.name).toArgb())
        val p = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = android.graphics.Color.WHITE
            textSize = ICON_PX * 0.36f
            typeface = Typeface.DEFAULT_BOLD
            textAlign = Paint.Align.CENTER
        }
        val letter = e.name.trim().take(1).uppercase().ifEmpty { "?" }
        canvas.drawText(letter, ICON_PX / 2f, ICON_PX / 2f - (p.descent() + p.ascent()) / 2, p)
        return out
    }
}
